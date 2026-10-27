//! Isolated Git worktrees for workers: creation on a new branch, the branch's commits, a
//! merge into the coordinator checkout and removal. Reads are bounded like every Git
//! inventory; changes run to completion and are never killed half-way.
use crate::process;
use std::path::Path;
use std::process::{Command, Stdio};

const OUTPUT: usize = 65_536;
const MESSAGE: usize = 512;

/// A Git command that changes the repository: its success, standard output and the first
/// part of its error output for the refusal.
fn change(dir: &Path, args: &[&str]) -> Result<(bool, String, String), String> {
    let output = Command::new("git")
        .args(["-c", "core.fsmonitor=false", "-C"])
        .arg(dir)
        .args(args)
        .env("GIT_TERMINAL_PROMPT", "0")
        .stdin(Stdio::null())
        .output()
        .map_err(|error| format!("Git failed: {error}"))?;
    let text = |bytes: &[u8], bound: usize| {
        let text = String::from_utf8_lossy(&bytes[..bytes.len().min(bound)]);
        text.trim().to_string()
    };
    Ok((
        output.status.success(),
        text(&output.stdout, OUTPUT),
        text(&output.stderr, MESSAGE),
    ))
}

fn changed(dir: &Path, args: &[&str], what: &str) -> Result<String, String> {
    match change(dir, args)? {
        (true, out, _) => Ok(out),
        (false, _, error) if error.is_empty() => Err(format!("{what} failed")),
        (false, _, error) => Err(format!("{what} failed: {error}")),
    }
}

fn raw(dir: &Path, args: &[&str]) -> Result<Option<String>, String> {
    Ok(process::git(dir, args, OUTPUT)?.map(|b| String::from_utf8_lossy(&b).into_owned()))
}

/// One-line output, trimmed; `-z` lists go through [`raw`], whose leading spaces matter.
fn read(dir: &Path, args: &[&str]) -> Result<Option<String>, String> {
    Ok(raw(dir, args)?.map(|out| out.trim().to_string()))
}

/// The commit `HEAD` names in `dir`, if it is a Git checkout with one.
pub fn head(dir: &Path) -> Result<Option<String>, String> {
    read(dir, &["rev-parse", "--verify", "-q", "HEAD"])
}

/// The commit a branch names in the repository at `dir`.
pub fn branch_head(dir: &Path, branch: &str) -> Result<Option<String>, String> {
    read(
        dir,
        &[
            "rev-parse",
            "--verify",
            "-q",
            &format!("refs/heads/{branch}"),
        ],
    )
}

/// The top folder of the Git checkout holding `dir`, if any.
pub fn top(dir: &Path) -> Result<Option<String>, String> {
    read(dir, &["rev-parse", "--show-toplevel"])
}

/// Whether Git ignores `path` (relative to `dir`), so the checkout never shows it.
pub fn ignored(dir: &Path, path: &str) -> Result<bool, String> {
    Ok(change(dir, &["check-ignore", "-q", "--no-index", path])?.0)
}

/// Whether a merge, rebase, cherry-pick or revert is in progress in the checkout at `dir`.
pub fn busy(dir: &Path) -> Result<bool, String> {
    for name in ["MERGE_HEAD", "CHERRY_PICK_HEAD", "REVERT_HEAD"] {
        if read(dir, &["rev-parse", "--verify", "-q", name])?.is_some() {
            return Ok(true);
        }
    }
    for name in ["rebase-merge", "rebase-apply"] {
        let Some(path) = read(dir, &["rev-parse", "--git-path", name])? else {
            continue;
        };
        if dir.join(path).exists() {
            return Ok(true);
        }
    }
    Ok(false)
}

/// Uncommitted paths of the checkout at `dir`, untracked ones included.
pub fn uncommitted(dir: &Path) -> Result<Vec<String>, String> {
    let listed = raw(
        dir,
        &[
            "status",
            "--porcelain=v1",
            "-z",
            "--untracked-files=all",
            "--no-renames",
        ],
    )?
    .ok_or("Git status failed")?;
    Ok(listed
        .split('\0')
        .filter(|entry| entry.len() > 3)
        .map(|entry| entry[3..].to_string())
        .collect())
}

/// Create a worktree at `path` on the new branch `branch` from the current `HEAD` of the
/// repository at `repository`; returns the base commit. An existing branch or folder is
/// refused, never reused.
pub fn add(repository: &Path, path: &Path, branch: &str) -> Result<String, String> {
    let base = head(repository)?.ok_or("the repository has no commit to branch from")?;
    if branch_head(repository, branch)?.is_some() {
        return Err(format!(
            "branch {branch} exists already; remove it or discard the task's earlier worktree"
        ));
    }
    if path.exists() {
        return Err(format!("{} exists already", path.display()));
    }
    let target = path.to_string_lossy();
    changed(
        repository,
        &["worktree", "add", "-q", "-b", branch, &target, &base],
        "creating the worktree",
    )?;
    Ok(base)
}

/// Commit every change of the worktree at `dir` on its branch; returns the branch head,
/// unchanged when there was nothing to commit.
pub fn commit(dir: &Path, message: &str) -> Result<String, String> {
    if !uncommitted(dir)?.is_empty() {
        changed(dir, &["add", "-A"], "staging the worktree")?;
        changed(
            dir,
            &["commit", "-q", "-m", message],
            "committing the worktree",
        )?;
    }
    head(dir)?.ok_or_else(|| "the worktree has no commit".into())
}

