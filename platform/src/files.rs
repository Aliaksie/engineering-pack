use std::{fs, fs::File, io, io::Read, path::Path};

#[derive(Debug)]
pub enum ReadError {
    Io(io::Error),
    /// Not a regular file, or larger than the budget before reading.
    Bounds,
    /// The file grew past the budget while it was read.
    Grew,
}

impl std::fmt::Display for ReadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(error) => error.fmt(f),
            Self::Bounds => f.write_str("file exceeds type or byte bounds"),
            Self::Grew => f.write_str("file grew beyond byte bound"),
        }
    }
}

/// How long a file must have been left alone before its metadata stands for its content.
/// Timestamps are as coarse as two seconds on some filesystems, so a write in the same tick
/// as a read could leave the metadata unchanged; a file changed this recently is read.
const SETTLED: std::time::Duration = std::time::Duration::from_secs(2);

/// A file's size and, once it has settled, a stamp of the metadata any write changes: the
/// size, modification and status-change times, device and inode on Unix; the size, last
/// write and creation times on Windows. `None` when the file does not exist.
pub fn stamp(path: &Path) -> io::Result<Option<(u64, Option<String>)>> {
    stamp_settled(path, SETTLED)
}

fn stamp_settled(
    path: &Path,
    settled: std::time::Duration,
) -> io::Result<Option<(u64, Option<String>)>> {
    let metadata = match fs::metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error),
    };
    let size = metadata.len();
    #[cfg(unix)]
    let (changed, stamp) = {
        use std::os::unix::fs::MetadataExt;
        let (seconds, nanos) = (metadata.ctime(), metadata.ctime_nsec())
            .max((metadata.mtime(), metadata.mtime_nsec()));
        let changed = std::time::UNIX_EPOCH
            + std::time::Duration::new(seconds.max(0) as u64, nanos.clamp(0, 999_999_999) as u32);
        let stamp = format!(
            "{size}:{}.{}:{}.{}:{}:{}",
            metadata.mtime(),
            metadata.mtime_nsec(),
            metadata.ctime(),
            metadata.ctime_nsec(),
            metadata.dev(),
            metadata.ino()
        );
        (Some(changed), stamp)
    };
    #[cfg(windows)]
    let (changed, stamp) = {
        use std::os::windows::fs::MetadataExt;
        let stamp = format!(
            "{size}:{}:{}",
            metadata.last_write_time(),
            metadata.creation_time()
        );
        (metadata.modified().ok(), stamp)
    };
    #[cfg(not(any(unix, windows)))]
    let (changed, stamp) = {
        let modified = metadata.modified().ok();
        let nanos = modified
            .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
            .map_or(0, |since| since.as_nanos());
        (modified, format!("{size}:{nanos}"))
    };
    let settled = changed
        .and_then(|time| std::time::SystemTime::now().duration_since(time).ok())
        .is_some_and(|age| age >= settled);
    Ok(Some((size, settled.then_some(stamp))))
}

/// Create one directory readable only by its owner where the platform supports modes. An
/// existing entry is accepted; callers verify what the path resolves to.
pub fn create_private_directory(path: &Path) -> io::Result<()> {
    #[cfg(unix)]
    let created =
        std::os::unix::fs::DirBuilderExt::mode(&mut fs::DirBuilder::new(), 0o700).create(path);
    #[cfg(not(unix))]
    let created = fs::create_dir(path);
    match created {
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => Ok(()),
        result => result,
    }
}

/// Create one new, empty file readable only by its owner where the platform supports
/// modes. An existing entry is refused with `AlreadyExists`.
pub fn create_private_file(path: &Path) -> io::Result<File> {
    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
    options.open(path)
}

/// Read a regular file within a byte budget. Refuse special files before opening
/// them: opening a FIFO for reading can otherwise wait indefinitely for a writer.
/// Callers own directory traversal and namespace policy.
pub fn read_bounded(path: &Path, max_bytes: usize) -> Result<Vec<u8>, ReadError> {
    let before = fs::symlink_metadata(path).map_err(ReadError::Io)?;
    if !before.is_file() || before.len() > max_bytes as u64 {
        return Err(ReadError::Bounds);
    }
    let file = File::open(path).map_err(ReadError::Io)?;
    let opened = file.metadata().map_err(ReadError::Io)?;
    if !opened.is_file() || opened.len() > max_bytes as u64 {
        return Err(ReadError::Bounds);
    }
    let mut bytes = Vec::new();
    file.take((max_bytes as u64).saturating_add(1))
        .read_to_end(&mut bytes)
        .map_err(ReadError::Io)?;
    if bytes.len() > max_bytes {
        return Err(ReadError::Grew);
    }
    Ok(bytes)
}

