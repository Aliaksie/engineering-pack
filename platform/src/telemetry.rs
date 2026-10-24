//! Local telemetry store shared with `platform/node/telemetry.mjs`: the optional
//! `.craft/telemetry.json` configuration, a private per-worktree key and a bounded
//! JSONL log with one backup, written under the state `.lock` protocol.
use crate::state;
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
pub fn configuration(root: &Path) -> Result<Value, String> {
    let folder = root.join(".craft");
    if fs::symlink_metadata(&folder).is_ok_and(|meta| meta.file_type().is_symlink()) {
        return Err("invalid telemetry directory".into());
    }
    Ok(state::read(&folder.join("telemetry.json"), 4096)?.unwrap_or_else(|| json!({})))
}

pub fn random_bytes<const N: usize>() -> Result<[u8; N], String> {
    let mut bytes = [0; N];
    getrandom::fill(&mut bytes).map_err(|e| e.to_string())?;
    Ok(bytes)
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// A random RFC 4122 version 4 UUID, as `crypto.randomUUID` formats it.
pub fn uuid() -> Result<String, String> {
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
pub fn identity(root: &Path, session: &str) -> Result<String, String> {
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
            Err(error) => return Err(error.to_string()),
        }
        data = state::read(&file, 256)?;
    }
    let key = data
        .as_ref()
        .and_then(|data| data["key"].as_str())
        .unwrap_or_default();
    if key.len() != 64 || !key.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f')) {
        return Err("invalid telemetry key".into());
    }
    Ok(craft_engine::telemetry::session_identity(key, session))
}

fn regular(file: &Path) -> Result<u64, String> {
    match fs::symlink_metadata(file) {
        Ok(meta) if meta.is_file() && meta.len() <= MAX_BYTES => Ok(meta.len()),
        Ok(_) => Err("invalid telemetry file".into()),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(0),
        Err(error) => Err(error.to_string()),
    }
}

/// Append up to 32 events atomically with respect to other writers. Returns `false`,
/// never an error, when the batch is dropped: telemetry is advisory. A writer waits
/// briefly for an active lock but never removes it.
pub fn append(root: &Path, events: &[Value]) -> bool {
    write_batch(root, events).unwrap_or(false)
}

fn write_batch(root: &Path, events: &[Value]) -> Result<bool, String> {
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
            Err(error) => return Err(error.to_string()),
        }
        fs::rename(&file, &backup).map_err(|e| e.to_string())?;
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
        .and_then(|mut handle| handle.write_all(text.as_bytes()))
        .map_err(|e| e.to_string())?;
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
