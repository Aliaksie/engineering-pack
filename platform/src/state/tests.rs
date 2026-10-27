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
        assert_eq!(listing(record.parent().unwrap()), ["record.json"]);
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
    assert_eq!(listing(record.parent().unwrap()), ["record.json"]);
    write(&record, &json!({"next": true}), 64, false).unwrap();
    assert_eq!(read(&record, 64).unwrap(), Some(json!({"next": true})));
}

#[test]
fn routing_generation_migrates_with_its_record_and_remains_source_bound() {
    let fixture = Fixture::new();
    let legacy = fixture.0.join(".capabilities").join("state");
    fs::create_dir_all(&legacy).unwrap();
    fs::write(legacy.join("routing-demo.json"), "{\"generation\":2}\n").unwrap();
    fs::write(legacy.join("routing-demo.json.generation"), "2\n").unwrap();
    let record = file(&fixture.0, "routing-demo.json", false, SESSION).unwrap();
    assert_eq!(fs::read_to_string(&record).unwrap(), "{\"generation\":2}\n");
    let generation = record.with_file_name("routing-demo.json.generation");
    assert_eq!(fs::read_to_string(&generation).unwrap(), "2\n");
    let receipts: Vec<String> = listing(record.parent().unwrap())
        .into_iter()
        .filter(|name| name.starts_with("migration-"))
        .collect();
    assert_eq!(receipts.len(), 1);
    let receipt = read(&record.with_file_name(&receipts[0]), 4096)
        .unwrap()
        .unwrap();
    assert_eq!(receipt["formatVersion"], 1);
    let source = receipt["source"].as_str().unwrap();
    assert!(source.len() == 24 && source.bytes().all(|b| b.is_ascii_hexdigit()));
    fs::write(legacy.join("routing-demo.json.generation"), "3\n").unwrap();
    assert!(
        file(&fixture.0, "routing-demo.json", false, SESSION)
            .unwrap_err()
            .to_string()
            .contains("changed after migration")
    );
    assert_eq!(fs::read_to_string(&generation).unwrap(), "2\n");
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
        place.directory.ends_with(
            Path::new(".craft/state")
                .join(&place.worktree_id)
                .join(&place.session_id)
        )
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
    for name in ["", ".", "..", "a/b", "a\\b", "λ.json"] {
        assert_eq!(
            file(&fixture.0, name, false, None).unwrap_err().to_string(),
            "invalid state filename"
        );
    }
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
        file(&fixture.0, "record.json", true, None)
            .unwrap_err()
            .to_string(),
        "state path must be a directory without symlinks"
    );
    assert!(listing(&elsewhere.0).is_empty());
}

#[cfg(unix)]
#[test]
fn legacy_hard_links_and_locks_block_migration() {
    let fixture = Fixture::new();
    let legacy = fixture.0.join(".capabilities").join("state");
    fs::create_dir_all(&legacy).unwrap();
    fs::write(fixture.0.join("outside.json"), "{}").unwrap();
    fs::hard_link(fixture.0.join("outside.json"), legacy.join("record.json")).unwrap();
    assert_eq!(
        file(&fixture.0, "record.json", true, None)
            .unwrap_err()
            .to_string(),
        "unsafe or oversized legacy state"
    );
    fs::remove_file(legacy.join("record.json")).unwrap();
    fs::write(legacy.join("record.json"), "{}").unwrap();
    fs::write(legacy.join("other.json.lock"), "").unwrap();
    let busy = file(&fixture.0, "record.json", true, None).unwrap_err();
    assert!(busy.to_string().contains("writer is busy"));
    assert_eq!(busy.kind(), Some(ErrorKind::ResourceBusy));
}

