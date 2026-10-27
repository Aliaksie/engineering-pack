//! Workspace-local state records. Placement, bytes, the `.lock` protocol and legacy
//! migration receipts are a persisted contract that existing stores rely on; see
//! `tests/fixtures/runtime/state-contracts.json`.
use crate::Error;
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
/// Copy receipts for records taken over from earlier stores.
const MIGRATIONS: &str = "migrations";
/// Folders the layout before this one kept under `<worktree-id>/`, and their place now.
/// Its evidence database is not moved: it is rebuildable and may be held open.
const RELOCATED: &[(&str, &str)] = &[
    ("sessions", "sessions"),
    ("shared/telemetry", "telemetry"),
    ("shared/workspaces", "workspaces"),
    ("shared/migrations", MIGRATIONS),
];
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
    if directory(&base, false)? {
        relayout(&base, &worktree_id)?;
    }
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

/// Whether a folder entry is in use by a writer: a lock beside it, or one inside it.
fn held(path: &Path) -> bool {
    let mut lock = path.as_os_str().to_owned();
    lock.push(".lock");
    if fs::symlink_metadata(PathBuf::from(lock)).is_ok() {
        return true;
    }
    fs::read_dir(path).is_ok_and(|entries| {
        entries.flatten().any(|entry| {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            name.ends_with(".lock") || name.ends_with(".lease-guard")
        })
    })
}

/// Move what the layout before this one kept under `<worktree-id>/` into place, one entry at
/// a time: an entry held by a writer, or whose place is taken, stays for a later access.
/// Emptied folders are removed; nothing is copied or overwritten.
fn relayout(base: &Path, worktree_id: &str) -> Result<(), Error> {
    let old = base.join(worktree_id);
    for (from, to) in RELOCATED {
        let source = old.join(from);
        match fs::symlink_metadata(&source) {
            Ok(meta) if meta.is_dir() && !meta.file_type().is_symlink() => {}
            _ => continue,
        }
        let destination = descend(base, &[to], true)?;
        for entry in fs::read_dir(&source)? {
            let name = entry?.file_name();
            let path = source.join(&name);
            if name.to_string_lossy().ends_with(".lock") || held(&path) {
                continue;
            }
            let target = destination.join(&name);
            if fs::symlink_metadata(&target).is_err() {
                // A writer of the earlier layout may hold it on Windows; retried later.
                let _ = fs::rename(&path, &target);
            }
        }
        let _ = fs::remove_dir(&source);
    }
    let _ = fs::remove_dir(old.join("shared"));
    let _ = fs::remove_dir(&old);
    Ok(())
}

