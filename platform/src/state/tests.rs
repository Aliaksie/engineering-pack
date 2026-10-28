//! Replays `tests/fixtures/runtime/state-contracts.json`, the frozen JS state contract.
use super::*;
use std::sync::atomic::{AtomicU64, Ordering};

const CONTRACTS: &str = include_str!("../../../tests/fixtures/runtime/state-contracts.json");
const SESSION: Option<&str> = Some("session λ");
static NEXT: AtomicU64 = AtomicU64::new(0);

struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "craft-state-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        Self(canonical_root(&path).unwrap())
    }
    fn record(&self, name: &str) -> PathBuf {
        file(&self.0, name, true, SESSION).unwrap()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// A session directory of the previous layout: `<worktree>/<session>/`.
fn previous_session(root: &Path, session: &str) -> PathBuf {
    let worktree = &hash(root.to_str().unwrap().as_bytes())[..24];
    let folder = root
        .join(".craft/state")
        .join(worktree)
        .join(&hash(session.as_bytes())[..24]);
    fs::create_dir_all(&folder).unwrap();
    folder
}

fn listing(directory: &Path) -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(directory)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().into_string().unwrap())
        .collect();
    names.sort();
    names
}

#[test]
fn bytes_and_exact_utf8_bounds() {
    let contracts: Value = serde_json::from_str(CONTRACTS).unwrap();
    for case in contracts["writes"].as_array().unwrap() {
        let fixture = Fixture::new();
        let record = fixture.record("record.json");
        let bytes = case["bytes"].as_u64().unwrap() as usize;
        let pretty = case["pretty"].as_bool().unwrap();
        let expected = case["text"].as_str().unwrap();
        write(&record, &case["value"], bytes, pretty).unwrap();
        assert_eq!(
            fs::read_to_string(&record).unwrap(),
            expected,
            "{}",
            case["id"]
        );
        assert_eq!(read(&record, bytes).unwrap().as_ref(), Some(&case["value"]));
        assert_eq!(
            read(&record, bytes - 1).unwrap_err().to_string(),
            "invalid bounded state file"
        );
        assert_eq!(
            write(&record, &case["value"], bytes - 1, pretty)
                .unwrap_err()
                .to_string(),
            "state exceeds size limit"
        );
        assert_eq!(fs::read_to_string(&record).unwrap(), expected);
    }
}

#[test]
fn missing_malformed_and_oversized_records_preserve_existing_bytes() {
    let fixture = Fixture::new();
    let record = fixture.record("record.json");
    assert_eq!(read(&record, 64).unwrap(), None);
    for text in ["{broken".to_string(), " ".repeat(65)] {
        fs::write(&record, &text).unwrap();
        assert!(write(&record, &json!({"replacement": true}), 64, false).is_err());
        assert_eq!(fs::read_to_string(&record).unwrap(), text);
        assert_eq!(listing(record.parent().unwrap()), [MANIFEST, "record.json"]);
    }
}

#[test]
fn exclusive_lock_refuses_overlap_and_unresolved_stale_locks() {
    let fixture = Fixture::new();
    let record = fixture.record("record.json");
    let lock_file = record.with_file_name("record.json.lock");
    let ticket = lock(&record).unwrap().unwrap();
    assert!(lock(&record).unwrap().is_none());
    drop(ticket);
    assert!(!lock_file.exists());
    fs::write(&lock_file, "unresolved writer").unwrap();
    assert!(lock(&record).unwrap().is_none());
    assert_eq!(fs::read_to_string(&lock_file).unwrap(), "unresolved writer");
}

#[test]
fn windows_denied_locks_are_busy_while_listed_and_retried_once_released() {
    let fixture = Fixture::new();
    let record = fixture.record("record.json");
    let lock_file = record.with_file_name("record.json.lock");
    let denied = || std::io::Error::from(ErrorKind::PermissionDenied);

    // A persistent denial without a listed lock is a real error, on every attempt.
    let mut attempts = 0;
    let error = acquire(lock_file.clone(), true, |_| {
        attempts += 1;
        Err(denied())
    })
    .unwrap_err();
    assert_eq!(
        (error.to_string(), attempts),
        (denied().to_string(), LOCK_ATTEMPTS)
    );

    // The previous holder finished deleting between the denial and the listing.
    let mut attempts = 0;
    let ticket = acquire(lock_file.clone(), true, |path| {
        attempts += 1;
        if attempts == 1 {
            Err(denied())
        } else {
            File::create_new(path)
        }
    })
    .unwrap()
    .unwrap();
    assert_eq!(attempts, 2);
    drop(ticket);

    fs::write(&lock_file, "deleting writer").unwrap();
    assert!(
        acquire(lock_file.clone(), true, |_| Err(denied()))
            .unwrap()
            .is_none()
    );
    assert_eq!(fs::read_to_string(&lock_file).unwrap(), "deleting writer");
    fs::remove_file(&lock_file).unwrap();

    // Elsewhere the same denial is never reinterpreted.
    assert!(acquire(lock_file, false, |_| Err(denied())).is_err());
}

