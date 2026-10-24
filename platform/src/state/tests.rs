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
            read(&record, bytes - 1).unwrap_err(),
            "invalid bounded state file"
        );
        assert_eq!(
            write(&record, &case["value"], bytes - 1, pretty).unwrap_err(),
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
fn failed_atomic_publication_preserves_old_bytes_and_removes_temporary_files() {
    let fixture = Fixture::new();
    let record = fixture.record("record.json");
    write(&record, &json!({"old": true}), 64, false).unwrap();
    FAIL_PUBLISH.with(|fail| fail.set(true));
    let failed = write(&record, &json!({"next": true}), 64, false);
    FAIL_PUBLISH.with(|fail| fail.set(false));
    assert_eq!(failed.unwrap_err(), "injected publication failure");
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
            location(&fixture.0, Some(&session)).unwrap_err(),
            "invalid state session"
        );
    }
    for name in ["", ".", "..", "a/b", "a\\b", "λ.json"] {
        assert_eq!(
            file(&fixture.0, name, false, None).unwrap_err(),
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
        file(&fixture.0, "record.json", true, None).unwrap_err(),
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
        file(&fixture.0, "record.json", true, None).unwrap_err(),
        "unsafe or oversized legacy state"
    );
    fs::remove_file(legacy.join("record.json")).unwrap();
    fs::write(legacy.join("record.json"), "{}").unwrap();
    fs::write(legacy.join("other.json.lock"), "").unwrap();
    assert!(
        file(&fixture.0, "record.json", true, None)
            .unwrap_err()
            .contains("writer is busy")
    );
}
