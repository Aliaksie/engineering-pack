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
    /// The state folder.
    fn base(&self) -> PathBuf {
        self.0.join(".craft").join("state")
    }
    /// Where an earlier layout kept its records.
    fn worktree(&self) -> PathBuf {
        let id = &hash(self.0.to_str().unwrap().as_bytes())[..24];
        self.base().join(id)
    }
    /// A session holding a checkpoint.
    fn session(&self, session: &str) -> Directory {
        let record = file(&self.0, "checkpoint.json", true, Some(session)).unwrap();
        fs::write(&record, "{}\n").unwrap();
        self.find(&hash(session.as_bytes())[..24])
    }
    fn find(&self, id: &str) -> Directory {
        inventory(&self.0)
            .unwrap()
            .sessions
            .into_iter()
            .find(|found| found.id == id)
            .unwrap()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn inventory_lists_sessions_and_workspaces_but_not_earlier_layouts_without_creating_state() {
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
    let sessions = fixture.base().join("sessions");
    fs::create_dir(sessions.join(".sweep-89abcdef0123456789abcdef")).unwrap();
    fs::create_dir(sessions.join("not-a-session")).unwrap();
    fs::create_dir_all(fixture.base().join("workspaces").join("e".repeat(64))).unwrap();
    fs::write(fixture.base().join("workspaces/guard.json"), "{}").unwrap();
    let worktree = fixture.worktree();
    let earlier = worktree.join("0123456789abcdef01234567");
    fs::create_dir_all(&earlier).unwrap();
    fs::write(earlier.join("0123456789abcdef01234567.json"), "{}").unwrap();
    fs::create_dir_all(worktree.join("shared/sessions").join("f".repeat(64))).unwrap();
    fs::create_dir_all(worktree.join("sessions").join("1".repeat(24))).unwrap();

    let found = inventory(&fixture.0).unwrap();
    let mut listed: Vec<_> = found
        .sessions
        .iter()
        .map(|s| (s.id.as_str(), s.staged))
        .collect();
    listed.sort();
    assert_eq!(
        listed,
        [
            ("89abcdef0123456789abcdef", true),
            (current.id.as_str(), false),
        ]
    );
    let workspaces: Vec<_> = found.workspaces.iter().map(|(name, _)| name).collect();
    assert_eq!(workspaces, [&"e".repeat(64)]);
    assert!(
        worktree.join("sessions").join("1".repeat(24)).exists(),
        "an earlier layout is not moved into place"
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
    let resumed = fixture.find(&again.id);
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
    let recreated = inspect(&found.path, &found.id, false).unwrap();
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

#[test]
fn an_interrupted_removal_keeps_the_manifest() {
    let fixture = Fixture::new();
    let found = fixture.session("a");
    let task = file(&fixture.0, "task.json", true, Some("a")).unwrap();
    fs::write(&task, "{}\n").unwrap();
    let found = fixture.find(&found.id);
    let staged = stage(&found).unwrap();
    // A record that can no longer be removed as a file stops the removal.
    fs::remove_file(staged.join("task.json")).unwrap();
    fs::create_dir(staged.join("task.json")).unwrap();
    assert!(purge(&found, &staged).is_err());
    assert_eq!(
        fs::read_dir(&staged)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().into_string().unwrap())
            .collect::<std::collections::BTreeSet<_>>(),
        ["manifest.json".to_string(), "task.json".to_string()].into()
    );
}

#[cfg(unix)]
#[test]
fn symlinked_session_directories_are_refused() {
    let fixture = Fixture::new();
    let found = fixture.session("a");
    let link = fixture.base().join("sessions").join("0".repeat(24));
    std::os::unix::fs::symlink(&found.path, &link).unwrap();
    assert!(inventory(&fixture.0).is_err());
    fs::remove_file(&link).unwrap();
    let inner = found.path.join("routing-0123456789abcdef01234567.json");
    std::os::unix::fs::symlink(found.path.join("checkpoint.json"), &inner).unwrap();
    let listed = fixture.find(&found.id);
    let entry = listed
        .entries
        .iter()
        .find(|e| e.name.starts_with("routing-"));
    assert!(!entry.unwrap().file);
}

/// A directory junction, the reparse point Windows creates without extra privileges.
#[cfg(windows)]
fn junction(link: &Path, target: &Path) {
    let status = std::process::Command::new("cmd")
        .args(["/C", "mklink", "/J"])
        .arg(link)
        .arg(target)
        .stdout(std::process::Stdio::null())
        .status()
        .unwrap();
    assert!(status.success());
}

#[cfg(windows)]
#[test]
fn junctioned_session_and_state_directories_are_refused() {
    let fixture = Fixture::new();
    let found = fixture.session("a");
    let link = fixture.base().join("sessions").join("0".repeat(24));
    junction(&link, &found.path);
    assert!(inventory(&fixture.0).is_err());
    fs::remove_dir(&link).unwrap();
    assert!(found.path.join("checkpoint.json").exists());

    let elsewhere = Fixture::new();
    let state = elsewhere.0.join(".craft");
    fs::create_dir(&state).unwrap();
    junction(
        &state.join("state"),
        &fixture.0.join(".craft").join("state"),
    );
    assert!(inventory(&elsewhere.0).is_err());
    assert!(file(&elsewhere.0, "checkpoint.json", true, Some("a")).is_err());
}
