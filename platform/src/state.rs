//! Workspace-local state records shared with `platform/node/state-files.mjs` until
//! the last JS reader or writer retires. Placement, bytes, the `.lock` protocol and
//! legacy migration receipts are the compatibility contract; both sides must pass
//! `tests/fixtures/runtime/state-contracts.json`.
use crate::files::{ReadError, read_bounded};
use craft_engine::hash;
use serde_json::{Value, json};
use std::{
    fs::{self, File, OpenOptions},
    io::{ErrorKind, Write},
    path::{Component, MAIN_SEPARATOR, Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

const LEGACY_FILE_BYTES: u64 = 4 * 1024 * 1024;
const LEGACY_TOTAL_BYTES: u64 = 16 * 1024 * 1024;
const LEGACY_ENTRIES: usize = 4096;
const RECEIPT_BYTES: usize = 4096;

#[derive(Debug, Clone, PartialEq)]
pub struct Location {
    pub directory: PathBuf,
    pub worktree_id: String,
    pub session_id: String,
}

fn short(bytes: &[u8]) -> String {
    hash(bytes)[..24].to_string()
}

fn text(path: &Path) -> Result<&str, String> {
    path.to_str()
        .ok_or_else(|| "non-UTF-8 state path unsupported".into())
}

/// The physical worktree path in the form Node's `fs.realpathSync.native` returns,
/// so both runtimes hash the same identity. Windows verbatim prefixes are removed.
pub fn canonical_root(root: &Path) -> Result<PathBuf, String> {
    let canonical = root.canonicalize().map_err(|e| e.to_string())?;
    let value = text(&canonical)?;
    Ok(match value.strip_prefix(r"\\?\") {
        Some(rest) => match rest.strip_prefix(r"UNC\") {
            Some(share) => PathBuf::from(format!(r"\\{share}")),
            None => PathBuf::from(rest),
        },
        None => canonical,
    })
}

/// An existing component must be a real directory; a missing one is created only on request.
fn directory(path: &Path, create: bool) -> Result<bool, String> {
    match fs::symlink_metadata(path) {
        Ok(meta) if meta.is_dir() && !meta.file_type().is_symlink() => Ok(true),
        Ok(_) => Err("state path must be a directory without symlinks".into()),
        Err(error) if error.kind() == ErrorKind::NotFound => {
            if !create {
                return Ok(false);
            }
            let mut builder = fs::DirBuilder::new();
            #[cfg(unix)]
            {
                use std::os::unix::fs::DirBuilderExt;
                builder.mode(0o700);
            }
            match builder.create(path) {
                Ok(()) => {}
                Err(error) if error.kind() == ErrorKind::AlreadyExists => {}
                Err(error) => return Err(error.to_string()),
            }
            directory(path, false)
        }
        Err(error) => Err(error.to_string()),
    }
}

fn descend(root: &Path, parts: &[&str], create: bool) -> Result<PathBuf, String> {
    let mut folder = root.to_path_buf();
    for part in parts {
        folder.push(part);
        directory(&folder, create)?;
    }
    Ok(folder)
}

pub fn location(root: &Path, session: Option<&str>) -> Result<Location, String> {
    let canonical = canonical_root(root)?;
    location_at(&canonical, session, false)
}

fn location_at(canonical: &Path, session: Option<&str>, create: bool) -> Result<Location, String> {
    // Node bounds the session in UTF-16 code units.
    if session.is_some_and(|s| s.is_empty() || s.encode_utf16().count() > 512) {
        return Err("invalid state session".into());
    }
    let worktree_id = short(text(canonical)?.as_bytes());
    let session_id = session.map_or_else(|| "shared".into(), |s| short(s.as_bytes()));
    let directory = descend(
        canonical,
        &[".craft", "state", &worktree_id, &session_id],
        create,
    )?;
    Ok(Location {
        directory,
        worktree_id,
        session_id,
    })
}

/// Path of one state record. Legacy records for `name` are copied in on first access.
pub fn file(
    root: &Path,
    name: &str,
    create: bool,
    session: Option<&str>,
) -> Result<PathBuf, String> {
    if name.is_empty()
        || name == "."
        || name == ".."
        || !name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'.' || b == b'-')
    {
        return Err("invalid state filename".into());
    }
    let canonical = canonical_root(root)?;
    let target = location_at(&canonical, session, create)?
        .directory
        .join(name);
    migrate_legacy(&canonical, &target, name)?;
    Ok(target)
}

pub fn read(file: &Path, max_bytes: usize) -> Result<Option<Value>, String> {
    match read_bounded(file, max_bytes) {
        Ok(bytes) => serde_json::from_slice(&bytes)
            .map(Some)
            .map_err(|e| e.to_string()),
        Err(ReadError::Io(error)) if error.kind() == ErrorKind::NotFound => Ok(None),
        Err(ReadError::Io(error)) => Err(error.to_string()),
        Err(_) => Err("invalid bounded state file".into()),
    }
}

/// Serialize as `JSON.stringify(value, null, pretty ? 2 : undefined)` plus a newline.
pub fn serialize(value: &Value, pretty: bool) -> Vec<u8> {
    let mut text = if pretty {
        serde_json::to_vec_pretty(value)
    } else {
        serde_json::to_vec(value)
    }
    .expect("JSON values serialize");
    text.push(b'\n');
    text
}

fn temporary(file: &Path) -> PathBuf {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let mut name = file.as_os_str().to_owned();
    name.push(format!(
        ".{}-{nanos}-{}.tmp",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    PathBuf::from(name)
}

fn create_private(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options
        .open(path)
        .and_then(|mut file| file.write_all(bytes))
        .map_err(|e| e.to_string())
}

#[cfg(test)]
thread_local! {
    pub(crate) static FAIL_PUBLISH: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// Atomically replace a record. The existing record must itself be a valid bounded
/// record, so a corrupt or foreign file is never silently overwritten.
pub fn write(file: &Path, value: &Value, max_bytes: usize, pretty: bool) -> Result<(), String> {
    let bytes = serialize(value, pretty);
    if bytes.len() > max_bytes {
        return Err("state exceeds size limit".into());
    }
    read(file, max_bytes)?;
    let staged = temporary(file);
    let result = create_private(&staged, &bytes).and_then(|()| {
        #[cfg(test)]
        if FAIL_PUBLISH.with(|fail| fail.get()) {
            return Err("injected publication failure".into());
        }
        fs::rename(&staged, file).map_err(|e| e.to_string())
    });
    let _ = fs::remove_file(&staged);
    result
}

/// Exclusive writer ticket: an existing `<record>.lock`, live or abandoned, refuses the
/// lock. A killed writer therefore blocks until its lock is resolved, never silently.
#[derive(Debug)]
pub struct Lock {
    path: PathBuf,
    file: Option<File>,
}

impl Drop for Lock {
    fn drop(&mut self) {
        drop(self.file.take());
        let _ = fs::remove_file(&self.path);
    }
}

pub fn lock(file: &Path) -> Result<Option<Lock>, String> {
    let mut name = file.as_os_str().to_owned();
    name.push(".lock");
    let path = PathBuf::from(name);
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    acquire(path, cfg!(windows), |path| options.open(path))
}

const LOCK_ATTEMPTS: usize = 3;

fn acquire(
    path: PathBuf,
    windows: bool,
    mut open: impl FnMut(&Path) -> std::io::Result<File>,
) -> Result<Option<Lock>, String> {
    let mut attempt = 1;
    loop {
        match open(&path) {
            Ok(handle) => {
                return Ok(Some(Lock {
                    path,
                    file: Some(handle),
                }));
            }
            Err(error) if error.kind() == ErrorKind::AlreadyExists => return Ok(None),
            // Windows denies creating a lock whose previous holder is still deleting it;
            // that lock is busy. A lock gone by the listing finished deleting in between,
            // so the create is retried; a denial that persists without a lock is real.
            Err(error) if windows && error.kind() == ErrorKind::PermissionDenied => {
                let parent = path.parent().ok_or("state lock parent required")?;
                let name = path.file_name().ok_or("state lock name required")?;
                let listed = fs::read_dir(parent)
                    .map_err(|e| e.to_string())?
                    .any(|entry| entry.is_ok_and(|entry| entry.file_name() == name));
                if listed {
                    return Ok(None);
                }
                if attempt == LOCK_ATTEMPTS {
                    return Err(error.to_string());
                }
                attempt += 1;
            }
            Err(error) => return Err(error.to_string()),
        }
    }
}

/// `path.resolve` semantics for a Git directory marker target.
fn lexical(base: &Path, target: &str) -> PathBuf {
    let mut result = PathBuf::new();
    for component in base.join(target).components() {
        match component {
            Component::ParentDir => {
                result.pop();
            }
            Component::CurDir => {}
            other => result.push(other),
        }
    }
    result
}

fn git_directory(root: &Path) -> Result<Option<PathBuf>, String> {
    let mut current = root;
    for _ in 0..64 {
        let marker = current.join(".git");
        match fs::symlink_metadata(&marker) {
            Ok(meta) if meta.file_type().is_symlink() => {
                return Err("Git metadata may not be a symlink".into());
            }
            Ok(meta) if meta.is_dir() => return Ok(Some(marker)),
            Ok(_) => {
                let bytes = read_bounded(&marker, 4096)
                    .map_err(|_| "invalid Git directory marker".to_string())?;
                let content = std::str::from_utf8(&bytes)
                    .map_err(|_| "invalid Git directory marker".to_string())?;
                // `^gitdir: ([^\r\n]+)\r?\n?$`
                let line = content.strip_suffix('\n').unwrap_or(content);
                let line = line.strip_suffix('\r').unwrap_or(line);
                let target = line
                    .strip_prefix("gitdir: ")
                    .filter(|t| !t.is_empty() && !t.contains(['\r', '\n']))
                    .ok_or("invalid Git directory marker")?;
                let target = lexical(current, target);
                let meta = fs::symlink_metadata(&target).map_err(|e| e.to_string())?;
                if !meta.is_dir() || meta.file_type().is_symlink() {
                    return Err("invalid Git directory".into());
                }
                return Ok(Some(target));
            }
            Err(error) if error.kind() == ErrorKind::NotFound => {}
            Err(error) => return Err(error.to_string()),
        }
        match current.parent() {
            Some(parent) => current = parent,
            None => return Ok(None),
        }
    }
    Err("state root exceeds directory search limit".into())
}

enum Entry {
    Directory(String),
    File(String, Vec<u8>, String),
}

/// Snapshot of one legacy record family, rejecting anything a writer could still change.
struct Inspection {
    entries: Vec<Entry>,
    bytes: u64,
}

fn unsafe_legacy() -> String {
    "unsafe or oversized legacy state".into()
}

fn busy_legacy() -> String {
    "legacy state writer is busy; stop older plugins before migration".into()
}

impl Inspection {
    fn visit(&mut self, file: &Path, relative: String, depth: usize) -> Result<(), String> {
        let meta = fs::symlink_metadata(file).map_err(|e| e.to_string())?;
        #[cfg(unix)]
        let linked = {
            use std::os::unix::fs::MetadataExt;
            meta.is_file() && meta.nlink() > 1
        };
        // Stable Rust exposes no link count on Windows; Node's check remains there until JS retires.
        #[cfg(not(unix))]
        let linked = false;
        if depth > 16
            || self.entries.len() >= LEGACY_ENTRIES
            || meta.file_type().is_symlink()
            || linked
        {
            return Err(unsafe_legacy());
        }
        if text(file)?.ends_with(".lock") {
            return Err(busy_legacy());
        }
        if meta.is_dir() {
            self.entries.push(Entry::Directory(relative.clone()));
            let mut children = fs::read_dir(file)
                .map_err(|e| e.to_string())?
                .map(|entry| {
                    let name = entry.map_err(|e| e.to_string())?.file_name();
                    name.into_string()
                        .map_err(|_| "non-UTF-8 state path unsupported".to_string())
                })
                .collect::<Result<Vec<_>, _>>()?;
            // Array.prototype.sort order: UTF-16 code units.
            children.sort_by(|a, b| a.encode_utf16().cmp(b.encode_utf16()));
            for child in children {
                self.visit(
                    &file.join(&child),
                    format!("{relative}{MAIN_SEPARATOR}{child}"),
                    depth + 1,
                )?;
            }
            return Ok(());
        }
        self.bytes += meta.len();
        if !meta.is_file() || meta.len() > LEGACY_FILE_BYTES || self.bytes > LEGACY_TOTAL_BYTES {
            return Err(unsafe_legacy());
        }
        let data = read_bounded(file, LEGACY_FILE_BYTES as usize).map_err(|_| unsafe_legacy())?;
        let after = fs::symlink_metadata(file).map_err(|e| e.to_string())?;
        #[cfg(unix)]
        let same_file = {
            use std::os::unix::fs::MetadataExt;
            after.ino() == meta.ino()
        };
        #[cfg(not(unix))]
        let same_file = true;
        if data.len() as u64 != meta.len()
            || !same_file
            || after.len() != meta.len()
            || after.modified().ok() != meta.modified().ok()
        {
            return Err("legacy state changed during migration".into());
        }
        if depth == 0 && relative.ends_with(".json") {
            serde_json::from_slice::<Value>(&data).map_err(|e| e.to_string())?;
        }
        let digest = short(&data);
        self.entries.push(Entry::File(relative, data, digest));
        Ok(())
    }

    /// Same digest as the JS fingerprint over `{relative, directory?, digest?}` objects.
    fn fingerprint(&self) -> String {
        let list: Vec<Value> = self
            .entries
            .iter()
            .map(|entry| match entry {
                Entry::Directory(relative) => json!({"relative": relative, "directory": true}),
                Entry::File(relative, _, digest) => json!({"relative": relative, "digest": digest}),
            })
            .collect();
        short(&serde_json::to_vec(&list).expect("JSON values serialize"))
    }
}

fn companions(name: &str) -> Vec<String> {
    match name {
        "workspace-guard.json" => vec![name.into(), "sessions".into()],
        "telemetry.jsonl" => vec![name.into(), format!("{name}.1")],
        _ if name.starts_with("routing-") => vec![name.into(), format!("{name}.generation")],
        _ => vec![name.into()],
    }
}

fn exists(path: &Path) -> Result<bool, String> {
    match fs::symlink_metadata(path) {
        Ok(_) => Ok(true),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error.to_string()),
    }
}

fn receipt_matches(receipt: &Value, fingerprint: &str) -> bool {
    receipt["formatVersion"] == 1 && receipt["source"] == fingerprint
}

// Legacy stores are read-only. Copy on first access and retain a source digest so an
// older plugin resuming writes cannot silently fork history. Consumers must stop old
// writers before upgrading; a legacy lock or changed source requires reconciliation.
fn migrate_legacy(root: &Path, target: &Path, name: &str) -> Result<(), String> {
    let mut sources = Vec::new();
    if let Some(git) = git_directory(root)? {
        sources.push(git.join("craft-state").join(short(text(root)?.as_bytes())));
    }
    sources.push(root.join(".craft").join("state"));
    sources.push(root.join(".capabilities").join("state"));
    for source in sources {
        // Validate every ancestor, including historical namespaces, before reading.
        let mut ancestor = source.as_path();
        while let Some(parent) = ancestor.parent() {
            match fs::symlink_metadata(ancestor) {
                Ok(meta) if !meta.is_dir() || meta.file_type().is_symlink() => {
                    return Err("legacy state path must be a directory without symlinks".into());
                }
                Err(error) if error.kind() != ErrorKind::NotFound => return Err(error.to_string()),
                _ => {}
            }
            ancestor = parent;
        }
        let present = companions(name)
            .into_iter()
            .filter_map(|item| match exists(&source.join(&item)) {
                Ok(true) => Some(Ok(item)),
                Ok(false) => None,
                Err(error) => Some(Err(error)),
            })
            .collect::<Result<Vec<_>, _>>()?;
        if present.is_empty() {
            continue;
        }
        // Flat legacy stores use per-record locks; any live/unresolved writer blocks migration.
        for entry in fs::read_dir(&source).map_err(|e| e.to_string())? {
            if entry
                .map_err(|e| e.to_string())?
                .file_name()
                .to_string_lossy()
                .ends_with(".lock")
            {
                return Err(busy_legacy());
            }
        }
        let mut inspection = Inspection {
            entries: Vec::new(),
            bytes: 0,
        };
        for item in present {
            inspection.visit(&source.join(&item), item, 0)?;
        }
        let fingerprint = inspection.fingerprint();
        let destination = target.parent().ok_or("state record parent required")?;
        let old = source.join(name);
        let receipt = destination.join(format!("migration-{}.json", short(text(&old)?.as_bytes())));
        if let Some(previous) = read(&receipt, RECEIPT_BYTES)? {
            if !receipt_matches(&previous, &fingerprint) {
                return Err(
                    "legacy state changed after migration; stop older plugins and reconcile stores"
                        .into(),
                );
            }
            continue;
        }
        // Create missing destination parents only when there is actual legacy data.
        let relative = destination
            .strip_prefix(root)
            .map_err(|_| "state destination outside root")?;
        let parts = relative
            .iter()
            .map(|part| part.to_str().ok_or("non-UTF-8 state path unsupported"))
            .collect::<Result<Vec<_>, _>>()?;
        descend(root, &parts, true)?;
        let _ticket = lock(&receipt)?.ok_or("state migration is busy")?;
        if let Some(current) = read(&receipt, RECEIPT_BYTES)? {
            if current["source"] != fingerprint.as_str() {
                return Err("legacy migration source conflict".into());
            }
            continue;
        }
        // Identical files from an interrupted copy are reusable; differing destinations
        // are never overwritten. Publish the receipt only after the complete copy.
        for entry in &inspection.entries {
            match entry {
                Entry::Directory(relative) => {
                    directory(&destination.join(relative), true)?;
                }
                Entry::File(relative, data, digest) => {
                    let file = destination.join(relative);
                    match fs::symlink_metadata(&file) {
                        Ok(meta) => {
                            let same = meta.is_file()
                                && !meta.file_type().is_symlink()
                                && meta.len() == data.len() as u64
                                && read_bounded(&file, data.len())
                                    .is_ok_and(|bytes| &short(&bytes) == digest);
                            if !same {
                                return Err("legacy state destination conflict".into());
                            }
                        }
                        Err(error) if error.kind() == ErrorKind::NotFound => {
                            let staged = temporary(&file);
                            // Exclusive atomic publication: a crash cannot leave a partial destination.
                            let result = create_private(&staged, data).and_then(|()| {
                                fs::hard_link(&staged, &file).map_err(|e| e.to_string())
                            });
                            let _ = fs::remove_file(&staged);
                            result?;
                        }
                        Err(error) => return Err(error.to_string()),
                    }
                }
            }
        }
        write(
            &receipt,
            &json!({"formatVersion": 1, "source": fingerprint}),
            RECEIPT_BYTES,
            false,
        )?;
    }
    Ok(())
}

#[cfg(test)]
mod tests;
