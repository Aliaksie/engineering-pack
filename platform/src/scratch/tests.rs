use super::*;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
static NEXT: AtomicU64 = AtomicU64::new(0);

struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "craft-scratch-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
    fn path(&self, name: &str) -> PathBuf {
        self.0.join(name)
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn message(error: io::Error) -> String {
    error.to_string()
}

#[test]
fn directories_are_created_privately_and_never_followed() {
    let fixture = Fixture::new();
    let dir = fixture.path("d");
    assert!(!directory(&dir, false).unwrap());
    assert!(directory(&dir, true).unwrap());
    assert!(
        directory(&dir, true).unwrap(),
        "an existing directory, as after a concurrent create, is accepted"
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            fs::metadata(&dir).unwrap().permissions().mode() & 0o777,
            0o700
        );
    }
    fs::write(fixture.path("file"), "unowned").unwrap();
    for create in [false, true] {
        let error = directory(&fixture.path("file"), create).unwrap_err();
        assert_eq!(message(error), "workspace directory must not be a symlink");
    }
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(&dir, fixture.path("link")).unwrap();
        assert!(directory(&fixture.path("link"), false).is_err());
    }
    let missing_parent = fixture.path("absent/child");
    assert_eq!(
        directory(&missing_parent, true).unwrap_err().kind(),
        ErrorKind::NotFound
    );
}

#[test]
fn usage_counts_regular_files_and_refuses_what_it_cannot_inspect() {
    let fixture = Fixture::new();
    let root = fixture.path("scratch");
    assert_eq!(usage(&root).unwrap(), Usage::default());
    fs::create_dir_all(root.join("a/b")).unwrap();
    fs::write(root.join("one"), "123").unwrap();
    fs::write(root.join("a/b/two"), "45").unwrap();
    assert_eq!(usage(&root).unwrap(), Usage { bytes: 5, files: 2 });
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(fixture.path("outside"), root.join("a/escape")).unwrap();
        let error = usage(&root).unwrap_err();
        assert_eq!(
            message(error),
            "workspace contains a symlink; cleanup refused"
        );
        fs::remove_file(root.join("a/escape")).unwrap();
    }
    let mut deep = root.clone();
    for _ in 0..=INSPECTION_DEPTH {
        deep.push("d");
    }
    fs::create_dir_all(&deep).unwrap();
    assert_eq!(
        message(usage(&root).unwrap_err()),
        "workspace inspection limit reached"
    );
}

#[test]
fn usage_bounds_the_number_of_entries() {
    let fixture = Fixture::new();
    let root = fixture.path("wide");
    fs::create_dir(&root).unwrap();
    for index in 0..INSPECTION_ENTRIES - 1 {
        fs::write(root.join(index.to_string()), "").unwrap();
    }
    assert_eq!(usage(&root).unwrap().files as usize, INSPECTION_ENTRIES - 1);
    fs::write(root.join("one-more"), "").unwrap();
    assert_eq!(
        message(usage(&root).unwrap_err()),
        "workspace inspection limit reached"
    );
}

#[test]
fn removal_inspects_first_and_keeps_targets_outside() {
    let fixture = Fixture::new();
    let tree = fixture.path("tree");
    fs::create_dir_all(tree.join("x")).unwrap();
    fs::write(tree.join("x/f"), "f").unwrap();
    remove(&tree).unwrap();
    assert!(!tree.exists());
    remove(&tree).unwrap();
    #[cfg(unix)]
    {
        fs::create_dir_all(&tree).unwrap();
        fs::create_dir(fixture.path("outside")).unwrap();
        fs::write(fixture.path("outside/keep"), "keep").unwrap();
        std::os::unix::fs::symlink(fixture.path("outside"), tree.join("escape")).unwrap();
        assert!(remove(&tree).is_err());
        assert!(tree.join("escape").exists());
        assert_eq!(
            fs::read_to_string(fixture.path("outside/keep")).unwrap(),
            "keep"
        );
    }
}

#[test]
fn names_are_bounded() {
    let fixture = Fixture::new();
    for name in ["a", "b", "c"] {
        fs::write(fixture.path(name), "").unwrap();
    }
    assert_eq!(names(&fixture.0, 3).unwrap().len(), 3);
    assert_eq!(
        message(names(&fixture.0, 2).unwrap_err()),
        "workspace session inspection limit reached"
    );
}

#[test]
fn created_files_never_overwrite() {
    let fixture = Fixture::new();
    create(&fixture.path("f"), b"first").unwrap();
    assert_eq!(
        create(&fixture.path("f"), b"second").unwrap_err().kind(),
        ErrorKind::AlreadyExists
    );
    assert_eq!(fs::read(fixture.path("f")).unwrap(), b"first");
}

