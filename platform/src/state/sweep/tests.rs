use super::*;
use crate::state::{canonical_root, file};
use craft_engine::hash;
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT: AtomicU64 = AtomicU64::new(0);

struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "craft-sweep-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        Self(canonical_root(&path).unwrap())
    }
    fn worktree(&self) -> PathBuf {
        let id = &hash(self.0.to_str().unwrap().as_bytes())[..24];
        self.0.join(".craft/state").join(id)
    }
    /// A current-layout session holding a checkpoint.
    fn session(&self, session: &str) -> Directory {
        let record = file(&self.0, "checkpoint.json", true, Some(session)).unwrap();
        fs::write(&record, "{}\n").unwrap();
        self.find(&hash(session.as_bytes())[..24], Layout::Current)
    }
    fn find(&self, id: &str, layout: Layout) -> Directory {
        inventory(&self.0)
            .unwrap()
            .sessions
            .into_iter()
            .find(|found| found.id == id && found.layout == layout)
            .unwrap()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn inventory_lists_both_layouts_workspaces_and_shared_leftovers_without_creating_state() {
    let fixture = Fixture::new();
    let empty = inventory(&fixture.0).unwrap();
    assert!(empty.sessions.is_empty() && empty.complete && empty.workspaces_complete);
    assert!(!fixture.0.join(".craft").exists());

    let current = fixture.session("a");
    assert_eq!(
        current
            .entries
            .iter()
            .map(|e| e.name.as_str())
            .collect::<Vec<_>>(),
        ["checkpoint.json", "manifest.json"]
    );
    assert!(matches!(current.described, Described::Workspace(_)));
    let worktree = fixture.worktree();
    let previous = worktree.join("0123456789abcdef01234567");
    fs::create_dir(&previous).unwrap();
    fs::write(previous.join("0123456789abcdef01234567.json"), "{}").unwrap();
    fs::create_dir(worktree.join(".sweep-89abcdef0123456789abcdef")).unwrap();
    fs::create_dir(worktree.join("not-a-session")).unwrap();
    fs::create_dir(worktree.join("shared")).unwrap();
    fs::write(worktree.join("shared/telemetry.jsonl"), "12345").unwrap();
    fs::create_dir_all(worktree.join("shared/sessions").join("f".repeat(64))).unwrap();
    fs::create_dir_all(worktree.join("shared/workspaces").join("e".repeat(64))).unwrap();
    fs::write(worktree.join("shared/workspaces/guard.json"), "{}").unwrap();

    let found = inventory(&fixture.0).unwrap();
    let mut sessions: Vec<_> = found
        .sessions
        .iter()
        .map(|s| (s.id.as_str(), s.layout.name(), s.staged))
        .collect();
    sessions.sort();
    assert_eq!(
        sessions,
        [
            ("0123456789abcdef01234567", "previous", false),
            ("89abcdef0123456789abcdef", "previous", true),
            (current.id.as_str(), "current", false),
        ]
    );
    let previous = found
        .sessions
        .iter()
        .find(|s| !s.staged && s.layout == Layout::Previous);
    assert_eq!(previous.unwrap().described, Described::Absent);
    let mut workspaces: Vec<_> = found
        .workspaces
        .iter()
        .map(|(name, _)| name.clone())
        .collect();
    workspaces.sort();
    assert_eq!(workspaces, ["e".repeat(64), "f".repeat(64)]);
    let leftovers: Vec<_> = found
        .leftovers
        .iter()
        .map(|l| (l.name.as_str(), l.directory, l.bytes))
        .collect();
    assert_eq!(
        leftovers,
        [
            ("sessions", true, Some(0)),
            ("telemetry.jsonl", false, Some(5))
        ]
    );
}

#[test]
fn an_unchanged_staged_directory_is_removed() {
    let fixture = Fixture::new();
    let found = fixture.session("a");
    let staged = stage(&found).unwrap();
    assert!(!found.path.exists() && staged.ends_with(format!(".sweep-{}", found.id)));
    assert!(unchanged(&found, &staged).unwrap());
    purge(&found, &staged).unwrap();
    assert!(!staged.exists() && !found.path.exists());
    // A staged directory is finished where it is.
    let again = fixture.session("b");
    let staged = stage(&again).unwrap();
    let resumed = fixture.find(&again.id, Layout::Current);
    assert!(resumed.staged);
    assert_eq!(stage(&resumed).unwrap(), staged);
}

#[test]
fn a_write_after_inventory_restores_the_directory() {
    let fixture = Fixture::new();
    let found = fixture.session("a");
    let staged = stage(&found).unwrap();
    fs::write(staged.join("checkpoint.json"), "{\"changed\":true}\n").unwrap();
    assert!(!unchanged(&found, &staged).unwrap());
    restore(&found, &staged).unwrap();
    assert_eq!(
        fs::read_to_string(found.path.join("checkpoint.json")).unwrap(),
        "{\"changed\":true}\n"
    );
}

#[test]
fn a_recreated_session_or_new_file_keeps_the_staged_directory() {
    let fixture = Fixture::new();
    let found = fixture.session("a");
    let staged = stage(&found).unwrap();
    fs::create_dir(&found.path).unwrap();
    assert!(restore(&found, &staged).is_err());
    assert!(staged.exists() && found.path.exists());
    let recreated = inspect(&found.path, &found.id, Layout::Current, false).unwrap();
    assert!(
        stage(&recreated).is_err(),
        "unfinished cleanup is not overwritten"
    );

    fs::write(staged.join("task.json"), "{}").unwrap();
    assert!(purge(&found, &staged).is_err());
    assert_eq!(
        fs::read_dir(&staged).unwrap().count(),
        1,
        "only the inspected files were removed"
    );
}

#[cfg(unix)]
#[test]
fn symlinked_session_directories_are_refused() {
    let fixture = Fixture::new();
    let found = fixture.session("a");
    let link = fixture.worktree().join("sessions").join("0".repeat(24));
    std::os::unix::fs::symlink(&found.path, &link).unwrap();
    assert!(inventory(&fixture.0).is_err());
    fs::remove_file(&link).unwrap();
    let inner = found.path.join("routing-0123456789abcdef01234567.json");
    std::os::unix::fs::symlink(found.path.join("checkpoint.json"), &inner).unwrap();
    let listed = fixture.find(&found.id, Layout::Current);
    let entry = listed
        .entries
        .iter()
        .find(|e| e.name.starts_with("routing-"));
    assert!(!entry.unwrap().file);
}
