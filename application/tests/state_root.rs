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
fn a_command_run_from_a_subfolder_works_on_the_checkout() {
    let repo = Repo::new("state-root-cli", &[("src/deep/a.txt", "a\n")]);
    prompt(&repo.root, "S-1");
    let deep = repo.root.join("src/deep");

    let preview: Value = serde_json::from_str(&craft(&deep, &["state-clean"], None)).unwrap();
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

#[test]
fn a_plan_made_in_a_linked_worktree_is_read_from_every_checkout_of_the_repository() {
    let repo = Repo::new("state-root-worktree", &[("a.txt", "a\n")]);
    let outside = repo.root.with_extension("inputs");
    let worktree = outside.join("wt");
    let other = outside.join("other");
    for (path, branch) in [(&worktree, "work"), (&other, "more")] {
        repo.git(&[
            "worktree",
            "add",
            "-q",
            "-b",
            branch,
            path.to_str().unwrap(),
        ]);
    }
    let tasks = repo.input(
        "tasks.json",
        &json!({"tasks": [{"id": "a", "goal": "Do it"}]}),
    );
    let made = craft(
        &worktree,
        &[
            "plan-create",
            "--plan",
            "shared",
            "--goal",
            "Ship",
            "--data",
            &tasks,
            "--json",
        ],
        None,
    );
    let made: Value = serde_json::from_str(&made).unwrap();
    // Compared as paths: Windows separators differ from the spelling here.
    let view = repo.root.join(".craft/state/plans/shared/plan.md");
    assert_eq!(Path::new(made["view"].as_str().unwrap()), view);
    for checkout in [&repo.root, &other] {
        let listed: Value =
            serde_json::from_str(&craft(checkout, &["plan", "--json"], None)).unwrap();
        assert_eq!(listed["plans"][0]["id"], "shared", "{}", checkout.display());
    }
    // Sessions and every other record stay with the worktree they ran in.
    prompt(&worktree, "S-wt");
    assert!(has_session(&worktree, "S-wt"));
    assert!(!has_session(&repo.root, "S-wt"));
    assert!(!worktree.join(".craft/state/plans").exists());
}

/// A repository with a linked worktree outside it, which shares its plans.
fn linked(name: &str) -> (Repo, PathBuf) {
    let repo = Repo::new(name, &[("a.txt", "a\n")]);
    let worktree = repo.root.with_extension("inputs").join("wt");
    repo.git(&[
        "worktree",
        "add",
        "-q",
        "-b",
        "work",
        worktree.to_str().unwrap(),
    ]);
    (repo, worktree)
}

/// A plan `id` with the tasks `ids`, made from `cwd`.
fn plan(repo: &Repo, cwd: &Path, id: &str, ids: &[&str]) {
    let tasks: Vec<Value> = ids
        .iter()
        .map(|t| json!({"id": t, "goal": "Do it"}))
        .collect();
    let data = repo.input(&format!("{id}.json"), &json!({"tasks": tasks}));
    craft(
        cwd,
        &[
            "plan-create",
            "--plan",
            id,
            "--goal",
            "Ship",
            "--data",
            &data,
        ],
        None,
    );
}

/// Claim and complete `task` of `id` in session `session`; returns the step's receipt.
fn finish(cwd: &Path, session: &str, id: &str, task: &str) -> String {
    craft(
        cwd,
        &[
            "task-start",
            "--session",
            session,
            "--plan",
            id,
            "--task",
            task,
        ],
        None,
    );
    craft(
        cwd,
        &[
            "task-step",
            "--session",
            session,
            "--stage",
            "verify",
            "--target",
            &format!("task:{task}"),
            "--approach",
            "direct",
            "--evidence",
            "e1",
            "--outcome",
            "passed",
            "--result",
            "Done",
            "--next",
            "Report",
            "--complete",
            "--bundle",
            &support::bundle(),
        ],
        None,
    )
}

fn status(cwd: &Path, id: &str) -> Value {
    let shown: Value =
        serde_json::from_str(&craft(cwd, &["plan", "--plan", id, "--json"], None)).unwrap();
    shown["status"].clone()
}

#[test]
fn completing_the_last_task_closes_the_plan_from_any_checkout() {
    let (repo, worktree) = linked("state-root-close");
    for (cwd, id) in [(&repo.root, "main"), (&worktree, "linked")] {
        plan(&repo, cwd, id, &["a", "b"]);
        let first = finish(cwd, "S-close", id, "a");
        assert!(!first.contains("tasks)"), "{first}");
        assert_eq!(status(cwd, id), "open");
        let last = finish(cwd, "S-close", id, "b");
        assert!(
            last.contains(&format!("Plan {id}: completed (2 of 2 tasks)")),
            "{last}"
        );
        assert!(
            last.contains("closing the ticket or opening a pull request stays their call"),
            "{last}"
        );
        // One plan store: the other checkout reads it closed.
        assert_eq!(status(&repo.root, id), "completed");
        assert_eq!(status(&worktree, id), "completed");
    }
    let _ = fs::remove_dir_all(repo.root.with_extension("inputs"));
}

#[test]
fn a_session_start_closes_plans_left_open_with_their_work_done() {
    let (repo, worktree) = linked("state-root-settle");
    plan(&repo, &repo.root, "finished", &["a"]);
    finish(&repo.root, "S-old", "finished", "a");
    // Reopened, as an earlier build left a plan whose work was done.
    let reopen = repo.input("reopen.json", &json!({"status": "open"}));
    let shown: Value = serde_json::from_str(&craft(
        &repo.root,
        &["plan", "--plan", "finished", "--json"],
        None,
    ))
    .unwrap();
    let revision = shown["revision"].to_string();
    craft(
        &repo.root,
        &[
            "plan-update",
            "--plan",
            "finished",
            "--expected-revision",
            &revision,
            "--data",
            &reopen,
        ],
        None,
    );
    assert_eq!(status(&repo.root, "finished"), "open");
    plan(&repo, &repo.root, "left", &["a", "b"]);
    finish(&repo.root, "S-old", "left", "a");
    for (cwd, session) in [(&worktree, "S-wt"), (&repo.root, "S-main")] {
        let payload = json!({"session_id": session, "cwd": cwd, "source": "startup"});
        let args = [
            "hook",
            "claude",
            "session-start",
            "--package",
            "craft-delivery",
            "--handler",
            "deliver",
        ];
        let started = craft(cwd, &args, Some(&payload));
        assert!(!started.contains("finished"), "{started}");
        assert!(started.contains(r#"\"plan\":\"left\""#), "{started}");
    }
    assert_eq!(status(&repo.root, "finished"), "completed");
    assert_eq!(status(&repo.root, "left"), "open");
    let _ = fs::remove_dir_all(repo.root.with_extension("inputs"));
}

/// `plan-update` of `id` at its current revision with `changes`; returns its receipt.
fn update(repo: &Repo, id: &str, changes: &Value) -> String {
    let shown: Value =
        serde_json::from_str(&craft(&repo.root, &["plan", "--plan", id, "--json"], None)).unwrap();
    let data = repo.input(&format!("{id}-{}.json", shown["revision"]), changes);
    let revision = shown["revision"].to_string();
    craft(
        &repo.root,
        &[
            "plan-update",
            "--plan",
            id,
            "--expected-revision",
            &revision,
            "--data",
            &data,
        ],
        None,
    )
}

#[test]
fn a_plan_update_closes_a_finished_plan_and_new_work_reopens_it() {
    let repo = Repo::new("state-root-update", &[("a.txt", "a\n")]);
    plan(&repo, &repo.root, "reviewed", &["a", "b"]);
    finish(&repo.root, "S-up", "reviewed", "a");
    let dropped = update(
        &repo,
        "reviewed",
        &json!({"tasks": [{"id": "b", "state": "abandoned"}]}),
    );
    assert!(
        dropped.contains("Plan reviewed: completed (1 of 1 tasks)"),
        "{dropped}"
    );
    assert_eq!(status(&repo.root, "reviewed"), "completed");
    let added = update(
        &repo,
        "reviewed",
        &json!({"tasks": [{"id": "c", "goal": "Fix the review finding"}]}),
    );
    assert!(!added.contains("tasks) —"), "{added}");
    assert_eq!(status(&repo.root, "reviewed"), "open");
    // The reopened plan has work left, so a session start keeps it open.
    let payload = json!({"session_id": "S-next", "cwd": repo.root, "source": "startup"});
    let args = [
        "hook",
        "claude",
        "session-start",
        "--package",
        "craft-delivery",
        "--handler",
        "deliver",
    ];
    let started = craft(&repo.root, &args, Some(&payload));
    assert!(started.contains(r#"\"ready\":[\"c\"]"#), "{started}");
    let last = finish(&repo.root, "S-up", "reviewed", "c");
    assert!(
        last.contains("Plan reviewed: completed (2 of 2 tasks)"),
        "{last}"
    );
    let _ = fs::remove_dir_all(repo.root.with_extension("inputs"));
}

#[test]
fn a_plan_that_cannot_close_at_session_start_stays_listed_and_is_reported() {
    let repo = Repo::new("state-root-busy", &[("a.txt", "a\n")]);
    plan(&repo, &repo.root, "finished", &["a"]);
    finish(&repo.root, "S-old", "finished", "a");
    let shown: Value = serde_json::from_str(&craft(
        &repo.root,
        &["plan", "--plan", "finished", "--json"],
        None,
    ))
    .unwrap();
    let reopen = repo.input("reopen.json", &json!({"status": "open"}));
    let revision = shown["revision"].to_string();
    craft(
        &repo.root,
        &[
            "plan-update",
            "--plan",
            "finished",
            "--expected-revision",
            &revision,
            "--data",
            &reopen,
        ],
        None,
    );
    // Another writer holds the plans lock for the whole session start.
    let index = state::plans(&repo.root, None, false)
        .unwrap()
        .join("index.json");
    let held = state::lock(&index).unwrap().expect("plans lock");
    let payload = json!({"session_id": "S-busy", "cwd": repo.root, "source": "startup"});
    let args = [
        "hook",
        "claude",
        "session-start",
        "--package",
        "craft-delivery",
        "--handler",
        "deliver",
    ];
    let started = craft(&repo.root, &args, Some(&payload));
    drop(held);
    assert!(started.contains(r#"\"plan\":\"finished\""#), "{started}");
    assert_eq!(status(&repo.root, "finished"), "open");
    let log = state::file(&repo.root, "telemetry/events.jsonl", false, None).unwrap();
    let events = fs::read_to_string(&log).unwrap_or_default();
    assert!(events.contains("STATE_UNAVAILABLE"), "{events}");
    let _ = fs::remove_dir_all(repo.root.with_extension("inputs"));
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
