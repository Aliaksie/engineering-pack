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

#[cfg(test)]
mod tests {
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
}
