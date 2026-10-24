//! Bounded JSON documents a person supplies: project configuration, private preferences
//! and command input. `platform/node/resources/` keeps the same rules until its last
//! command migrates.
use crate::files::{ReadError, read_bounded};
use serde_json::{Value, json};
use std::{fmt, fs, io, path::Path};

pub const PROJECT_BYTES: usize = 1024 * 1024;
pub const PREFERENCE_BYTES: usize = 65_536;
pub const INPUT_BYTES: usize = 65_536;

#[derive(Debug)]
pub enum DocumentError {
    Io(io::Error),
    Invalid(String),
}

impl fmt::Display for DocumentError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => error.fmt(f),
            Self::Invalid(message) => f.write_str(message),
        }
    }
}

impl DocumentError {
    pub fn io_kind(&self) -> Option<io::ErrorKind> {
        match self {
            Self::Io(error) => Some(error.kind()),
            Self::Invalid(_) => None,
        }
    }
}

fn invalid(message: &str) -> DocumentError {
    DocumentError::Invalid(message.into())
}

fn parse(bytes: &[u8]) -> Result<Value, DocumentError> {
    serde_json::from_slice(bytes).map_err(|error| DocumentError::Invalid(error.to_string()))
}

fn bounded(path: &Path, max_bytes: usize, message: &str) -> Result<Value, DocumentError> {
    match read_bounded(path, max_bytes) {
        Ok(bytes) => parse(&bytes),
        Err(ReadError::Io(error)) => Err(DocumentError::Io(error)),
        Err(_) => Err(invalid(message)),
    }
}

/// `.craft/project.json`, or `{"formatVersion":1}` when absent. Neither the folder nor
/// the file may be a symlink. Unlike the JS reader, the file is bounded.
pub fn project(root: &Path) -> Result<Value, DocumentError> {
    let folder = root.join(".craft");
    let file = folder.join("project.json");
    for candidate in [&folder, &file] {
        if fs::symlink_metadata(candidate).is_ok_and(|meta| meta.file_type().is_symlink()) {
            return Err(invalid("project configuration may not be a symlink"));
        }
    }
    if fs::symlink_metadata(&file).is_err() {
        return Ok(json!({ "formatVersion": 1 }));
    }
    bounded(
        &file,
        PROJECT_BYTES,
        "project configuration must be a bounded JSON file",
    )
}

/// Private preferences, or `{}` when absent. On POSIX the file and its folder must be
/// owned by the current user and not group- or world-writable.
pub fn preferences(file: &Path) -> Result<Value, DocumentError> {
    let meta = match fs::symlink_metadata(file) {
        Ok(meta) => meta,
        Err(error)
            if matches!(
                error.kind(),
                io::ErrorKind::NotFound | io::ErrorKind::NotADirectory
            ) =>
        {
            return Ok(json!({}));
        }
        Err(error) => return Err(DocumentError::Io(error)),
    };
    // `path.dirname`: a bare file name lives in the working directory.
    let parent = file
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let parent = fs::symlink_metadata(parent).map_err(DocumentError::Io)?;
    for entry in [&parent, &meta] {
        if entry.file_type().is_symlink() || !private(entry) {
            return Err(invalid(
                "preferences must be user-owned, non-symlink and not writable by others",
            ));
        }
    }
    let message = "preferences must be a bounded JSON file";
    if !meta.is_file() || meta.len() > PREFERENCE_BYTES as u64 {
        return Err(invalid(message));
    }
    read_preferences(file, &meta, message)
}

#[cfg(unix)]
fn private(meta: &fs::Metadata) -> bool {
    use std::os::unix::fs::MetadataExt;
    meta.uid() == rustix::process::getuid().as_raw() && meta.mode() & 0o022 == 0
}

/// Windows mode bits do not describe ACL ownership or write access.
#[cfg(not(unix))]
fn private(_: &fs::Metadata) -> bool {
    true
}

