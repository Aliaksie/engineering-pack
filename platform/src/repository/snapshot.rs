//! Bounded Git worktree snapshots: revision, status, index and working-tree content identity.
use crate::{
    process,
    search::{SearchError, open_source, safe_file},
    state,
};
use craft_engine::values::lower_hex;
use craft_engine::{repository, routing::private_path};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    fs::{self, Metadata},
    io::{ErrorKind, Read},
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

const FILES: usize = 2000;
const BYTES: u64 = 16 * 1024 * 1024;
const FILE_BYTES: u64 = 2 * 1024 * 1024;
const MILLISECONDS: u64 = 5000;
const OUTPUT: usize = 1024 * 1024;

fn git(root: &Path, args: &[&str], max_bytes: usize) -> Option<String> {
    // A timed-out, overflowing or failed query is unavailable, never partial.
    process::git(root, args, max_bytes)
        .ok()
        .flatten()
        .map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
}

/// The symbolic ref of `HEAD` (`HEAD` when detached), or `null`.
pub fn checkout(root: &Path) -> Value {
    git(root, &["rev-parse", "--symbolic-full-name", "HEAD"], 16384)
        .map_or(Value::Null, |text| json!(text.trim()))
}

fn hex(text: &str) -> bool {
    matches!(text.len(), 40 | 64) && lower_hex(text, text.len())
}

/// `text.slice(units)` in UTF-16 code units.
fn after(text: &str, units: usize) -> String {
    let utf16: Vec<u16> = text.encode_utf16().skip(units).collect();
    String::from_utf16_lossy(&utf16)
}

fn unchanged(before: &Metadata, after: &Metadata) -> bool {
    let same = before.len() == after.len() && before.modified().ok() == after.modified().ok();
    #[cfg(unix)]
    let same = {
        use std::os::unix::fs::MetadataExt;
        same && before.ino() == after.ino()
            && (before.ctime(), before.ctime_nsec()) == (after.ctime(), after.ctime_nsec())
    };
    same
}

enum Entry {
    Missing,
    Hashed,
    Limit,
    Failed,
}

struct Content {
    root: PathBuf,
    digest: Sha256,
    files: usize,
    bytes: u64,
}

impl Content {
    fn inspect(&mut self, relative: &str) -> Entry {
        let parts: Vec<&str> = relative.split('/').collect();
        if Path::new(relative).is_absolute() || relative.starts_with('/') || parts.contains(&"..") {
            return Entry::Failed;
        }
        let mut path = self.root.clone();
        for part in &parts[..parts.len() - 1] {
            path.push(part);
            match fs::symlink_metadata(&path) {
                Ok(meta) if meta.is_dir() && !meta.file_type().is_symlink() => {}
                Err(error) if error.kind() == ErrorKind::NotFound => return Entry::Missing,
                _ => return Entry::Failed,
            }
        }
        path.push(parts[parts.len() - 1]);
        let before = match fs::symlink_metadata(&path) {
            Ok(before) => before,
            Err(error) if error.kind() == ErrorKind::NotFound => return Entry::Missing,
            Err(_) => return Entry::Failed,
        };
        if before.file_type().is_symlink() {
            return match fs::read_link(&path) {
                Ok(target) => {
                    self.digest.update(b"symlink:");
                    self.digest.update(target.to_string_lossy().as_bytes());
                    Entry::Hashed
                }
                Err(error) if error.kind() == ErrorKind::NotFound => Entry::Missing,
                Err(_) => Entry::Failed,
            };
        }
        if !before.is_file() {
            // Directories are gitlinks: a separate repository, outside this snapshot.
            return Entry::Failed;
        }
        if before.len() > FILE_BYTES || self.bytes + before.len() > BYTES {
            return Entry::Limit;
        }
        let mut file = match open_source(&path) {
            Ok(file) => file,
            Err(error) if error.kind() == ErrorKind::NotFound => return Entry::Missing,
            Err(_) => return Entry::Failed,
        };
        let mut content = Vec::with_capacity(before.len() as usize + 1);
        let read = (&mut file).take(before.len() + 1).read_to_end(&mut content);
        let after = file.metadata();
        match (read, after) {
            (Ok(count), Ok(after))
                if count as u64 == before.len() && unchanged(&before, &after) =>
            {
                self.bytes += count as u64;
                self.digest.update(b"file:");
                self.digest.update(&content);
                Entry::Hashed
            }
            _ => Entry::Failed,
        }
    }
}