#[test]
fn failed_atomic_publication_preserves_old_bytes_and_removes_temporary_files() {
    let fixture = Fixture::new();
    let record = fixture.record("record.json");
    write(&record, &json!({"old": true}), 64, false).unwrap();
    FAIL_PUBLISH.with(|fail| fail.set(true));
    let failed = write(&record, &json!({"next": true}), 64, false);
    FAIL_PUBLISH.with(|fail| fail.set(false));
    assert_eq!(
        failed.unwrap_err().to_string(),
        "injected publication failure"
    );
    assert_eq!(fs::read_to_string(&record).unwrap(), "{\"old\":true}\n");
    assert_eq!(listing(record.parent().unwrap()), [MANIFEST, "record.json"]);
    write(&record, &json!({"next": true}), 64, false).unwrap();
    assert_eq!(read(&record, 64).unwrap(), Some(json!({"next": true})));
}

#[test]
fn placement_hashes_the_physical_root_and_session() {
    let fixture = Fixture::new();
    let place = location(&fixture.0, SESSION).unwrap();
    let root = fixture.0.to_str().unwrap();
    assert_eq!(place.worktree_id, hash(root.as_bytes())[..24]);
    assert_eq!(place.session_id, hash("session λ".as_bytes())[..24]);
    assert_eq!(location(&fixture.0, None).unwrap().session_id, "shared");
    assert!(
        place
            .directory
            .ends_with(Path::new(".craft/state/sessions").join(&place.session_id))
    );
    // 512 UTF-16 units is the bound; one astral character is two units.
    assert!(location(&fixture.0, Some(&"x".repeat(512))).is_ok());
    for session in ["".to_string(), "x".repeat(513), "😀".repeat(257)] {
        assert_eq!(
            location(&fixture.0, Some(&session))
                .unwrap_err()
                .to_string(),
            "invalid state session"
        );
    }
    for name in [
        "",
        ".",
        "..",
        "a/b",
        "a\\b",
        "λ.json",
        "record.json",
        "telemetry/",
        "telemetry/a/b",
        "sessions/x.json",
    ] {
        assert_eq!(
            file(&fixture.0, name, false, None).unwrap_err().to_string(),
            "invalid state filename"
        );
    }
    for name in ["telemetry/key.json", "a/b", "..", "λ.json"] {
        assert_eq!(
            file(&fixture.0, name, false, SESSION)
                .unwrap_err()
                .to_string(),
            "invalid state filename"
        );
    }
    assert!(
        file(&fixture.0, "telemetry/key.json", false, None)
            .unwrap()
            .ends_with(Path::new(".craft/state/telemetry/key.json"))
    );
    assert_eq!(
        shared(&fixture.0, "sessions", false)
            .unwrap_err()
            .to_string(),
        "invalid state family"
    );
    assert!(
        !fixture.0.join(".craft").exists(),
        "lookups never create state"
    );
}

#[cfg(unix)]
#[test]
fn symlinked_state_directories_are_refused() {
    let fixture = Fixture::new();
    let elsewhere = Fixture::new();
    fs::create_dir(fixture.0.join(".craft")).unwrap();
    std::os::unix::fs::symlink(&elsewhere.0, fixture.0.join(".craft").join("state")).unwrap();
    assert_eq!(
        file(&fixture.0, "telemetry/key.json", true, None)
            .unwrap_err()
            .to_string(),
        "state path must be a directory without symlinks"
    );
    assert!(listing(&elsewhere.0).is_empty());
}

fn metadata(directory: &Path) -> Vec<(PathBuf, Vec<u8>, Option<std::time::SystemTime>)> {
    let mut files = Vec::new();
    let mut pending = vec![directory.to_path_buf()];
    while let Some(next) = pending.pop() {
        for entry in fs::read_dir(&next).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                pending.push(path);
            } else {
                let modified = fs::metadata(&path).unwrap().modified().ok();
                files.push((path.clone(), fs::read(&path).unwrap(), modified));
            }
        }
    }
    files.sort();
    files
}

fn git_metadata(root: &Path) -> PathBuf {
    let git = root.join(".git");
    fs::create_dir_all(git.join("refs")).unwrap();
    fs::write(git.join("HEAD"), "ref: refs/heads/main\n").unwrap();
    git
}

