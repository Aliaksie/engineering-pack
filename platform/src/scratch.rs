//! Filesystem side of private task workspaces, shared with `platform/node/workspace.mjs`
//! until its last JS caller retires: bounded inspection, verified removal, exclusive
//! writes, owner liveness and the recoverable workspace lock.
use crate::files::{ReadError, read_bounded};
use crate::state::{self, Lock};
use craft_engine::scratch::Liveness;
use serde_json::Value;
use std::fs::{self, OpenOptions};
use std::io::{self, ErrorKind, Read, Write};
use std::path::Path;

const INSPECTION_DEPTH: usize = 16;
const INSPECTION_ENTRIES: usize = 4096;
const LOCK_OWNER: &str = "craft-workspace-lock";
const LOCK_BYTES: usize = 1024;
/// A workspace lock is held for one bounded operation; where the holder's liveness cannot
/// be observed, a lock this old is abandoned.
#[cfg(windows)]
const UNKNOWN_LOCK_MS: u128 = 600_000;

fn refused(message: &str) -> io::Error {
    io::Error::other(message.to_string())
}

/// Whether `path` is a real directory; a missing one is created (0700) on request. A
/// directory created concurrently is accepted; anything else at the path is refused.
pub fn directory(path: &Path, create: bool) -> io::Result<bool> {
    let mut metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => Some(metadata),
        Err(error) if error.kind() == ErrorKind::NotFound => None,
        Err(error) => return Err(error),
    };
    if metadata.is_none() && create {
        let mut builder = fs::DirBuilder::new();
        #[cfg(unix)]
        {
            use std::os::unix::fs::DirBuilderExt;
            builder.mode(0o700);
        }
        match builder.create(path) {
            Ok(()) => {}
            Err(error) if error.kind() == ErrorKind::AlreadyExists => {}
            Err(error) => return Err(error),
        }
        metadata = Some(fs::symlink_metadata(path)?);
    }
    match metadata {
        Some(metadata) if !metadata.is_dir() || metadata.file_type().is_symlink() => {
            Err(refused("workspace directory must not be a symlink"))
        }
        found => Ok(found.is_some()),
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Usage {
    pub bytes: u64,
    pub files: u64,
}

/// Bytes and regular files below `root`; symlinks, special files and exhausted inspection
/// bounds refuse, so nothing is removed or counted that was not seen.
pub fn usage(root: &Path) -> io::Result<Usage> {
    let mut result = Usage::default();
    if !directory(root, false)? {
        return Ok(result);
    }
    let mut entries = 0;
    walk(root, 0, &mut entries, &mut result)?;
    Ok(result)
}

fn walk(path: &Path, depth: usize, entries: &mut usize, result: &mut Usage) -> io::Result<()> {
    *entries += 1;
    if depth > INSPECTION_DEPTH || *entries > INSPECTION_ENTRIES {
        return Err(refused("workspace inspection limit reached"));
    }
    let metadata = fs::symlink_metadata(path)?;
    let kind = metadata.file_type();
    if kind.is_symlink() {
        return Err(refused("workspace contains a symlink; cleanup refused"));
    }
    if kind.is_file() {
        result.bytes += metadata.len();
        result.files += 1;
    } else if kind.is_dir() {
        for child in fs::read_dir(path)? {
            walk(&child?.path(), depth + 1, entries, result)?;
        }
    } else {
        return Err(refused("workspace contains an unsupported file"));
    }
    Ok(())
}

/// Remove a tree after inspecting it; cooperating writers share the workspace lock.
pub fn remove(path: &Path) -> io::Result<()> {
    usage(path)?;
    if directory(path, false)? {
        fs::remove_dir_all(path)?;
    }
    if fs::symlink_metadata(path).is_ok() {
        return Err(refused("workspace cleanup could not be verified"));
    }
    Ok(())
}

/// Names in `dir`, refusing more than `limit`.
pub fn names(dir: &Path, limit: usize) -> io::Result<Vec<String>> {
    let mut names = Vec::new();
    for entry in fs::read_dir(dir)? {
        if names.len() >= limit {
            return Err(refused("workspace session inspection limit reached"));
        }
        names.push(entry?.file_name().to_string_lossy().into_owned());
    }
    Ok(names)
}

/// Create `path` with `bytes`; an existing file is never overwritten.
pub fn create(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options.open(path)?.write_all(bytes)
}

/// Copy one bounded regular file into a new `target`. `admit` sees the size before
/// anything is read; the opened handle must be the inspected file at that size.
pub fn copy(
    source: &Path,
    target: &Path,
    admit: impl FnOnce(u64) -> io::Result<()>,
) -> io::Result<u64> {
    let metadata = fs::symlink_metadata(source)?;
    #[cfg(unix)]
    let single = {
        use std::os::unix::fs::MetadataExt;
        metadata.nlink() == 1
    };
    // Stable Rust exposes no link count on Windows.
    #[cfg(not(unix))]
    let single = true;
    if !metadata.is_file() || metadata.file_type().is_symlink() || !single {
        return Err(refused("unsafe or oversized evidence"));
    }
    admit(metadata.len())?;
    if fs::symlink_metadata(target).is_ok() {
        return Err(refused("evidence already exists"));
    }
    let file = crate::search::open_source(source)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let opened = file.metadata()?;
        if opened.ino() != metadata.ino() || opened.dev() != metadata.dev() {
            return Err(refused("promotion source changed"));
        }
    }
    let mut bytes = Vec::new();
    file.take(metadata.len() + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 != metadata.len() {
        return Err(refused("promotion source changed size"));
    }
    create(target, &bytes)?;
    Ok(metadata.len())
}

/// Whether process `pid` runs. Unix asks the kernel; a process that exists but belongs
/// to another user counts as live. Stable Rust cannot ask on Windows without `unsafe`.
pub fn liveness(pid: i64) -> Liveness {
    if pid == i64::from(std::process::id()) {
        return Liveness::Live;
    }
    #[cfg(unix)]
    {
        let Some(pid) = i32::try_from(pid)
            .ok()
            .and_then(rustix::process::Pid::from_raw)
        else {
            return Liveness::Live;
        };
        match rustix::process::test_kill_process(pid) {
            Err(rustix::io::Errno::SRCH) => Liveness::Dead,
            _ => Liveness::Live,
        }
    }
    #[cfg(not(unix))]
    Liveness::Unknown
}

/// The workspace lock: the shared `.lock` protocol plus the holder's pid, so a lock left
/// by a dead holder can be reclaimed. Busy, publishing or unknown locks defer (`None`).
pub fn lock(anchor: &Path) -> io::Result<Option<Lock>> {
    let mut ticket = state::lock(anchor).map_err(io::Error::other)?;
    if ticket.is_none() {
        let mut name = anchor.as_os_str().to_owned();
        name.push(".lock");
        let file = Path::new(&name);
        let seen = match fs::symlink_metadata(file) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error),
        };
        let owner: Value = match read_bounded(file, LOCK_BYTES) {
            Ok(bytes) => match serde_json::from_slice(&bytes) {
                Ok(owner) => owner,
                // The holder has created but not yet recorded its lock.
                Err(_) => return Ok(None),
            },
            Err(ReadError::Io(error)) if error.kind() == ErrorKind::NotFound => return Ok(None),
            Err(ReadError::Io(error)) => return Err(error),
            Err(_) => return Err(refused("invalid bounded state file")),
        };
        let integer = craft_engine::contracts::safe_integer;
        let recognized =
            owner["owner"] == LOCK_OWNER && integer(&owner["formatVersion"]) == Some(1);
        let Some(pid) = integer(&owner["pid"]).filter(|pid| *pid >= 1 && recognized) else {
            return Ok(None);
        };
        if !abandoned(liveness(pid), &seen) {
            return Ok(None);
        }
        // Remove only the lock that was inspected; a replacement belongs to a new holder.
        match fs::symlink_metadata(file) {
            Ok(current) if same(&seen, &current) => fs::remove_file(file)?,
            Ok(_) => return Ok(None),
            Err(error) if error.kind() == ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
        ticket = state::lock(anchor).map_err(io::Error::other)?;
    }
    if let Some(held) = ticket.as_mut() {
        let owner =
            serde_json::json!({"owner": LOCK_OWNER, "formatVersion": 1, "pid": std::process::id()});
        held.record(owner.to_string().as_bytes())?;
    }
    Ok(ticket)
}

fn abandoned(liveness: Liveness, _lock: &fs::Metadata) -> bool {
    match liveness {
        Liveness::Dead => true,
        Liveness::Live => false,
        #[cfg(windows)]
        Liveness::Unknown => _lock
            .modified()
            .ok()
            .and_then(|modified| modified.elapsed().ok())
            .is_some_and(|age| age.as_millis() >= UNKNOWN_LOCK_MS),
        #[cfg(not(windows))]
        Liveness::Unknown => false,
    }
}

fn same(seen: &fs::Metadata, current: &fs::Metadata) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        seen.ino() == current.ino() && seen.dev() == current.dev()
    }
    #[cfg(not(unix))]
    {
        seen.len() == current.len() && seen.modified().ok() == current.modified().ok()
    }
}

#[cfg(test)]
mod tests;