/// One worktree: `{checkout, head, status, gitAvailable, fingerprints}`.
pub fn state(root: &Path) -> Value {
    let mut issues: Vec<String> = Vec::new();
    let identity = git(
        root,
        &["rev-parse", "HEAD", "--symbolic-full-name", "HEAD"],
        OUTPUT,
    );
    let lines: Option<Vec<String>> =
        identity.map(|t| t.trim_end().split('\n').map(str::to_string).collect());
    // Keep the original queries for failed or unexpected output (including unborn HEAD).
    let combined = lines.filter(|l| l.len() <= 2 && hex(&l[0]));
    let (head, checkout) = match combined {
        Some(lines) => (
            json!(lines[0]),
            json!(lines.get(1).cloned().unwrap_or_default()),
        ),
        None => (
            git(root, &["rev-parse", "--verify", "HEAD"], OUTPUT)
                .map_or(Value::Null, |h| json!(h.trim())),
            self::checkout(root),
        ),
    };
    let status = git(
        root,
        &["status", "--porcelain=v1", "-z", "--untracked-files=all"],
        OUTPUT,
    );
    let index = git(root, &["ls-files", "--stage", "-z"], OUTPUT);
    let inventory = process::git(
        root,
        &[
            "ls-files",
            "--cached",
            "--others",
            "--exclude-standard",
            "-z",
        ],
        OUTPUT,
    )
    .ok()
    .flatten();
    if status.is_none() {
        issues.push("Git status could not be inspected".into());
    }
    if index.is_none() {
        issues.push("Git index could not be inspected".into());
    }
    if inventory.is_none() {
        issues.push("Working-tree inventory could not be inspected".into());
    }
    let mut content = Content {
        root: root.to_path_buf(),
        digest: Sha256::new(),
        files: 0,
        bytes: 0,
    };
    let started = Instant::now();
    let mut paths: Vec<(String, bool)> = inventory
        .as_deref()
        .unwrap_or_default()
        .split(|b| *b == 0)
        .filter(|p| !p.is_empty())
        .map(|p| match std::str::from_utf8(p) {
            Ok(text) => (text.to_string(), true),
            Err(_) => (String::from_utf8_lossy(p).into_owned(), false),
        })
        .collect();
    paths.sort_by(|a, b| {
        a.0.encode_utf16()
            .cmp(b.0.encode_utf16())
            .then(b.1.cmp(&a.1))
    });
    paths.dedup_by(|a, b| a.0 == b.0);
    for (relative, valid) in &paths {
        // Checkpoint output cannot be part of the state it describes.
        if private_path(relative) {
            continue;
        }
        if content.files >= FILES || started.elapsed() > Duration::from_millis(MILLISECONDS) {
            issues.push("Working-tree file or time limit reached".into());
            break;
        }
        content.files += 1;
        content
            .digest
            .update(serde_json::to_string(relative).expect("strings serialize"));
        // A name that is not UTF-8 cannot be addressed by its decoded spelling.
        let entry = if *valid {
            content.inspect(relative)
        } else {
            Entry::Failed
        };
        match entry {
            Entry::Hashed => {}
            Entry::Missing => content.digest.update(b"missing:"),
            Entry::Limit => {
                issues.push("Working-tree byte limit reached".into());
                break;
            }
            Entry::Failed => {
                if issues.len() < 10 {
                    issues.push(format!("Working-tree inspection failed: {relative}"));
                }
            }
        }
    }
    let status_entries: Vec<Value> = status
        .as_deref()
        .unwrap_or_default()
        .split('\0')
        .filter(|e| !e.is_empty() && !private_path(&after(e, 3)))
        .map(|e| json!(e))
        .collect();
    let index_digest = index.as_ref().map_or(Value::Null, |index| {
        let kept: Vec<&str> = index
            .split('\0')
            .filter(|e| {
                !e.is_empty()
                    && !private_path(&after(
                        e,
                        e.encode_utf16().position(|u| u == 9).map_or(0, |i| i + 1),
                    ))
            })
            .collect();
        json!(craft_engine::hash(kept.join("\0").as_bytes()))
    });
    json!({
        "checkout": checkout,
        "head": head,
        "status": status_entries,
        "gitAvailable": status.is_some() && index.is_some(),
        "fingerprints": {
            "index": index_digest,
            "workingTree": if inventory.is_some() { json!(craft_engine::hex_bytes(&content.digest.finalize())) } else { Value::Null },
            "complete": issues.is_empty(),
            "issues": issues,
            "files": content.files,
            "bytes": content.bytes,
            "limits": {"files": FILES, "bytes": BYTES, "fileBytes": FILE_BYTES, "milliseconds": MILLISECONDS},
        }
    })
}