/// The last `max_bytes` of a regular file, for output whose summary comes last. Refuses
/// special files like [`read_bounded`]; a cut first line is kept as is.
pub fn read_tail(path: &Path, max_bytes: usize) -> Result<Vec<u8>, ReadError> {
    use std::io::{Seek, SeekFrom};
    let before = fs::symlink_metadata(path).map_err(ReadError::Io)?;
    if !before.is_file() {
        return Err(ReadError::Bounds);
    }
    let mut file = File::open(path).map_err(ReadError::Io)?;
    let opened = file.metadata().map_err(ReadError::Io)?;
    if !opened.is_file() {
        return Err(ReadError::Bounds);
    }
    let start = opened.len().saturating_sub(max_bytes as u64);
    file.seek(SeekFrom::Start(start)).map_err(ReadError::Io)?;
    let mut bytes = Vec::new();
    file.take(max_bytes as u64)
        .read_to_end(&mut bytes)
        .map_err(ReadError::Io)?;
    Ok(bytes)
}

#[cfg(test)]
mod tests {

    #[test]
    fn a_stamp_waits_for_the_file_to_settle_and_follows_every_write() {
        let directory = std::env::temp_dir().join(format!("craft-stamp-{}", std::process::id()));
        let _ = fs::remove_dir_all(&directory);
        fs::create_dir(&directory).unwrap();
        let file = directory.join("a.js");
        assert!(
            stamp(&file).unwrap().is_none(),
            "a missing file has no stamp"
        );
        fs::write(&file, "const a = 1;").unwrap();
        assert_eq!(
            stamp(&file).unwrap(),
            Some((12, None)),
            "just written: read it"
        );
        let hour = std::time::Duration::from_secs(3600);
        assert_eq!(stamp_settled(&file, hour).unwrap(), Some((12, None)));
        let zero = std::time::Duration::ZERO;
        let (_, first) = stamp_settled(&file, zero).unwrap().unwrap();
        let first = first.unwrap();
        assert_eq!(
            stamp_settled(&file, zero).unwrap().unwrap().1.as_deref(),
            Some(first.as_str())
        );
        // Same size, modification time put back: the status-change time still moves on Unix.
        let modified = fs::metadata(&file).unwrap().modified().unwrap();
        std::thread::sleep(std::time::Duration::from_millis(20));
        fs::write(&file, "const b = 1;").unwrap();
        File::options()
            .write(true)
            .open(&file)
            .unwrap()
            .set_modified(modified)
            .unwrap();
        let (size, second) = stamp_settled(&file, zero).unwrap().unwrap();
        assert_eq!(size, 12);
        #[cfg(unix)]
        assert_ne!(second.unwrap(), first);
        #[cfg(not(unix))]
        drop(second);
        fs::remove_dir_all(&directory).unwrap();
    }
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(0);

    struct Fixture(std::path::PathBuf);
    impl Fixture {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!(
                "craft-files-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir(&path).unwrap();
            Self(path)
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn bounded_regular_files_and_missing_paths() {
        let fixture = Fixture::new();
        let file = fixture.0.join("source.mjs");
        fs::write(&file, b"abcd").unwrap();
        assert_eq!(read_bounded(&file, 4).unwrap(), b"abcd");
        assert!(matches!(read_bounded(&file, 3), Err(ReadError::Bounds)));
        assert!(matches!(
            read_bounded(&fixture.0, 4),
            Err(ReadError::Bounds)
        ));
        assert!(matches!(
            read_bounded(&fixture.0.join("missing"), 4),
            Err(ReadError::Io(error)) if error.kind() == io::ErrorKind::NotFound
        ));
        fs::write(&file, b"").unwrap();
        assert_eq!(read_bounded(&file, 0).unwrap(), b"");
    }

    #[cfg(unix)]
    #[test]
    fn special_files_are_rejected_before_open() {
        let fixture = Fixture::new();
        let fifo = fixture.0.join("pipe.mjs");
        assert!(
            std::process::Command::new("mkfifo")
                .arg(&fifo)
                .status()
                .unwrap()
                .success()
        );
        // The old reader blocks on this FIFO.
        assert!(matches!(read_bounded(&fifo, 100), Err(ReadError::Bounds)));
    }

    #[test]
    fn a_tail_read_keeps_the_end_of_a_large_file() {
        let fixture = Fixture::new();
        let path = fixture.0.join("out.txt");
        fs::write(&path, format!("{}summary line\n", "x".repeat(10_000))).unwrap();
        assert_eq!(read_tail(&path, 13).unwrap(), b"summary line\n");
        assert_eq!(read_tail(&path, 1 << 20).unwrap().len(), 10_013);
        assert!(matches!(read_tail(&fixture.0, 10), Err(ReadError::Bounds)));
    }
}