#[cfg(unix)]
fn read_preferences(
    file: &Path,
    meta: &fs::Metadata,
    message: &str,
) -> Result<Value, DocumentError> {
    use std::io::Read;
    use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
    let handle = fs::OpenOptions::new()
        .read(true)
        .custom_flags(rustix::fs::OFlags::NOFOLLOW.bits() as i32)
        .open(file)
        .map_err(DocumentError::Io)?;
    let opened = handle.metadata().map_err(DocumentError::Io)?;
    if (opened.dev(), opened.ino()) != (meta.dev(), meta.ino()) {
        return Err(invalid("preferences changed while opening"));
    }
    let mut bytes = Vec::new();
    handle
        .take(PREFERENCE_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(DocumentError::Io)?;
    if bytes.len() > PREFERENCE_BYTES {
        return Err(invalid(message));
    }
    parse(&bytes)
}

/// Stable Rust exposes no Windows file index, so the open-time identity check that
/// Node performs is not available; the bounded regular-file read still applies.
#[cfg(not(unix))]
fn read_preferences(file: &Path, _: &fs::Metadata, message: &str) -> Result<Value, DocumentError> {
    bounded(file, PREFERENCE_BYTES, message)
}

/// A command input document (`--data`, `--facts`): a bounded regular file, never a symlink.
pub fn input(file: &Path) -> Result<Value, DocumentError> {
    let message = "session input must be a bounded regular JSON file";
    if fs::symlink_metadata(file)
        .map_err(DocumentError::Io)?
        .file_type()
        .is_symlink()
    {
        return Err(invalid(message));
    }
    bounded(file, INPUT_BYTES, message)
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
                "craft-documents-{}-{}",
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
    fn absent_documents_have_defaults_and_present_ones_are_bounded() {
        let fixture = Fixture::new();
        assert_eq!(project(&fixture.0).unwrap(), json!({ "formatVersion": 1 }));
        assert_eq!(
            preferences(&fixture.0.join("missing/preferences.json")).unwrap(),
            json!({})
        );
        fs::create_dir(fixture.0.join(".craft")).unwrap();
        fs::write(
            fixture.0.join(".craft/project.json"),
            r#"{"formatVersion":1,"languages":["go"]}"#,
        )
        .unwrap();
        assert_eq!(project(&fixture.0).unwrap()["languages"], json!(["go"]));
        fs::write(
            fixture.0.join(".craft/project.json"),
            vec![b' '; PROJECT_BYTES + 1],
        )
        .unwrap();
        assert!(
            matches!(project(&fixture.0), Err(DocumentError::Invalid(m)) if m.contains("bounded"))
        );
        let data = fixture.0.join("data.json");
        assert!(
            matches!(input(&data), Err(error) if error.io_kind() == Some(io::ErrorKind::NotFound))
        );
        fs::write(&data, vec![b' '; INPUT_BYTES + 1]).unwrap();
        assert!(matches!(input(&data), Err(DocumentError::Invalid(_))));
        fs::write(&data, "{\"a\":1}").unwrap();
        assert_eq!(input(&data).unwrap(), json!({ "a": 1 }));
        assert!(matches!(input(&fixture.0), Err(DocumentError::Invalid(_))));
    }

    #[cfg(unix)]
    #[test]
    fn preferences_must_be_private_regular_and_unlinked() {
        use std::os::unix::fs::{PermissionsExt, symlink};
        let fixture = Fixture::new();
        fs::set_permissions(&fixture.0, fs::Permissions::from_mode(0o700)).unwrap();
        let file = fixture.0.join("preferences.json");
        fs::write(&file, r#"{"routing":{"editHints":false}}"#).unwrap();
        fs::set_permissions(&file, fs::Permissions::from_mode(0o600)).unwrap();
        assert_eq!(preferences(&file).unwrap()["routing"]["editHints"], false);
        let relative = std::env::current_dir()
            .unwrap()
            .join("craft-documents-relative.json");
        fs::write(&relative, "{}").unwrap();
        fs::set_permissions(&relative, fs::Permissions::from_mode(0o600)).unwrap();
        let bare = preferences(Path::new("craft-documents-relative.json"));
        fs::remove_file(&relative).unwrap();
        assert_eq!(bare.unwrap(), json!({}));
        fs::set_permissions(&file, fs::Permissions::from_mode(0o620)).unwrap();
        assert!(
            matches!(preferences(&file), Err(DocumentError::Invalid(m)) if m.contains("writable"))
        );
        fs::set_permissions(&file, fs::Permissions::from_mode(0o600)).unwrap();
        fs::set_permissions(&fixture.0, fs::Permissions::from_mode(0o777)).unwrap();
        assert!(
            matches!(preferences(&file), Err(DocumentError::Invalid(m)) if m.contains("writable"))
        );
        fs::set_permissions(&fixture.0, fs::Permissions::from_mode(0o700)).unwrap();
        let link = fixture.0.join("link.json");
        symlink(&file, &link).unwrap();
        assert!(
            matches!(preferences(&link), Err(DocumentError::Invalid(m)) if m.contains("symlink"))
        );
        fs::write(&file, vec![b' '; PREFERENCE_BYTES + 1]).unwrap();
        assert!(
            matches!(preferences(&file), Err(DocumentError::Invalid(m)) if m.contains("bounded"))
        );
        let folder = fixture.0.join("folder.json");
        fs::create_dir(&folder).unwrap();
        assert!(
            matches!(preferences(&folder), Err(DocumentError::Invalid(m)) if m.contains("bounded"))
        );
        let project = fixture.0.join("project");
        fs::create_dir(&project).unwrap();
        symlink(&fixture.0, project.join(".craft")).unwrap();
        assert!(
            matches!(super::project(&project), Err(DocumentError::Invalid(m)) if m.contains("symlink"))
        );
    }
}
