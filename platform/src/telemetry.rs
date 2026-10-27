//! Local telemetry store: the optional `.craft/telemetry.json` configuration, a private
//! per-worktree key and a bounded JSONL log with one backup, written under the state
//! `.lock` protocol.
use crate::Error;
use crate::state;
use craft_engine::telemetry::report::Storage;
use serde_json::{Value, json};
use std::{
    fs::{self, OpenOptions},
    io::{ErrorKind, Write},
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

pub const MAX_BYTES: u64 = 1_048_576;
pub const BATCH_EVENTS: usize = 32;
const LINE_BYTES: usize = 8192;
const LOCK_WAIT: Duration = Duration::from_millis(25);

/// The physical project root, or the absolute spelling when it cannot be resolved.
pub fn root(root: &Path) -> PathBuf {
    state::canonical_root(root)
        .unwrap_or_else(|_| std::path::absolute(root).unwrap_or_else(|_| root.to_path_buf()))
}

/// Raw configuration, `{}` when absent. Validation belongs to the caller.
pub fn configuration(root: &Path) -> Result<Value, Error> {
    let folder = root.join(".craft");
    if fs::symlink_metadata(&folder).is_ok_and(|meta| meta.file_type().is_symlink()) {
        return Err("invalid telemetry directory".into());
    }
    Ok(state::read(&folder.join("telemetry.json"), 4096)?.unwrap_or_else(|| json!({})))
}

pub fn random_bytes<const N: usize>() -> Result<[u8; N], Error> {
    let mut bytes = [0; N];
    getrandom::fill(&mut bytes).map_err(|e| Error::from(e.to_string()))?;
    Ok(bytes)
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// A random RFC 4122 version 4 UUID, as `crypto.randomUUID` formats it.
pub fn uuid() -> Result<String, Error> {
    let mut bytes = random_bytes::<16>()?;
    bytes[6] = bytes[6] & 0x0f | 0x40;
    bytes[8] = bytes[8] & 0x3f | 0x80;
    let text = hex(&bytes);
    Ok(format!(
        "{}-{}-{}-{}-{}",
        &text[..8],
        &text[8..12],
        &text[12..16],
        &text[16..20],
        &text[20..]
    ))
}

/// The worktree's pseudonym for `session`, creating the private key on first use.
pub fn identity(root: &Path, session: &str) -> Result<String, Error> {
    let file = state::file(root, "telemetry-key.json", true, None)?;
    let mut data = state::read(&file, 256)?;
    if data.is_none() {
        let bytes = format!("{}\n", json!({ "key": hex(&random_bytes::<32>()?) }));
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        match options
            .open(&file)
            .and_then(|mut handle| handle.write_all(bytes.as_bytes()))
        {
            Ok(()) => {}
            Err(error) if error.kind() == ErrorKind::AlreadyExists => {}
            Err(error) => return Err(error.into()),
        }
        data = state::read(&file, 256)?;
    }
    pseudonym(data.as_ref(), session)
}

/// The pseudonym for `session` when the worktree key exists; never creates the key.
pub fn existing_identity(root: &Path, session: &str) -> Result<Option<String>, Error> {
    let file = state::file(root, "telemetry-key.json", false, None)?;
    match state::read(&file, 256)? {
        None | Some(Value::Null) => Ok(None),
        data => pseudonym(data.as_ref(), session).map(Some),
    }
}

fn pseudonym(data: Option<&Value>, session: &str) -> Result<String, Error> {
    let key = data
        .and_then(|data| data["key"].as_str())
        .unwrap_or_default();
    if !craft_engine::values::lower_hex(key, 64) {
        return Err("invalid telemetry key".into());
    }
    Ok(craft_engine::telemetry::session_identity(key, session))
}

fn regular(file: &Path) -> Result<u64, Error> {
    match fs::symlink_metadata(file) {
        Ok(meta) if meta.is_file() && meta.len() <= MAX_BYTES => Ok(meta.len()),
        Ok(_) => Err("invalid telemetry file".into()),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(0),
        Err(error) => Err(error.into()),
    }
}

/// Retained records of the log and its backup, oldest file first, with storage facts.
/// Reads take no lock, so a concurrent writer may be partly visible.
pub fn snapshot(root: &Path) -> Result<(Vec<Value>, Storage), Error> {
    let file = state::file(root, "telemetry.jsonl", false, None)?;
    let mut backup = file.as_os_str().to_owned();
    backup.push(".1");
    let mut records = Vec::new();
    let mut storage = Storage {
        max_file_bytes: MAX_BYTES,
        ..Storage::default()
    };
    for (path, is_backup) in [(PathBuf::from(backup), true), (file, false)] {
        match fs::symlink_metadata(&path) {
            Ok(meta) if meta.is_file() && meta.len() <= MAX_BYTES => {}
            Ok(_) => return Err("invalid telemetry file".into()),
            Err(error) if error.kind() == ErrorKind::NotFound => continue,
            Err(error) => return Err(error.into()),
        }
        let content = fs::read(&path)?;
        storage.files += 1;
        storage.bytes += content.len() as u64;
        storage.backup_present |= is_backup;
        for line in String::from_utf8_lossy(&content).split('\n') {
            if line.is_empty() {
                continue;
            }
            if line.len() > LINE_BYTES {
                storage.oversized_lines += 1;
                continue;
            }
            match serde_json::from_str::<Value>(line) {
                Ok(record) if record["schemaVersion"].as_f64() == Some(1.0) => records.push(record),
                Ok(_) => storage.unsupported_records += 1,
                Err(_) => storage.malformed_lines += 1,
            }
        }
    }
    Ok((records, storage))
}

/// Append up to 32 events atomically with respect to other writers. Returns `false`,
/// never an error, when the batch is dropped: telemetry is advisory. A writer waits
/// briefly for an active lock but never removes it.
pub fn append(root: &Path, events: &[Value]) -> bool {
    write_batch(root, events).unwrap_or(false)
}

fn write_batch(root: &Path, events: &[Value]) -> Result<bool, Error> {
    if events.is_empty() || events.len() > BATCH_EVENTS {
        return Ok(false);
    }
    let mut text = String::new();
    for event in events {
        let line = format!("{event}\n");
        if line.len() > LINE_BYTES {
            return Ok(false);
        }
        text.push_str(&line);
    }
    let file = state::file(root, "telemetry.jsonl", true, None)?;
    let started = Instant::now();
    let _ticket = loop {
        if let Some(ticket) = state::lock(&file)? {
            break ticket;
        }
        let Some(remaining) = LOCK_WAIT
            .checked_sub(started.elapsed())
            .filter(|r| !r.is_zero())
        else {
            return Ok(false);
        };
        std::thread::sleep(remaining.min(Duration::from_millis(2)));
    };
    let size = regular(&file)?;
    let mut backup = file.as_os_str().to_owned();
    backup.push(".1");
    let backup = PathBuf::from(backup);
    regular(&backup)?;
    if size + text.len() as u64 > MAX_BYTES {
        match fs::remove_file(&backup) {
            Ok(()) => {}
            Err(error) if error.kind() == ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
        fs::rename(&file, &backup)?;
    }
    let mut options = OpenOptions::new();
    options.append(true).create(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options
            .mode(0o600)
            .custom_flags(rustix::fs::OFlags::NOFOLLOW.bits() as i32);
    }
    options
        .open(&file)
        .and_then(|mut handle| handle.write_all(text.as_bytes()))?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(0);

    struct Fixture(PathBuf);
    impl Fixture {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!(
                "craft-telemetry-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir(&path).unwrap();
            Self(state::canonical_root(&path).unwrap())
        }
        fn log(&self) -> PathBuf {
            state::file(&self.0, "telemetry.jsonl", false, None).unwrap()
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn identities_are_stable_per_worktree_and_uuids_are_version_four() {
        let fixture = Fixture::new();
        let first = identity(&fixture.0, "s1").unwrap();
        assert_eq!(identity(&fixture.0, "s1").unwrap(), first);
        assert_ne!(identity(&fixture.0, "s2").unwrap(), first);
        let key = state::file(&fixture.0, "telemetry-key.json", false, None).unwrap();
        fs::write(&key, "{\"key\":\"short\"}\n").unwrap();
        assert!(identity(&fixture.0, "s1").is_err());
        let id = uuid().unwrap();
        assert_eq!(id.len(), 36);
        assert_eq!(&id[14..15], "4");
        assert!(matches!(&id[19..20], "8" | "9" | "a" | "b"));
        assert_ne!(uuid().unwrap(), id);
    }

    #[test]
    fn batches_are_bounded_locked_and_rotated_once() {
        let fixture = Fixture::new();
        assert!(!append(&fixture.0, &[]));
        assert!(!append(&fixture.0, &vec![json!({}); BATCH_EVENTS + 1]));
        assert!(!append(
            &fixture.0,
            &[json!({ "a": "x".repeat(LINE_BYTES) })]
        ));
        assert!(append(&fixture.0, &[json!({ "n": 1 }), json!({ "n": 2 })]));
        let log = fixture.log();
        assert_eq!(fs::read_to_string(&log).unwrap(), "{\"n\":1}\n{\"n\":2}\n");
        let lock = PathBuf::from(format!("{}.lock", log.display()));
        fs::write(&lock, "other writer").unwrap();
        let started = Instant::now();
        assert!(!append(&fixture.0, &[json!({ "n": 3 })]));
        assert!(started.elapsed() >= LOCK_WAIT);
        assert_eq!(fs::read_to_string(&lock).unwrap(), "other writer");
        fs::remove_file(&lock).unwrap();
        fs::write(&log, vec![b'x'; MAX_BYTES as usize - 4]).unwrap();
        assert!(append(&fixture.0, &[json!({ "n": 4 })]));
        assert_eq!(fs::read_to_string(&log).unwrap(), "{\"n\":4}\n");
        assert_eq!(
            fs::metadata(format!("{}.1", log.display())).unwrap().len(),
            MAX_BYTES - 4
        );
        fs::write(&log, vec![b'x'; MAX_BYTES as usize + 1]).unwrap();
        assert!(!append(&fixture.0, &[json!({ "n": 5 })]));
    }

    #[test]
    fn a_writer_waits_for_a_lock_released_within_the_wait() {
        let fixture = Fixture::new();
        let log = state::file(&fixture.0, "telemetry.jsonl", true, None).unwrap();
        let lock = PathBuf::from(format!("{}.lock", log.display()));
        fs::write(&lock, "").unwrap();
        let releaser = {
            let lock = lock.clone();
            std::thread::spawn(move || {
                std::thread::sleep(Duration::from_millis(5));
                fs::remove_file(lock).unwrap();
            })
        };
        assert!(append(&fixture.0, &[json!({ "n": 1 })]));
        releaser.join().unwrap();
        assert_eq!(fs::read_to_string(&log).unwrap(), "{\"n\":1}\n");
    }

    #[test]
    fn concurrent_batch_writers_append_complete_groups() {
        let fixture = Fixture::new();
        let root = fixture.0.clone();
        let writers: Vec<_> = (0..4)
            .map(|writer| {
                let root = root.clone();
                std::thread::spawn(move || {
                    (0..8)
                        .filter(|n| {
                            let batch: Vec<Value> = (0..5)
                                .map(|i| json!({ "writer": writer, "n": n, "i": i }))
                                .collect();
                            append(&root, &batch)
                        })
                        .count()
                })
            })
            .collect();
        let accepted: usize = writers.into_iter().map(|w| w.join().unwrap()).sum();
        assert!(accepted > 0);
        let text = fs::read_to_string(fixture.log()).unwrap();
        let lines: Vec<&str> = text.lines().collect();
        // Each accepted batch is one complete group of five records.
        assert_eq!(lines.len(), accepted * 5);
        for line in lines {
            let event: Value = serde_json::from_str(line).unwrap();
            assert!(event["writer"].is_u64() && event["n"].is_u64(), "{line}");
        }
    }

    #[cfg(unix)]
    #[test]
    fn a_symlinked_log_is_refused_without_creating_its_target() {
        let fixture = Fixture::new();
        let log = state::file(&fixture.0, "telemetry.jsonl", true, None).unwrap();
        std::os::unix::fs::symlink(fixture.0.join("target"), &log).unwrap();
        assert!(!append(&fixture.0, &[json!({ "n": 1 })]));
        assert!(!fixture.0.join("target").exists());
    }

    #[test]
    fn snapshots_count_skipped_lines_and_never_create_keys() {
        let fixture = Fixture::new();
        assert_eq!(existing_identity(&fixture.0, "s1").unwrap(), None);
        let (records, storage) = snapshot(&fixture.0).unwrap();
        assert!(records.is_empty());
        assert_eq!((storage.files, storage.backup_present), (0, false));
        assert!(!fixture.0.join(".craft").exists());
        let log = state::file(&fixture.0, "telemetry.jsonl", true, None).unwrap();
        fs::write(
            format!("{}.1", log.display()),
            "{\"schemaVersion\":1,\"n\":0}\n",
        )
        .unwrap();
        let oversized = "x".repeat(LINE_BYTES + 1);
        let mut text = format!(
            "broken\nnull\n{{\"schemaVersion\":2}}\n{oversized}\n{{\"schemaVersion\":1.0}}\n"
        )
        .into_bytes();
        text.extend([0xff, b'\n']);
        fs::write(&log, &text).unwrap();
        let (records, storage) = snapshot(&fixture.0).unwrap();
        assert_eq!(records.len(), 2);
        assert_eq!(records[0]["n"], 0);
        assert_eq!(
            (
                storage.files,
                storage.backup_present,
                storage.malformed_lines,
                storage.unsupported_records,
                storage.oversized_lines
            ),
            (2, true, 2, 2, 1)
        );
        assert_eq!(storage.max_file_bytes, MAX_BYTES);
        let first = identity(&fixture.0, "s1").unwrap();
        assert_eq!(existing_identity(&fixture.0, "s1").unwrap(), Some(first));
        #[cfg(unix)]
        {
            fs::remove_file(&log).unwrap();
            std::os::unix::fs::symlink(fixture.0.join("target"), &log).unwrap();
            assert_eq!(
                snapshot(&fixture.0).unwrap_err().to_string(),
                "invalid telemetry file"
            );
        }
    }

    #[test]
    fn configuration_is_bounded_and_refuses_linked_folders() {
        let fixture = Fixture::new();
        assert_eq!(configuration(&fixture.0).unwrap(), json!({}));
        fs::create_dir(fixture.0.join(".craft")).unwrap();
        fs::write(
            fixture.0.join(".craft/telemetry.json"),
            r#"{"local":false}"#,
        )
        .unwrap();
        assert_eq!(
            configuration(&fixture.0).unwrap(),
            json!({ "local": false })
        );
        fs::write(fixture.0.join(".craft/telemetry.json"), vec![b' '; 4097]).unwrap();
        assert!(configuration(&fixture.0).is_err());
        #[cfg(unix)]
        {
            let other = Fixture::new();
            std::os::unix::fs::symlink(fixture.0.join(".craft"), other.0.join(".craft")).unwrap();
            assert!(configuration(&other.0).is_err());
        }
    }
}