/// Uncommitted files read by [`changes`]; more leave the changes incomplete.
const CHANGED_FILES: usize = 256;

/// What changed in one worktree: files committed since a base revision, and uncommitted
/// files with the digest of their current content.
pub struct Changes {
    pub head: Value,
    /// `None` when there was no base or Git could not compare it with `HEAD`.
    pub committed: Option<Vec<String>>,
    pub uncommitted: Vec<(String, String)>,
    /// False when Git status failed or more than the bounded number of files changed.
    pub complete: bool,
}

/// The current content identity of one worktree file: a SHA-256 of its bytes, `missing`, or
/// for a file too large or unreadable to hash, of its size and modification time.
fn content_digest(root: &Path, relative: &str) -> String {
    let mut content = Content {
        root: root.to_path_buf(),
        digest: Sha256::new(),
        files: 0,
        bytes: 0,
    };
    match content.inspect(relative) {
        Entry::Hashed => craft_engine::hex_bytes(&content.digest.finalize()),
        Entry::Missing => "missing".into(),
        Entry::Limit | Entry::Failed => {
            let meta = fs::symlink_metadata(root.join(relative)).ok();
            let identity = meta.map(|m| format!("{}:{:?}", m.len(), m.modified().ok()));
            craft_engine::hash(identity.unwrap_or_default().as_bytes())
        }
    }
}

/// Files changed in the worktree at `root` since the commit `base`, without private state.
pub fn changes(root: &Path, base: Option<&str>) -> Changes {
    let head = git(root, &["rev-parse", "--verify", "HEAD"], 16384)
        .map_or(Value::Null, |h| json!(h.trim()));
    let committed = base.filter(|b| hex(b)).and_then(|base| {
        if head == base {
            return Some(Vec::new());
        }
        let range = format!("{base}..HEAD");
        let names = git(
            root,
            &["diff", "--name-only", "--no-renames", "-z", &range],
            OUTPUT,
        )?;
        Some(
            names
                .split('\0')
                .filter(|n| !n.is_empty() && !private_path(n))
                .map(str::to_string)
                .collect(),
        )
    });
    let status = git(
        root,
        &["status", "--porcelain=v1", "-z", "--untracked-files=all"],
        OUTPUT,
    );
    let mut complete = status.is_some();
    let mut paths: Vec<String> = Vec::new();
    let mut entries = status.as_deref().unwrap_or_default().split('\0');
    while let Some(entry) = entries.next() {
        if entry.is_empty() {
            continue;
        }
        paths.push(after(entry, 3));
        // A rename or copy is followed by the path it came from.
        if matches!(entry.as_bytes().first(), Some(b'R' | b'C')) {
            paths.extend(entries.next().map(str::to_string));
        }
    }
    paths.retain(|p| !private_path(p));
    paths.sort();
    paths.dedup();
    if paths.len() > CHANGED_FILES {
        complete = false;
        paths.truncate(CHANGED_FILES);
    }
    let uncommitted = paths
        .into_iter()
        .map(|path| {
            let digest = content_digest(root, &path);
            (path, digest)
        })
        .collect();
    Changes {
        head,
        committed,
        uncommitted,
        complete,
    }
}