/// Files `branch` changes relative to its merge base with `HEAD`.
pub fn branch_files(repository: &Path, branch: &str) -> Result<Vec<String>, String> {
    let names = raw(
        repository,
        &[
            "diff",
            "--name-only",
            "--no-renames",
            "-z",
            &format!("HEAD...{branch}"),
        ],
    )?
    .ok_or("Git diff failed")?;
    Ok(names
        .split('\0')
        .filter(|n| !n.is_empty())
        .map(str::to_string)
        .collect())
}

/// Whether `branch` merges into `HEAD` without conflicts, tried without touching the
/// checkout; `Err(paths)` lists the conflicted files.
pub fn trial(repository: &Path, branch: &str) -> Result<Result<(), Vec<String>>, String> {
    let (clean, out, error) = change(
        repository,
        &[
            "merge-tree",
            "--write-tree",
            "--name-only",
            "--no-messages",
            "HEAD",
            branch,
        ],
    )?;
    if clean {
        return Ok(Ok(()));
    }
    let mut lines = out.lines();
    // Exit 1 with a tree id first means conflicts; anything else is a failure.
    match lines.next() {
        Some(tree) if !out.is_empty() && tree.len() >= 40 => {
            let mut paths: Vec<String> = lines
                .filter(|l| !l.is_empty())
                .map(str::to_string)
                .collect();
            paths.dedup();
            Ok(Err(paths))
        }
        _ => Err(format!(
            "trying the merge failed (Git 2.38 or newer is needed): {error}"
        )),
    }
}

/// Merge `branch` into the checkout at `repository` with a merge commit; returns it. A merge
/// that stops is aborted, so the checkout is left as it was.
pub fn merge(repository: &Path, branch: &str, message: &str) -> Result<String, String> {
    let (merged, _, error) = change(
        repository,
        &["merge", "--no-ff", "--no-edit", "-q", "-m", message, branch],
    )?;
    if !merged {
        let _ = change(repository, &["merge", "--abort"]);
        return Err(format!("merging {branch} failed: {error}"));
    }
    head(repository)?.ok_or_else(|| "the merge left no commit".into())
}

/// Remove the clean worktree at `path` of the repository at `repository`, then its branch
/// when it is merged. Returns what was kept and why; nothing unmerged or uncommitted is
/// deleted.
pub fn remove(repository: &Path, path: &Path, branch: &str) -> Vec<String> {
    let mut kept = Vec::new();
    if path.exists() {
        let target = path.to_string_lossy();
        if let Err(error) = changed(
            repository,
            &["worktree", "remove", &target],
            "removing the worktree",
        ) {
            kept.push(format!("{} kept: {error}", path.display()));
        }
    }
    if branch_head(repository, branch).ok().flatten().is_some()
        && let Err(error) = changed(
            repository,
            &["branch", "-d", "-q", branch],
            "deleting the branch",
        )
    {
        kept.push(format!("branch {branch} kept: {error}"));
    }
    kept
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::fs;

    struct Repository(std::path::PathBuf);
    impl Drop for Repository {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn git(dir: &Path, args: &[&str]) {
        let status = Command::new("git")
            .args([
                "-c",
                "user.name=n",
                "-c",
                "user.email=n@x",
                "-c",
                "commit.gpgsign=false",
                "-C",
            ])
            .arg(dir)
            .args(args)
            .status()
            .unwrap();
        assert!(status.success());
    }

    #[test]
    fn uncommitted_paths_keep_their_first_character() {
        let path = std::env::temp_dir().join(format!("craft-worktree-{}", std::process::id()));
        fs::create_dir(&path).unwrap();
        let repository = Repository(crate::state::canonical_root(&path).unwrap());
        let root = &repository.0;
        fs::create_dir(root.join("src")).unwrap();
        fs::write(root.join("src/x.txt"), "one\n").unwrap();
        git(root, &["init", "-q"]);
        // Commits made here run without the test's command-line identity.
        for (key, value) in [
            ("user.name", "n"),
            ("user.email", "n@x"),
            ("commit.gpgsign", "false"),
        ] {
            git(root, &["config", key, value]);
        }
        git(root, &["add", "-A"]);
        git(root, &["commit", "-qm", "init"]);
        // Status ` M` starts with a space, which trimming the output would remove.
        fs::write(root.join("src/x.txt"), "two\n").unwrap();
        fs::write(root.join("new.txt"), "new\n").unwrap();
        let mut found = uncommitted(root).unwrap();
        found.sort();
        assert_eq!(found, ["new.txt", "src/x.txt"]);
        assert!(!busy(root).unwrap());
        // A worktree branch changes files the coordinator can see before merging.
        let place = root.join("wt");
        let base = add(root, &place, "craft/p/t").unwrap();
        assert_eq!(head(root).unwrap().as_deref(), Some(base.as_str()));
        assert!(
            add(root, &root.join("wt2"), "craft/p/t")
                .unwrap_err()
                .contains("exists already")
        );
        fs::write(place.join("src/x.txt"), "three\n").unwrap();
        let committed = commit(&place, "craft p/t: change").unwrap();
        assert_ne!(committed, base);
        assert_eq!(branch_files(root, "craft/p/t").unwrap(), ["src/x.txt"]);
        assert_eq!(trial(root, "craft/p/t").unwrap(), Ok(()));
        let kept = remove(root, &place, "craft/p/t");
        assert!(
            kept.iter().any(|k| k.contains("branch craft/p/t kept")),
            "{kept:?}"
        );
        assert!(!place.exists());
    }
}
