//! Workspace-local state records. Placement, bytes and the `.lock` protocol are a persisted
//! contract that existing stores rely on; see `tests/fixtures/runtime/state-contracts.json`.
use crate::Error;
use crate::files::{ReadError, read_bounded};
use craft_engine::hash;
use serde_json::{Value, json};
use std::{
    fs::{self, File, OpenOptions},
    io::{ErrorKind, Write},
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

#[derive(Debug, Clone, PartialEq)]
pub struct Location {
    pub directory: PathBuf,
    pub worktree_id: String,
    pub session_id: String,
}

fn short(bytes: &[u8]) -> String {
    hash(bytes)[..24].to_string()
}

fn text(path: &Path) -> Result<&str, Error> {
    path.to_str()
        .ok_or_else(|| "non-UTF-8 state path unsupported".into())
}

/// The physical path in the form Node's `fs.realpathSync.native` returns. Windows verbatim
/// prefixes are removed.
pub fn physical(path: &Path) -> std::io::Result<PathBuf> {
    let canonical = path.canonicalize()?;
    let Some(value) = canonical.to_str() else {
        return Ok(canonical);
    };
    Ok(match value.strip_prefix(r"\\?\") {
        Some(rest) => match rest.strip_prefix(r"UNC\") {
            Some(share) => PathBuf::from(format!(r"\\{share}")),
            None => PathBuf::from(rest),
        },
        None => canonical,
    })
}

/// The physical worktree path, so both runtimes hash the same identity.
pub fn canonical_root(root: &Path) -> Result<PathBuf, Error> {
    let physical = physical(root)?;
    text(&physical)?;
    Ok(physical)
}

/// An existing component must be a real directory; a missing one is created only on request.
fn directory(path: &Path, create: bool) -> Result<bool, Error> {
    match fs::symlink_metadata(path) {
        Ok(meta) if meta.is_dir() && !meta.file_type().is_symlink() => Ok(true),
        Ok(_) => Err("state path must be a directory without symlinks".into()),
        Err(error) if error.kind() == ErrorKind::NotFound => {
            if !create {
                return Ok(false);
            }
            crate::files::create_private_directory(path)?;
            directory(path, false)
        }
        Err(error) => Err(error.into()),
    }
}

fn descend(root: &Path, parts: &[&str], create: bool) -> Result<PathBuf, Error> {
    let mut folder = root.to_path_buf();
    for part in parts {
        folder.push(part);
        directory(&folder, create)?;
    }
    Ok(folder)
}

/// Shared record families, one directory each beside `sessions/`.
const FAMILIES: &[&str] = &["telemetry", "intelligence", "workspaces"];
/// The per-session description, written once when the session directory is created.
pub const MANIFEST: &str = "manifest.json";
pub const MANIFEST_BYTES: usize = 1024;

pub fn location(root: &Path, session: Option<&str>) -> Result<Location, Error> {
    let canonical = canonical_root(root)?;
    location_at(&canonical, session, false)
}

fn location_at(canonical: &Path, session: Option<&str>, create: bool) -> Result<Location, Error> {
    // Node bounds the session in UTF-16 code units.
    if session.is_some_and(|s| s.is_empty() || s.encode_utf16().count() > 512) {
        return Err("invalid state session".into());
    }
    let worktree_id = short(text(canonical)?.as_bytes());
    let base = descend(canonical, &[".craft", "state"], create)?;
    let (directory, session_id) = match session {
        None => (base, "shared".to_string()),
        Some(session) => {
            let id = short(session.as_bytes());
            let sessions = descend(&base, &["sessions"], create)?;
            let directory = sessions.join(&id);
            if !self::directory(&directory, false)? && create {
                directory_and_manifest(&directory, session)?;
            }
            (directory, id)
        }
    };
    Ok(Location {
        directory,
        worktree_id,
        session_id,
    })
}

/// Create a session directory and describe it: creation time, the workspace identity of
/// the session and the host when a hook created it. Never the raw session, task or paths.
fn directory_and_manifest(directory: &Path, session: &str) -> Result<(), Error> {
    self::directory(directory, true)?;
    let created = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_millis() as u64);
    let mut manifest = json!({
        "formatVersion": 1,
        "created": created,
        "workspace": craft_engine::digest(&json!(session)),
    });
    let host = HOST.with(|host| host.borrow().clone()).filter(|host| {
        (1..=32).contains(&host.len())
            && host
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
    });
    if let Some(host) = host {
        manifest["host"] = json!(host);
    }
    // The first creator describes the directory; a concurrent one keeps that description.
    match create_private(&directory.join(MANIFEST), &serialize(&manifest, false)) {
        Err(error) if error.kind() == Some(ErrorKind::AlreadyExists) => Ok(()),
        result => result,
    }
}