#[test]
fn copies_only_admitted_single_regular_files_into_new_targets() {
    let fixture = Fixture::new();
    let (source, target) = (fixture.path("source"), fixture.path("target"));
    fs::write(&source, "evidence").unwrap();
    let refuse = |_| Err(io::Error::other("unsafe or oversized evidence"));
    assert!(copy(&source, &target, refuse).is_err());
    assert!(!target.exists(), "nothing is written before admission");
    let mut seen = None;
    assert_eq!(
        copy(&source, &target, |size| {
            seen = Some(size);
            Ok(())
        })
        .unwrap(),
        8
    );
    assert_eq!(seen, Some(8));
    assert_eq!(fs::read_to_string(&target).unwrap(), "evidence");
    assert_eq!(
        message(copy(&source, &target, |_| Ok(())).unwrap_err()),
        "evidence already exists"
    );
    assert_eq!(
        copy(&fixture.path("absent"), &fixture.path("t2"), |_| Ok(()))
            .unwrap_err()
            .kind(),
        ErrorKind::NotFound
    );
    #[cfg(unix)]
    {
        fs::hard_link(&source, fixture.path("alias")).unwrap();
        assert_eq!(
            message(copy(&source, &fixture.path("t3"), |_| Ok(())).unwrap_err()),
            "unsafe or oversized evidence"
        );
        std::os::unix::fs::symlink(&target, fixture.path("link")).unwrap();
        assert_eq!(
            message(copy(&fixture.path("link"), &fixture.path("t4"), |_| Ok(())).unwrap_err()),
            "unsafe or oversized evidence"
        );
    }
}

fn lock_file(fixture: &Fixture) -> PathBuf {
    fixture.path("guard.json.lock")
}

fn holder(pid: i64) -> String {
    serde_json::json!({"owner": LOCK_OWNER, "formatVersion": 1, "pid": pid}).to_string()
}

#[test]
fn the_lock_records_its_holder_and_is_released_on_drop() {
    let fixture = Fixture::new();
    let anchor = fixture.path("guard.json");
    let held = lock(&anchor).unwrap().expect("free lock");
    let recorded: Value = serde_json::from_slice(&fs::read(lock_file(&fixture)).unwrap()).unwrap();
    assert_eq!(recorded["pid"], std::process::id());
    assert!(lock(&anchor).unwrap().is_none(), "a held lock defers");
    drop(held);
    assert!(!lock_file(&fixture).exists());
}

#[test]
fn only_a_recognized_dead_holder_is_reclaimed() {
    let fixture = Fixture::new();
    let anchor = fixture.path("guard.json");
    let deferred = [
        String::new(),
        "{\"owner\":\"other\",\"formatVersion\":1,\"pid\":2147483647}".into(),
        "{\"owner\":\"craft-workspace-lock\",\"formatVersion\":2,\"pid\":2147483647}".into(),
        "{\"owner\":\"craft-workspace-lock\",\"formatVersion\":1,\"pid\":0}".into(),
        holder(i64::from(std::process::id())),
    ];
    for content in deferred {
        fs::write(lock_file(&fixture), &content).unwrap();
        assert!(lock(&anchor).unwrap().is_none(), "{content}");
        assert_eq!(fs::read_to_string(lock_file(&fixture)).unwrap(), content);
    }
    fs::write(lock_file(&fixture), "x".repeat(LOCK_BYTES + 1)).unwrap();
    assert_eq!(
        message(lock(&anchor).unwrap_err()),
        "invalid bounded state file"
    );
    fs::write(lock_file(&fixture), holder(2_147_483_647)).unwrap();
    let reclaimed = lock(&anchor).unwrap();
    assert!(reclaimed.is_some(), "a dead holder's lock is reclaimed");
    let recorded: Value = serde_json::from_slice(&fs::read(lock_file(&fixture)).unwrap()).unwrap();
    assert_eq!(recorded["pid"], std::process::id());
}

#[test]
fn unknown_holders_are_abandoned_only_on_windows_after_the_lock_age() {
    let fixture = Fixture::new();
    fs::write(lock_file(&fixture), "").unwrap();
    let metadata = fs::symlink_metadata(lock_file(&fixture)).unwrap();
    assert!(abandoned(Liveness::Dead, &metadata));
    assert!(!abandoned(Liveness::Live, &metadata));
    assert!(
        !abandoned(Liveness::Unknown, &metadata),
        "a fresh lock is never abandoned"
    );
}

#[test]
fn a_lock_released_or_still_deleting_during_reclaim_defers_instead_of_failing() {
    let error = |kind: ErrorKind| Err(io::Error::from(kind));
    for windows in [false, true] {
        assert!(gone(&io::Error::from(ErrorKind::NotFound), windows));
        assert!(reclaimed(Ok(()), windows).unwrap());
        assert!(
            reclaimed(error(ErrorKind::NotFound), windows).unwrap(),
            "a concurrent reclaimer removed it first"
        );
        assert!(reclaimed(error(ErrorKind::Other), windows).is_err());
    }
    let denied = || io::Error::from(ErrorKind::PermissionDenied);
    assert!(
        gone(&denied(), true),
        "Windows denies a lock its holder is deleting"
    );
    assert!(!gone(&denied(), false));
    assert!(!reclaimed(Err(denied()), true).unwrap());
    assert!(reclaimed(Err(denied()), false).is_err());
}