/// Where the layout before this one kept its shared files: `<worktree-id>/shared/`.
pub fn previous_shared(root: &Path) -> Result<PathBuf, Error> {
    let canonical = canonical_root(root)?;
    let worktree_id = short(text(&canonical)?.as_bytes());
    Ok(canonical
        .join(".craft")
        .join("state")
        .join(worktree_id)
        .join("shared"))
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
/// `<family>/<name>`. Records of earlier layouts are copied in on first access.
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
    let target = folder.join(name.rsplit('/').next().expect("record name"));
    migrate_legacy(&canonical, &place, &target, name, session)?;
    Ok(target)
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

/// Exclusive writer ticket: an existing `<record>.lock`, live or abandoned, refuses the
/// lock. A killed writer therefore blocks until its lock is resolved, never silently.
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

pub fn lock(file: &Path) -> Result<Option<Lock>, Error> {
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
    let ticket = acquire(path.clone(), cfg!(windows), |path| options.open(path))?;
    if ticket.is_none() && crate::lease::reclaim(file)? {
        acquire(path, cfg!(windows), |path| options.open(path))
    } else {
        Ok(ticket)
    }
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

fn git_directory(root: &Path) -> Result<Option<PathBuf>, Error> {
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
                let meta = fs::symlink_metadata(&target)?;
                if !meta.is_dir() || meta.file_type().is_symlink() {
                    return Err("invalid Git directory".into());
                }
                return Ok(Some(target));
            }
            Err(error) if error.kind() == ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
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

fn unsafe_legacy() -> Error {
    "unsafe or oversized legacy state".into()
}

fn busy_legacy() -> Error {
    Error::busy("legacy state writer is busy; stop older plugins before migration")
}

impl Inspection {
    fn visit(&mut self, file: &Path, relative: String, depth: usize) -> Result<(), Error> {
        let meta = fs::symlink_metadata(file)?;
        #[cfg(unix)]
        let linked = {
            use std::os::unix::fs::MetadataExt;
            meta.is_file() && meta.nlink() > 1
        };
        // Stable Rust exposes no link count on Windows, so hard-linked legacy files pass there.
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
            let mut children = fs::read_dir(file)?
                .map(|entry| {
                    let name = entry?.file_name();
                    name.into_string()
                        .map_err(|_| Error::from("non-UTF-8 state path unsupported"))
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
        let after = fs::symlink_metadata(file)?;
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
            serde_json::from_slice::<Value>(&data)?;
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

/// The name a record had before this layout and its companions as `(previous, current)`
/// relative paths. Records new to this layout, or not carried over (workspaces and the
/// rebuildable evidence database), have none.
fn previous(name: &str, session: Option<&str>) -> Option<(String, Vec<(String, String)>)> {
    let renamed = |old: String, new: &str| Some((old.clone(), vec![(old, new.to_string())]));
    match session {
        Some(session) => match name {
            "checkpoint.json" => renamed(format!("{}.json", short(session.as_bytes())), name),
            "task.json" => renamed(
                format!(
                    "active-{}.json",
                    &craft_engine::digest(&json!(session))[..24]
                ),
                name,
            ),
            _ if name.starts_with("activity-") => {
                renamed(name.replacen("activity-", "session-", 1), name)
            }
            _ if name.starts_with("control-") => {
                renamed(name.replacen("control-", "native-control-", 1), name)
            }
            _ if name.starts_with("workers-") => renamed(name.into(), name),
            _ if name.starts_with("routing-") => Some((
                name.into(),
                vec![
                    (name.into(), name.into()),
                    (format!("{name}.generation"), format!("{name}.generation")),
                ],
            )),
            _ => None,
        },
        None => match name {
            "telemetry/events.jsonl" => Some((
                "telemetry.jsonl".into(),
                vec![
                    ("telemetry.jsonl".into(), "events.jsonl".into()),
                    ("telemetry.jsonl.1".into(), "events.jsonl.1".into()),
                ],
            )),
            "telemetry/key.json" => renamed("telemetry-key.json".into(), "key.json"),
            "telemetry/captures.json" => renamed("telemetry-captures.json".into(), "captures.json"),
            _ => None,
        },
    }
}

fn exists(path: &Path) -> Result<bool, Error> {
    match fs::symlink_metadata(path) {
        Ok(_) => Ok(true),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error.into()),
    }
}

fn receipt_matches(receipt: &Value, fingerprint: &str) -> bool {
    receipt["formatVersion"] == 1 && receipt["source"] == fingerprint
}

/// Size and modification time of every present companion: an unchanged stamp proves an
/// unchanged source without reading it again on each access.
fn stamp(source: &Path, present: &[(String, String)]) -> Result<String, Error> {
    let mut list = Vec::new();
    for (old, _) in present {
        let meta = fs::symlink_metadata(source.join(old))?;
        let modified = meta
            .modified()
            .ok()
            .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
            .map_or(0, |elapsed| elapsed.as_nanos());
        list.push(json!([old, meta.len(), modified.to_string()]));
    }
    Ok(short(
        &serde_json::to_vec(&list).expect("JSON values serialize"),
    ))
}

// Earlier stores are read-only. Copy on first access and retain a source digest so an
// older plugin resuming writes cannot silently fork history. Consumers must stop old
// writers before upgrading; a legacy lock or changed source requires reconciliation.
// The newest earlier layout holding the record is its only source.
fn migrate_legacy(
    root: &Path,
    place: &Location,
    target: &Path,
    name: &str,
    session: Option<&str>,
) -> Result<(), Error> {
    let Some((old_name, pairs)) = previous(name, session) else {
        return Ok(());
    };
    let worktree = root.join(".craft").join("state").join(&place.worktree_id);
    // (directory, whether only the record's own locks block it)
    let mut sources = vec![(worktree.join(&place.session_id), true)];
    if let Some(git) = git_directory(root)? {
        sources.push((git.join("craft-state").join(&place.worktree_id), false));
    }
    sources.push((root.join(".craft").join("state"), false));
    sources.push((root.join(".capabilities").join("state"), false));
    let receipts = root.join(".craft").join("state").join(MIGRATIONS);
    for (source, scoped) in sources {
        // Validate every ancestor, including historical namespaces, before reading.
        let mut ancestor = source.as_path();
        while let Some(parent) = ancestor.parent() {
            match fs::symlink_metadata(ancestor) {
                Ok(meta) if !meta.is_dir() || meta.file_type().is_symlink() => {
                    return Err("legacy state path must be a directory without symlinks".into());
                }
                Err(error) if error.kind() != ErrorKind::NotFound => return Err(error.into()),
                _ => {}
            }
            ancestor = parent;
        }
        let mut present = Vec::new();
        for pair in &pairs {
            if exists(&source.join(&pair.0))? {
                present.push(pair.clone());
            }
        }
        let old = source.join(&old_name);
        let receipt = receipts.join(format!("migration-{}.json", short(text(&old)?.as_bytes())));
        if present.is_empty() {
            // Cleanup removes the previous layout's copied sources; older stores they were
            // copied from must not become the record's source again.
            if scoped && exists(&receipt)? {
                return Ok(());
            }
            continue;
        }
        let stamp = stamp(&source, &present)?;
        let previous = read(&receipt, RECEIPT_BYTES)?;
        // Logs and caches are replaceable: once copied, the copy is authoritative and an
        // older writer still appending to the source is ignored, not a fork to refuse.
        let replaceable = name.starts_with("telemetry/events") || name.starts_with("routing-");
        if previous
            .as_ref()
            .is_some_and(|r| r["stamp"] == stamp.as_str() || replaceable && r["formatVersion"] == 1)
        {
            return Ok(());
        }
        // The earlier session layout holds other records' live locks; only this record's
        // count. Flat stores use per-record locks; any unresolved writer blocks migration.
        if scoped {
            for (old, _) in &present {
                if exists(&source.join(format!("{old}.lock")))? {
                    return Err(busy_legacy());
                }
            }
        } else {
            for entry in fs::read_dir(&source)? {
                if entry?.file_name().to_string_lossy().ends_with(".lock") {
                    return Err(busy_legacy());
                }
            }
        }
        let mut inspection = Inspection {
            entries: Vec::new(),
            bytes: 0,
        };
        for (old, new) in &present {
            inspection.visit(&source.join(old), new.clone(), 0)?;
        }
        let fingerprint = inspection.fingerprint();
        let published = json!({"formatVersion": 1, "source": fingerprint, "stamp": stamp});
        if let Some(previous) = previous {
            if !receipt_matches(&previous, &fingerprint) {
                return Err(
                    "legacy state changed after migration; stop older plugins and reconcile stores"
                        .into(),
                );
            }
            // Same content under a new stamp (a copy or touch): remember it.
            return write(&receipt, &published, RECEIPT_BYTES, false);
        }
        // Create the destination and receipt parents only when there is earlier data.
        let destination = target.parent().ok_or("state record parent required")?;
        if let Some(session) = session
            && !directory(&place.directory, false)?
        {
            let sessions = place.directory.parent().ok_or("state session parent")?;
            location_at(root, None, true)?;
            directory(sessions, true)?;
            directory_and_manifest(&place.directory, session)?;
        }
        for folder in [destination, receipts.as_path()] {
            let relative = folder
                .strip_prefix(root)
                .map_err(|_| "state destination outside root")?;
            let parts = relative
                .iter()
                .map(|part| part.to_str().ok_or("non-UTF-8 state path unsupported"))
                .collect::<Result<Vec<_>, _>>()?;
            descend(root, &parts, true)?;
        }
        let _ticket = lock(&receipt)?.ok_or_else(|| Error::busy("state migration is busy"))?;
        if let Some(current) = read(&receipt, RECEIPT_BYTES)? {
            if current["source"] != fingerprint.as_str() {
                return Err("legacy migration source conflict".into());
            }
            return Ok(());
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
                            let result = create_private(&staged, data)
                                .and_then(|()| fs::hard_link(&staged, &file).map_err(Error::from));
                            let _ = fs::remove_file(&staged);
                            result?;
                        }
                        Err(error) => return Err(error.into()),
                    }
                }
            }
        }
        return write(&receipt, &published, RECEIPT_BYTES, false);
    }
    Ok(())
}

/// Shared records an earlier layout kept as files directly in `shared/`.
const SHARED_RECORDS: &[&str] = &[
    "telemetry/events.jsonl",
    "telemetry/key.json",
    "telemetry/captures.json",
];

/// Names of the previous layout's `shared/` files holding the same record as `old`, once
/// the record was copied, the source is unchanged since (a log may have grown) and no
/// writer holds it: their removal loses nothing. Nothing is copied or written here.
/// `None` when `old` held no shared record.
pub fn retirable(root: &Path, old: &str) -> Result<Option<Vec<String>>, Error> {
    let Some((name, old_name, pairs)) = SHARED_RECORDS.iter().find_map(|name| {
        previous(name, None)
            .filter(|(_, pairs)| pairs.iter().any(|(previous, _)| previous == old))
            .map(|(old_name, pairs)| (*name, old_name, pairs))
    }) else {
        return Ok(None);
    };
    let source = previous_shared(root)?;
    let mut present = Vec::new();
    for pair in pairs {
        if exists(&source.join(&pair.0))? {
            if exists(&source.join(format!("{}.lock", pair.0)))? {
                return Err(busy_legacy());
            }
            present.push(pair);
        }
    }
    let receipt = location(root, None)?
        .directory
        .join(MIGRATIONS)
        .join(format!(
            "migration-{}.json",
            short(text(&source.join(&old_name))?.as_bytes())
        ));
    let receipt =
        read(&receipt, RECEIPT_BYTES)?.ok_or("not copied yet; it is copied when first used")?;
    let replaceable = name.starts_with("telemetry/events");
    if !present.is_empty() && !replaceable && receipt["stamp"] != stamp(&source, &present)?.as_str()
    {
        return Err("legacy state changed after migration".into());
    }
    Ok(Some(present.into_iter().map(|(old, _)| old).collect()))
}

pub mod sweep;

#[cfg(test)]
mod tests;