thread_local! {
    static HOST: std::cell::RefCell<Option<String>> = const { std::cell::RefCell::new(None) };
}

/// While held, session directories this thread creates name `host` in their manifest.
pub struct Describing(Option<String>);

impl Drop for Describing {
    fn drop(&mut self) {
        HOST.with(|host| *host.borrow_mut() = self.0.take());
    }
}

/// Name the host of the hook running on this thread; nothing is written until a record is.
pub fn describing(host: &str) -> Describing {
    Describing(HOST.with(|current| current.borrow_mut().replace(host.to_string())))
}

/// Directory of one shared family, for records that are not single JSON files.
pub fn shared(root: &Path, family: &str, create: bool) -> Result<PathBuf, Error> {
    if !FAMILIES.contains(&family) {
        return Err("invalid state family".into());
    }
    let canonical = canonical_root(root)?;
    descend(
        &location_at(&canonical, None, create)?.directory,
        &[family],
        create,
    )
}

/// Work plans, one folder each beside `sessions/`.
pub const PLANS: &str = "plans";

/// The plans folder, or the folder of the plan named `plan`.
pub fn plans(root: &Path, plan: Option<&str>, create: bool) -> Result<PathBuf, Error> {
    if plan.is_some_and(|id| !craft_engine::plan::slug(id)) {
        return Err("invalid plan id".into());
    }
    let canonical = canonical_root(root)?;
    let base = location_at(&canonical, None, create)?.directory;
    let folder = descend(&base, &[PLANS], create)?;
    match plan {
        Some(id) => descend(&folder, &[id], create),
        None => Ok(folder),
    }
}

fn segment(name: &str) -> bool {
    !(name.is_empty()
        || name == "."
        || name == ".."
        || !name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'.' || b == b'-'))
}

/// Path of one state record: `sessions/<session>/<name>` for a session, otherwise
/// `<family>/<name>`.
pub fn file(
    root: &Path,
    name: &str,
    create: bool,
    session: Option<&str>,
) -> Result<PathBuf, Error> {
    let family = match (session, name.split_once('/')) {
        (Some(_), None) if segment(name) => None,
        (None, Some((family, file))) if FAMILIES.contains(&family) && segment(file) => Some(family),
        _ => return Err("invalid state filename".into()),
    };
    let canonical = canonical_root(root)?;
    let place = location_at(&canonical, session, create)?;
    let folder = match family {
        Some(family) => descend(&place.directory, &[family], create)?,
        None => place.directory.clone(),
    };
    Ok(folder.join(name.rsplit('/').next().expect("record name")))
}

