use std::{fs, fs::File, io::Read, path::Path};

/// Read a regular file within a byte budget. Refuse special files before opening
/// them: opening a FIFO for reading can otherwise wait indefinitely for a writer.
/// Callers own directory traversal and namespace policy.
pub fn read_bounded(path: &Path, max_bytes: usize) -> Result<Vec<u8>, String> {
    let before = fs::symlink_metadata(path).map_err(|e| e.to_string())?;
    if !before.is_file() || before.len() > max_bytes as u64 {
        return Err("source exceeds file/byte bounds".into());
    }
    let file = File::open(path).map_err(|e| e.to_string())?;
    let opened = file.metadata().map_err(|e| e.to_string())?;
    if !opened.is_file() || opened.len() > max_bytes as u64 {
        return Err("source exceeds file/byte bounds".into());
    }
    let mut bytes = Vec::new();
    file.take((max_bytes as u64).saturating_add(1))
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() > max_bytes {
        return Err("source grew beyond byte bound".into());
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
        assert!(read_bounded(&file, 3).is_err());
        assert!(read_bounded(&fixture.0, 4).is_err());
        assert!(read_bounded(&fixture.0.join("missing"), 4).is_err());
        fs::write(&file, b"").unwrap();
        assert_eq!(read_bounded(&file, 0).unwrap(), b"");
    }

    #[test]
    fn source_paths_cannot_escape_the_selected_root() {
        let fixture = Fixture::new();
        fs::write(fixture.0.join("source.mjs"), b"export const x = 1;").unwrap();
        for path in [
            "../source.mjs",
            "/source.mjs",
            "a/../source.mjs",
            "a\\source.mjs",
        ] {
            assert!(
                crate::repository::read_source(&fixture.0, path).is_err(),
                "{path}"
            );
        }
        assert!(crate::repository::read_source(&fixture.0, "source.mjs").is_ok());
    }

    #[cfg(unix)]
    #[test]
    fn symlinks_and_special_files_are_rejected_before_open() {
        use std::os::unix::fs::symlink;
        let fixture = Fixture::new();
        fs::write(fixture.0.join("source.mjs"), b"source").unwrap();
        symlink("source.mjs", fixture.0.join("alias.mjs")).unwrap();
        symlink("missing", fixture.0.join("dangling.mjs")).unwrap();
        symlink(&fixture.0, fixture.0.join("directory")).unwrap();
        for path in ["alias.mjs", "dangling.mjs", "directory/source.mjs"] {
            assert!(crate::repository::read_source(&fixture.0, path).is_err());
        }
        let fifo = fixture.0.join("pipe.mjs");
        assert!(
            std::process::Command::new("mkfifo")
                .arg(&fifo)
                .status()
                .unwrap()
                .success()
        );
        // Directly exercise the pre-open guard: the old reader blocks on this FIFO.
        assert_eq!(
            read_bounded(&fifo, 100).unwrap_err(),
            "source exceeds file/byte bounds"
        );
        assert!(crate::repository::read_source(&fixture.0, "pipe.mjs").is_err());
    }
}