/// Every file under `directory` with its bytes and modification time.
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
fn legacy_records_migrate_without_git_metadata_or_configuration_writes() {
    for namespace in ["git", "craft", "capabilities"] {
        let fixture = Fixture::new();
        let root = &fixture.0;
        let git = git_metadata(root);
        fs::create_dir(root.join(".craft")).unwrap();
        fs::write(
            root.join(".craft/project.json"),
            r#"{"consumer":"preserve"}"#,
        )
        .unwrap();
        fs::write(root.join(".craft/telemetry.json"), r#"{"local":false}"#).unwrap();
        let old = match namespace {
            "git" => git
                .join("craft-state")
                .join(&hash(root.to_str().unwrap().as_bytes())[..24]),
            "craft" => root.join(".craft/state"),
            _ => root.join(".capabilities/state"),
        };
        fs::create_dir_all(old.join("sessions/owned/evidence")).unwrap();
        fs::write(old.join("record.json"), "{\"history\":[1,2]}\n").unwrap();
        fs::write(old.join("telemetry.jsonl"), "{\"schemaVersion\":1}\n").unwrap();
        fs::write(
            old.join("telemetry.jsonl.1"),
            "{\"schemaVersion\":1,\"older\":true}\n",
        )
        .unwrap();
        fs::write(
            old.join("sessions/owned/evidence/artifact.txt"),
            "retained evidence",
        )
        .unwrap();
        let before = metadata(&git);
        let session = Some("existing-session");
        let record = file(root, "record.json", false, session).unwrap();
        assert_eq!(
            read(&record, 1024).unwrap(),
            Some(json!({"history": [1, 2]})),
            "{namespace}"
        );
        write(&record, &json!({"history": [1, 2, 3]}), 1024, false).unwrap();
        assert_eq!(file(root, "record.json", false, session).unwrap(), record);
        let log = file(root, "telemetry.jsonl", false, None).unwrap();
        assert_eq!(
            fs::read_to_string(format!("{}.1", log.display())).unwrap(),
            "{\"schemaVersion\":1,\"older\":true}\n"
        );
        let guard = file(root, "workspace-guard.json", false, None).unwrap();
        assert_eq!(
            fs::read_to_string(guard.with_file_name("sessions/owned/evidence/artifact.txt"))
                .unwrap(),
            "retained evidence"
        );
        assert_eq!(
            metadata(&git),
            before,
            "{namespace}: Git metadata unchanged"
        );
        assert_eq!(
            fs::read_to_string(old.join("record.json")).unwrap(),
            "{\"history\":[1,2]}\n"
        );
        assert_eq!(
            fs::read_to_string(root.join(".craft/project.json")).unwrap(),
            r#"{"consumer":"preserve"}"#
        );
        assert_eq!(
            fs::read_to_string(root.join(".craft/telemetry.json")).unwrap(),
            r#"{"local":false}"#
        );
        // An older writer resuming on the legacy store cannot silently fork history.
        fs::write(old.join("record.json"), r#"{"history":[4]}"#).unwrap();
        assert!(
            file(root, "record.json", false, session)
                .unwrap_err()
                .to_string()
                .contains("changed after migration")
        );
        assert_eq!(
            read(&record, 1024).unwrap(),
            Some(json!({"history": [1, 2, 3]}))
        );
    }
}

#[cfg(unix)]
#[test]
fn migration_refuses_old_locks_malformed_records_symlinks_and_destination_conflicts() {
    let fixture = Fixture::new();
    let legacy = fixture.0.join(".craft/state");
    fs::create_dir_all(&legacy).unwrap();
    let old = legacy.join("record.json");
    let target = fixture.record("record.json");
    fs::write(&old, r#"{"value":1}"#).unwrap();
    fs::write(legacy.join("record.json.lock"), "").unwrap();
    assert!(
        file(&fixture.0, "record.json", true, SESSION)
            .unwrap_err()
            .to_string()
            .contains("legacy state writer is busy")
    );
    fs::remove_file(legacy.join("record.json.lock")).unwrap();
    fs::write(&old, "broken").unwrap();
    assert!(file(&fixture.0, "record.json", true, SESSION).is_err());
    fs::remove_file(&old).unwrap();
    std::os::unix::fs::symlink(&target, &old).unwrap();
    assert_eq!(
        file(&fixture.0, "record.json", true, SESSION)
            .unwrap_err()
            .to_string(),
        "unsafe or oversized legacy state"
    );
    fs::remove_file(&old).unwrap();
    fs::write(&old, r#"{"value":1}"#).unwrap();
    fs::write(&target, r#"{"value":2}"#).unwrap();
    assert_eq!(
        file(&fixture.0, "record.json", true, SESSION)
            .unwrap_err()
            .to_string(),
        "legacy state destination conflict"
    );
    assert_eq!(fs::read_to_string(&target).unwrap(), r#"{"value":2}"#);
    // A matching file without a published receipt is an interrupted copy.
    fs::write(&target, r#"{"value":1}"#).unwrap();
    assert_eq!(
        file(&fixture.0, "record.json", true, SESSION).unwrap(),
        target
    );
    assert_eq!(read(&target, 1024).unwrap(), Some(json!({"value": 1})));
}

#[test]
fn an_overlapping_migration_writer_is_refused_without_partial_copies() {
    let fixture = Fixture::new();
    // Spell the source as migration does: the receipt name hashes the path text, and on
    // Windows `join(".craft/state")` keeps the forward slash.
    let legacy = fixture.0.join(".craft").join("state");
    fs::create_dir_all(&legacy).unwrap();
    fs::write(legacy.join("record.json"), r#"{"history":[1]}"#).unwrap();
    let target = location(&fixture.0, SESSION).unwrap().directory;
    fs::create_dir_all(&target).unwrap();
    let old = legacy.join("record.json");
    let receipt = target.join(format!(
        "migration-{}.json",
        short(old.to_str().unwrap().as_bytes())
    ));
    fs::write(format!("{}.lock", receipt.display()), "").unwrap();
    let busy = file(&fixture.0, "record.json", true, SESSION).unwrap_err();
    assert_eq!(busy.to_string(), "state migration is busy");
    assert_eq!(busy.kind(), Some(ErrorKind::ResourceBusy));
    assert!(!target.join("record.json").exists());
    fs::remove_file(format!("{}.lock", receipt.display())).unwrap();
    let record = file(&fixture.0, "record.json", true, SESSION).unwrap();
    assert_eq!(read(&record, 1024).unwrap(), Some(json!({"history": [1]})));
    assert!(
        listing(&target)
            .iter()
            .all(|name| !name.ends_with(".tmp") && !name.ends_with(".lock"))
    );
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
fn linked_worktree_migration_reads_its_own_legacy_git_directory() {
    let fixture = Fixture::new();
    let main = git_metadata(&fixture.0);
    let linked = fixture.0.join("linked");
    let linked_git = main.join("worktrees/linked");
    fs::create_dir_all(&linked_git).unwrap();
    fs::create_dir(&linked).unwrap();
    fs::write(
        linked.join(".git"),
        format!("gitdir: {}\n", linked_git.display()),
    )
    .unwrap();
    let legacy = linked_git
        .join("craft-state")
        .join(&hash(linked.to_str().unwrap().as_bytes())[..24]);
    fs::create_dir_all(&legacy).unwrap();
    fs::write(legacy.join("record.json"), r#"{"worktree":"linked"}"#).unwrap();
    let before = metadata(&main);
    let session = Some("linked-session");
    let record = file(&linked, "record.json", false, session).unwrap();
    assert_eq!(
        read(&record, 1024).unwrap(),
        Some(json!({"worktree": "linked"}))
    );
    let main_record = file(&fixture.0, "record.json", false, session).unwrap();
    assert_eq!(read(&main_record, 1024).unwrap(), None);
    assert_eq!(metadata(&main), before);
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
        ["counter.json"],
        "no lock or temporary file remains"
    );
}