pub fn read(file: &Path, max_bytes: usize) -> Result<Option<Value>, Error> {
    match read_bounded(file, max_bytes) {
        Ok(bytes) => serde_json::from_slice(&bytes)
            .map(Some)
            .map_err(Error::from),
        Err(ReadError::Io(error)) if error.kind() == ErrorKind::NotFound => Ok(None),
        Err(ReadError::Io(error)) => Err(error.into()),
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

fn create_private(path: &Path, bytes: &[u8]) -> Result<(), Error> {
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
        .map_err(Error::from)
}

#[cfg(test)]
thread_local! {
    pub(crate) static FAIL_PUBLISH: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// Atomically replace a record. The existing record must itself be a valid bounded
/// record, so a corrupt or foreign file is never silently overwritten.
pub fn write(file: &Path, value: &Value, max_bytes: usize, pretty: bool) -> Result<(), Error> {
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
        fs::rename(&staged, file).map_err(Error::from)
    });
    let _ = fs::remove_file(&staged);
    result
}

/// Atomically replace a generated text view that nothing reads back.
pub fn write_view(file: &Path, text: &str, max_bytes: usize) -> Result<(), Error> {
    if text.len() > max_bytes {
        return Err("view exceeds size limit".into());
    }
    let staged = temporary(file);
    let result = create_private(&staged, text.as_bytes())
        .and_then(|()| fs::rename(&staged, file).map_err(Error::from));
    let _ = fs::remove_file(&staged);
    result
}

/// Exclusive writer ticket on `<record>.lock`, removed on drop unless retained.
#[derive(Debug)]
pub struct Lock {
    path: PathBuf,
    file: Option<File>,
    retained: bool,
}

impl Lock {
    /// Record the holder in the lock file, for protocols that recover abandoned locks.
    pub fn record(&mut self, bytes: &[u8]) -> std::io::Result<()> {
        self.file.as_mut().expect("held lock").write_all(bytes)
    }

    /// Transfer cleanup to the native lease protocol across process invocations.
    pub(crate) fn retain(mut self) {
        self.retained = true;
    }
}

impl Drop for Lock {
    fn drop(&mut self) {
        drop(self.file.take());
        if !self.retained {
            let _ = fs::remove_file(&self.path);
        }
    }
}

/// The record's writer lock. An existing `<record>.lock` refuses it unless it is a native
/// lease whose host died; any other holder, live or abandoned, blocks until resolved.
pub fn lock(file: &Path) -> Result<Option<Lock>, Error> {
    match ticket(file)? {
        None if crate::lease::reclaim(file)? => ticket(file),
        ticket => Ok(ticket),
    }
}

/// The lock primitive: create `<record>.lock` exclusively, never recovering an existing
/// one. Recovery protocols (leases, workspace locks) build on it, not on `lock`.
pub(crate) fn ticket(file: &Path) -> Result<Option<Lock>, Error> {
    let mut name = file.as_os_str().to_owned();
    name.push(".lock");
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    acquire(PathBuf::from(name), cfg!(windows), |path| {
        options.open(path)
    })
}

const LOCK_ATTEMPTS: usize = 3;

fn acquire(
    path: PathBuf,
    windows: bool,
    mut open: impl FnMut(&Path) -> std::io::Result<File>,
) -> Result<Option<Lock>, Error> {
    let mut attempt = 1;
    loop {
        match open(&path) {
            Ok(handle) => {
                return Ok(Some(Lock {
                    path,
                    file: Some(handle),
                    retained: false,
                }));
            }
            Err(error) if error.kind() == ErrorKind::AlreadyExists => return Ok(None),
            // Windows denies creating a lock whose previous holder is still deleting it;
            // that lock is busy. A lock gone by the listing finished deleting in between,
            // so the create is retried; a denial that persists without a lock is real.
            Err(error) if windows && error.kind() == ErrorKind::PermissionDenied => {
                let parent = path.parent().ok_or("state lock parent required")?;
                let name = path.file_name().ok_or("state lock name required")?;
                let listed = fs::read_dir(parent)?
                    .any(|entry| entry.is_ok_and(|entry| entry.file_name() == name));
                if listed {
                    return Ok(None);
                }
                if attempt == LOCK_ATTEMPTS {
                    return Err(error.into());
                }
                attempt += 1;
            }
            Err(error) => return Err(error.into()),
        }
    }
}

pub mod sweep;

#[cfg(test)]
mod tests;