#[test]
fn fresh_writes_never_create_or_change_git_metadata() {
    let fixture = Fixture::new();
    let git = git_metadata(&fixture.0);
    let before = metadata(&git);
    write(
        &fixture.record("record.json"),
        &json!({"value": 1}),
        1024,
        false,
    )
    .unwrap();
    assert_eq!(metadata(&git), before);
}

#[test]
fn records_of_earlier_stores_are_neither_read_nor_changed() {
    let fixture = Fixture::new();
    let root = &fixture.0;
    let session = SESSION.unwrap();
    let previous = previous_session(root, session);
    let short = &hash(session.as_bytes())[..24];
    fs::write(previous.join(format!("{short}.json")), "{\"old\":1}\n").unwrap();
    let older = git_metadata(root).join("craft-state");
    fs::create_dir_all(&older).unwrap();
    fs::write(older.join(format!("{short}.json")), "{\"old\":2}\n").unwrap();
    fs::create_dir_all(root.join(".capabilities/state")).unwrap();
    fs::write(root.join(".capabilities/state/telemetry-key.json"), "{}\n").unwrap();
    let before = [
        metadata(&root.join(".git")),
        metadata(&root.join(".capabilities")),
        metadata(&previous),
    ];

    let checkpoint = fixture.record("checkpoint.json");
    assert_eq!(read(&checkpoint, 1024).unwrap(), None);
    let key = file(root, "telemetry/key.json", true, None).unwrap();
    assert_eq!(read(&key, 1024).unwrap(), None);
    assert!(!root.join(".craft/state/migrations").exists());
    assert_eq!(
        [
            metadata(&root.join(".git")),
            metadata(&root.join(".capabilities")),
            metadata(&previous),
        ],
        before
    );
}

#[test]
fn concurrent_writers_serialize_on_one_lock() {
    const ROUNDS: u64 = 40;
    const WRITERS: u64 = 4;
    let fixture = Fixture::new();
    let record = fixture.record("counter.json");
    write(&record, &json!({"count": 0}), 1024, false).unwrap();
    let writers: Vec<_> = (0..WRITERS)
        .map(|_| {
            let record = record.clone();
            std::thread::spawn(move || {
                let mut done = 0;
                while done < ROUNDS {
                    let Some(_ticket) = lock(&record).unwrap() else {
                        std::thread::sleep(std::time::Duration::from_millis(1));
                        continue;
                    };
                    let count = read(&record, 1024).unwrap().unwrap()["count"]
                        .as_u64()
                        .unwrap();
                    write(&record, &json!({"count": count + 1}), 1024, false).unwrap();
                    done += 1;
                }
            })
        })
        .collect();
    for writer in writers {
        writer.join().unwrap();
    }
    assert_eq!(
        read(&record, 1024).unwrap(),
        Some(json!({"count": WRITERS * ROUNDS}))
    );
    assert_eq!(
        listing(record.parent().unwrap()),
        ["counter.json", MANIFEST],
        "no lock or temporary file remains"
    );
}

#[test]
fn a_new_session_directory_is_described_once_without_raw_identities() {
    let fixture = Fixture::new();
    let described = |session: &str, host: Option<&str>| {
        let _describing = host.map(describing);
        let record = file(&fixture.0, "record.json", true, Some(session)).unwrap();
        read(&record.with_file_name(MANIFEST), MANIFEST_BYTES)
            .unwrap()
            .unwrap()
    };
    let manifest = described("session λ", Some("claude"));
    assert_eq!(manifest["formatVersion"], 1);
    assert_eq!(manifest["host"], "claude");
    assert_eq!(
        manifest["workspace"],
        craft_engine::digest(&json!("session λ"))
    );
    assert!(manifest["created"].as_u64().unwrap() > 0);
    let directory = location(&fixture.0, Some("session λ")).unwrap().directory;
    let text = fs::read_to_string(directory.join(MANIFEST)).unwrap();
    assert!(!text.contains("session λ") && !text.contains(fixture.0.to_str().unwrap()));
    // Later writers, with or without a host, keep the first description.
    assert_eq!(described("session λ", Some("codex")), manifest);
    // Outside a hook, and for odd host names, the host is omitted; the scope ends with its guard.
    assert!(described("other", None).get("host").is_none());
    assert!(described("odd", Some("Host/1")).get("host").is_none());
    {
        let _outer = describing("claude");
        drop(describing("codex"));
        assert_eq!(described("nested", None)["host"], "claude");
    }
    assert!(described("after", None).get("host").is_none());
    // Lookups never create or describe a session.
    assert!(
        !location(&fixture.0, Some("absent"))
            .unwrap()
            .directory
            .exists()
    );
}