/// [`changes`] for the task's source: the worktree at `root`, or each selected repository
/// with its paths prefixed by the repository path. `bases` maps a repository path (`""` for
/// `root`) to its base revision.
pub fn task_changes(
    root: &Path,
    repositories: Option<&Value>,
    bases: &dyn Fn(&str) -> Option<String>,
) -> Result<Vec<(String, Changes)>, SearchError> {
    let Some(repositories) = repositories else {
        return Ok(vec![(String::new(), changes(root, bases("").as_deref()))]);
    };
    let scopes = repository::scopes(repositories)?;
    Ok(roots(root, &scopes)?
        .into_iter()
        .map(|(path, root)| {
            let found = changes(&root, bases(&path).as_deref());
            (path, found)
        })
        .collect())
}

/// Explicitly selected worktree roots below `root`; never sibling repositories.
pub fn roots(root: &Path, scopes: &[String]) -> Result<Vec<(String, PathBuf)>, SearchError> {
    let root = state::canonical_root(root)?;
    let mut seen = Vec::new();
    let mut roots = Vec::new();
    for scope in scopes {
        // Reject aliases so a saved relative selection cannot silently change its target.
        let Some(candidate) = safe_file(&root, scope)? else {
            return Err("repository scope must not traverse symlinks".into());
        };
        let canonical = state::canonical_root(&candidate)?;
        if canonical != candidate {
            return Err("repository scope must name its directory exactly, without aliases".into());
        }
        let top = process::git(&canonical, &["rev-parse", "--show-toplevel"], 16384)?
            .ok_or_else(|| format!("repository scope is not a Git worktree root: {scope}"))?;
        let top = String::from_utf8_lossy(&top);
        if state::canonical_root(Path::new(top.trim()))? != canonical {
            return Err(format!("repository scope is not a Git worktree root: {scope}").into());
        }
        if seen.contains(&canonical) {
            return Err("duplicate repository root".into());
        }
        seen.push(canonical.clone());
        roots.push((scope.clone(), canonical));
    }
    Ok(roots)
}

