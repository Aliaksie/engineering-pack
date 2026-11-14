//! One state directory per checkout: whichever subfolder or spelling a hook or command
//! starts from, the checkout's records land in its own `.craft/state`, never below it, and
//! `state-clean` reaches every record the runtime writes there.
mod support;

use craft_platform::state;
use serde_json::{Value, json};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, SystemTime};
use support::Repo;

/// `craft` started in `cwd` with no user or organization layer of the machine.
fn craft(cwd: &Path, args: &[&str], stdin: Option<&Value>) -> String {
    let home = cwd.join(".home-unused");
    let mut child = Command::new(env!("CARGO_BIN_EXE_craft"))
        .args(args)
        .current_dir(cwd)
        .env("HOME", &home)
        .env("USERPROFILE", &home)
        .env("CRAFT_ORGANIZATION_CONFIG", home.join("none.json"))
        .env("CRAFT_EVIDENCE_WARM", "0")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut input = child.stdin.take().unwrap();
    if let Some(payload) = stdin {
        input.write_all(payload.to_string().as_bytes()).unwrap();
    }
    drop(input);
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "{args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap()
}

/// A Claude prompt hook in `cwd`: it keeps the session's intent record.
fn prompt(cwd: &Path, session: &str) {
    let payload =
        json!({"session_id": session, "cwd": cwd, "prompt": "fix the failing login check"});
    let args = [
        "hook",
        "claude",
        "before-prompt",
        "--package",
        "craft-delivery",
        "--handler",
        "deliver",
    ];
    craft(cwd, &args, Some(&payload));
}

/// Every `.craft` directory below `root`, outside Git's own.
fn state_folders(root: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    let mut pending = vec![root.to_path_buf()];
    while let Some(directory) = pending.pop() {
        for entry in fs::read_dir(&directory).unwrap().flatten() {
            let path = entry.path();
            if !entry.file_type().unwrap().is_dir() || entry.file_name() == ".git" {
                continue;
            }
            if entry.file_name() == ".craft" {
                found.push(path);
            } else {
                pending.push(path);
            }
        }
    }
    found.sort();
    found
}

fn has_session(root: &Path, session: &str) -> bool {
    let place = state::location(root, Some(session)).unwrap();
    place.directory.join("intent.json").is_file()
}

#[test]
fn a_hook_from_a_subfolder_or_another_spelling_keeps_the_checkouts_state() {
    let repo = Repo::new(
        "state-root",
        &[("src/deep/a.txt", "a\n"), ("app/app/b.txt", "b\n")],
    );
    let worktree = repo.root.with_extension("worktree");
    repo.git(&[
        "worktree",
        "add",
        "-q",
        worktree.to_str().unwrap(),
        "-b",
        "second",
    ]);

    prompt(&repo.root, "top");
    prompt(&repo.root.join("src/deep"), "deep");
    prompt(&repo.root.join("app/app"), "nested");
    prompt(&worktree.join("src"), "worktree");
    #[cfg(unix)]
    {
        let link = repo.root.with_extension("link");
        std::os::unix::fs::symlink(&repo.root, &link).unwrap();
        prompt(&link.join("src"), "link");
        fs::remove_file(&link).unwrap();
        assert!(has_session(&repo.root, "link"));
    }

    for session in ["top", "deep", "nested"] {
        assert!(has_session(&repo.root, session), "{session}");
    }
    assert_eq!(state_folders(&repo.root), [repo.root.join(".craft")]);
    // A second worktree is a checkout of its own: its session stays there.
    assert!(has_session(&worktree, "worktree"));
    assert!(!has_session(&repo.root, "worktree"));
    assert_eq!(state_folders(&worktree), [worktree.join(".craft")]);
    let _ = fs::remove_dir_all(&worktree);
}

#[test]
fn a_command_given_a_subfolder_works_on_the_checkout() {
    let repo = Repo::new("state-root-cli", &[("src/deep/a.txt", "a\n")]);
    prompt(&repo.root, "S-1");
    let deep = repo.root.join("src/deep");

    let preview: Value =
        serde_json::from_str(&craft(&deep, &["state-clean", "--root", "."], None)).unwrap();
    let id = state::location(&repo.root, Some("S-1")).unwrap().session_id;
    assert_eq!(preview["sessions"][0]["session"], id.as_str());

    craft(
        &deep,
        &["project-evidence", "refresh", "--enable", "--json"],
        None,
    );
    assert!(repo.root.join(".craft/state/intelligence").is_dir());
    assert_eq!(state_folders(&repo.root), [repo.root.join(".craft")]);
}

/// Date a file or directory `days` back.
fn age(path: &Path, days: u64) {
    let when = SystemTime::now() - Duration::from_secs(days * 86_400);
    let mut options = fs::OpenOptions::new();
    #[cfg(unix)]
    options.read(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        // FILE_WRITE_ATTRIBUTES, and FILE_FLAG_BACKUP_SEMANTICS to open a directory.
        options.access_mode(0x0100).custom_flags(0x0200_0000);
    }
    options.open(path).unwrap().set_modified(when).unwrap();
}

#[test]
fn state_clean_removes_an_idle_session_holding_any_record_the_runtime_writes() {
    let repo = Repo::new("state-root-clean", &[("a.txt", "a\n")]);
    let records = [
        "intent.json",
        "restore.json",
        "test-runs.json",
        "check-runs.json",
        "stop-review.json",
    ];
    let mut directory = PathBuf::new();
    for record in records {
        let file = state::file(&repo.root, record, true, Some("idle")).unwrap();
        fs::write(&file, "{}\n").unwrap();
        directory = file.parent().unwrap().to_path_buf();
    }
    for entry in fs::read_dir(&directory).unwrap() {
        age(&entry.unwrap().path(), 15);
    }
    age(&directory, 15);

    let applied: Value = serde_json::from_str(&craft(
        &repo.root,
        &["state-clean", "--root", ".", "--apply"],
        None,
    ))
    .unwrap();
    assert_eq!(applied["sessions"][0]["action"], "removed", "{applied}");
    assert!(!directory.exists());
}