/// The task's source: the worktree at `root`, or the selected repositories as one snapshot.
/// `checkout_only` observes only `{checkout}`.
pub fn workspace(
    root: &Path,
    repositories: Option<&Value>,
    checkout_only: bool,
) -> Result<Value, SearchError> {
    let Some(repositories) = repositories else {
        return Ok(if checkout_only {
            json!({"checkout": checkout(root)})
        } else {
            state(root)
        });
    };
    let scopes = repository::scopes(repositories)?;
    let roots = roots(root, &scopes)?;
    if checkout_only {
        let entries: Vec<_> = roots
            .iter()
            .map(|(path, root)| (path.clone(), checkout(root)))
            .collect();
        return Ok(json!({"checkout": repository::checkout(&entries)}));
    }
    let entries: Vec<Value> = roots
        .iter()
        .map(|(path, root)| {
            let mut entry = json!({"path": path});
            for (key, value) in state(root).as_object().unwrap() {
                entry[key] = value.clone();
            }
            entry
        })
        .collect();
    Ok(repository::snapshot(&entries)?)
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::process::Command;
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT: AtomicU64 = AtomicU64::new(0);

    struct Repository(PathBuf);
    impl Repository {
        fn new(files: &[&str]) -> Self {
            let path = std::env::temp_dir().join(format!(
                "craft-snapshot-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir(&path).unwrap();
            let root = state::canonical_root(&path).unwrap();
            for file in files {
                fs::write(root.join(file), "content\n").unwrap();
            }
            let git = |args: &[&str]| {
                let status = Command::new("git")
                    .args(["-c", "user.name=n", "-c", "user.email=n@x", "-C"])
                    .arg(&root)
                    .args(args)
                    .status()
                    .unwrap();
                assert!(status.success());
            };
            git(&["init", "-q"]);
            git(&["add", "-A"]);
            git(&["commit", "-qm", "init", "--allow-empty"]);
            Self(root)
        }
    }
    impl Drop for Repository {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn changes_list_commits_since_a_base_and_uncommitted_content() {
        let repository = Repository::new(&["a.txt", "b.txt"]);
        let root = &repository.0;
        let base = changes(root, None);
        assert!(base.committed.is_none() && base.uncommitted.is_empty() && base.complete);
        let head = base.head.as_str().unwrap().to_string();
        fs::write(root.join("a.txt"), "changed\n").unwrap();
        fs::create_dir_all(root.join(".craft/state")).unwrap();
        fs::write(root.join(".craft/state/private.json"), "{}").unwrap();
        let dirty = changes(root, Some(&head));
        assert_eq!(dirty.committed, Some(vec![]));
        assert_eq!(dirty.uncommitted.len(), 1);
        assert_eq!(dirty.uncommitted[0].0, "a.txt");
        let git = |args: &[&str]| {
            Command::new("git")
                .args(["-c", "user.name=n", "-c", "user.email=n@x", "-C"])
                .arg(root)
                .args(args)
                .status()
                .unwrap()
        };
        assert!(git(&["mv", "b.txt", "c.txt"]).success());
        assert!(git(&["commit", "-qam", "move"]).success());
        let moved = changes(root, Some(&head));
        assert_eq!(
            moved.committed,
            Some(vec!["a.txt".to_string(), "b.txt".into(), "c.txt".into()])
        );
        assert!(moved.uncommitted.is_empty());
        fs::write(root.join("a.txt"), "again\n").unwrap();
        let again = changes(root, Some(&head));
        assert_ne!(again.uncommitted[0].1, dirty.uncommitted[0].1);
        assert!(changes(root, Some("not-a-revision")).committed.is_none());
    }

    #[test]
    fn a_special_file_in_place_of_a_tracked_file_is_incomplete_without_blocking() {
        let repository = Repository::new(&["a.txt", "f"]);
        fs::remove_file(repository.0.join("f")).unwrap();
        assert!(
            Command::new("mkfifo")
                .arg(repository.0.join("f"))
                .status()
                .unwrap()
                .success()
        );
        let started = Instant::now();
        let snapshot = state(&repository.0);
        assert!(started.elapsed() < Duration::from_secs(5));
        assert_eq!(snapshot["fingerprints"]["complete"], false);
        assert_eq!(
            snapshot["fingerprints"]["issues"],
            json!(["Working-tree inspection failed: f"])
        );
        assert_eq!(snapshot["fingerprints"]["files"], 2);
    }

    // The JS snapshot decoded such a name lossily, hashed it as missing and reported a
    // complete snapshot, so content changes under that name went undetected.
    #[cfg(target_os = "linux")]
    #[test]
    fn a_name_that_is_not_utf8_makes_the_snapshot_incomplete() {
        use std::os::unix::ffi::OsStrExt;
        let repository = Repository::new(&["a.txt"]);
        let name = std::ffi::OsStr::from_bytes(b"bad-\xff.txt");
        fs::write(repository.0.join(name), "raw").unwrap();
        let snapshot = state(&repository.0);
        assert_eq!(snapshot["fingerprints"]["complete"], false);
        let issues = snapshot["fingerprints"]["issues"].as_array().unwrap();
        assert!(
            issues[0]
                .as_str()
                .unwrap()
                .starts_with("Working-tree inspection failed: bad-")
        );
    }

    #[test]
    fn a_scope_spelled_differently_from_its_directory_is_refused() {
        let repository = Repository::new(&["a.txt"]);
        let sub = Repository::new(&[]);
        fs::rename(&sub.0, repository.0.join("sub")).unwrap();
        let roots = |scope: &str| roots(&repository.0, &[scope.to_string()]);
        assert_eq!(roots("sub").unwrap()[0].1, repository.0.join("sub"));
        // Case-insensitive volumes resolve `Sub`; it is an alias of the saved selection.
        if fs::symlink_metadata(repository.0.join("Sub")).is_ok() {
            assert!(
                roots("Sub")
                    .unwrap_err()
                    .to_string()
                    .contains("without aliases")
            );
        }
        std::os::unix::fs::symlink("sub", repository.0.join("link")).unwrap();
        assert!(roots("link").unwrap_err().to_string().contains("symlinks"));
        assert_eq!(roots("missing").unwrap_err().code(), "ENOENT");
    }
}
