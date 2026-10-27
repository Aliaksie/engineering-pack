//! Bounded inventory of session directories and their verified removal: a directory is
//! renamed aside, listed again and removed only when nothing in it changed since inventory.
use super::{MANIFEST, MANIFEST_BYTES, directory, location, read};
use crate::Error;
use craft_engine::retention::{
    DATABASE, Described, Entry, Layout, SESSION_ENTRIES, SHARED_FAMILIES, described,
};
use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// Session directories inspected in one run, across both layouts.
pub const SESSION_DIRECTORIES: usize = 4096;
/// Workspace directories and previous-layout shared entries inspected in one run.
const SHARED_ENTRIES: usize = 256;
const STAGED: &str = ".sweep-";

/// One session directory as observed.
#[derive(Clone, Debug)]
pub struct Directory {
    pub id: String,
    pub layout: Layout,
    /// Where it was found; a staged directory was set aside by an interrupted cleanup.
    pub path: PathBuf,
    pub staged: bool,
    pub entries: Vec<Entry>,
    pub complete: bool,
    pub modified: i64,
    pub described: Described,
}

impl Directory {
    /// The name the directory has outside cleanup.
    pub fn original(&self) -> PathBuf {
        self.path.with_file_name(&self.id)
    }
}

/// A previous-layout entry left in `shared/` after its records were copied.
#[derive(Clone, Debug)]
pub struct Leftover {
    pub name: String,
    pub directory: bool,
    pub bytes: Option<u64>,
}

/// Size of one current shared family, or why it could not be measured.
#[derive(Debug)]
pub struct Family {
    pub name: &'static str,
    pub usage: Result<crate::scratch::Usage, String>,
}

#[derive(Debug, Default)]
pub struct Inventory {
    pub sessions: Vec<Directory>,
    /// Workspace directories of both layouts, for the caller to verify ownership.
    pub workspaces: Vec<(String, PathBuf)>,
    pub leftovers: Vec<Leftover>,
    pub families: Vec<Family>,
    /// Whether every session directory was listed within its bound.
    pub complete: bool,
    /// Whether every workspace directory was listed within its bound.
    pub workspaces_complete: bool,
}

fn millis(time: std::io::Result<SystemTime>) -> i64 {
    time.ok()
        .and_then(|time| time.duration_since(SystemTime::UNIX_EPOCH).ok())
        .map_or(0, |elapsed| elapsed.as_millis() as i64)
}

/// Up to `limit` names in `folder`, and whether that was all of them. A missing folder is empty.
fn listing(folder: &Path, limit: usize) -> Result<(Vec<String>, bool), Error> {
    if !directory(folder, false)? {
        return Ok((Vec::new(), true));
    }
    let mut names = Vec::new();
    for entry in fs::read_dir(folder)? {
        if names.len() == limit {
            return Ok((names, false));
        }
        let name = entry?.file_name();
        names.push(
            name.into_string()
                .map_err(|_| Error::from("non-UTF-8 state path unsupported"))?,
        );
    }
    names.sort();
    Ok((names, true))
}

fn session_id(name: &str) -> Option<(&str, bool)> {
    let (id, staged) = match name.strip_prefix(STAGED) {
        Some(id) => (id, true),
        None => (name, false),
    };
    craft_engine::values::lower_hex(id, 24).then_some((id, staged))
}

/// List one session directory without following links.
pub fn inspect(path: &Path, id: &str, layout: Layout, staged: bool) -> Result<Directory, Error> {
    let meta = fs::symlink_metadata(path)?;
    if !meta.is_dir() || meta.file_type().is_symlink() {
        return Err("state path must be a directory without symlinks".into());
    }
    let (names, complete) = listing(path, SESSION_ENTRIES)?;
    let mut entries = Vec::with_capacity(names.len());
    for name in names {
        let meta = fs::symlink_metadata(path.join(&name))?;
        entries.push(Entry {
            file: meta.is_file() && !meta.file_type().is_symlink(),
            bytes: meta.len(),
            modified: millis(meta.modified()),
            name,
        });
    }
    let manifest = entries
        .iter()
        .any(|entry| entry.file && entry.name == MANIFEST);
    let described = match (layout, manifest) {
        (Layout::Current, true) => match read(&path.join(MANIFEST), MANIFEST_BYTES) {
            Ok(value) => described(value.as_ref()),
            Err(_) => Described::Invalid,
        },
        _ => Described::Absent,
    };
    Ok(Directory {
        id: id.to_string(),
        layout,
        path: path.to_path_buf(),
        staged,
        entries,
        complete,
        modified: millis(meta.modified()),
        described,
    })
}

/// Session directories of both layouts under the worktree's state, its workspaces and the
/// previous layout's shared leftovers. Nothing is created.
pub fn inventory(root: &Path) -> Result<Inventory, Error> {
    let shared = location(root, None)?.directory;
    let worktree = shared
        .parent()
        .ok_or("state worktree required")?
        .to_path_buf();
    let mut inventory = Inventory {
        complete: true,
        workspaces_complete: true,
        ..Inventory::default()
    };
    // `shared` and `sessions` sit beside previous-layout session directories.
    let current = worktree.join("sessions");
    for (folder, layout) in [(&current, Layout::Current), (&worktree, Layout::Previous)] {
        let room = SESSION_DIRECTORIES - inventory.sessions.len();
        let (names, complete) = listing(folder, room + 2)?;
        let mut found = 0;
        for name in names {
            let Some((id, staged)) = session_id(&name) else {
                continue;
            };
            if found == room {
                inventory.complete = false;
                break;
            }
            found += 1;
            inventory
                .sessions
                .push(inspect(&folder.join(&name), id, layout, staged)?);
        }
        inventory.complete &= complete;
    }
    for family in ["workspaces", "sessions"] {
        let folder = shared.join(family);
        let (names, complete) = listing(&folder, SHARED_ENTRIES)?;
        inventory.workspaces_complete &= complete;
        inventory.workspaces.extend(
            names
                .into_iter()
                .filter(|name| !name.starts_with("guard.json"))
                .map(|name| (name.clone(), folder.join(name))),
        );
    }
    for (name, _) in SHARED_FAMILIES {
        inventory.families.push(Family {
            name,
            usage: crate::scratch::usage(&shared.join(name)).map_err(|e| e.to_string()),
        });
    }
    let (names, _) = listing(&shared, SHARED_ENTRIES)?;
    for name in names {
        if SHARED_FAMILIES.iter().any(|(family, _)| *family == name) {
            continue;
        }
        let path = shared.join(&name);
        let meta = fs::symlink_metadata(&path)?;
        let folder = meta.is_dir() && !meta.file_type().is_symlink();
        let bytes = if folder {
            crate::scratch::usage(&path).ok().map(|usage| usage.bytes)
        } else {
            Some(meta.len())
        };
        inventory.leftovers.push(Leftover {
            name,
            directory: folder,
            bytes,
        });
    }
    Ok(inventory)
}

fn remove_file(path: &Path) -> Result<(), Error> {
    match fs::remove_file(path) {
        Err(error) if error.kind() != ErrorKind::NotFound => Err(error.into()),
        _ => Ok(()),
    }
}

/// Remove files the previous layout left directly in `shared/`, by name.
pub fn discard(root: &Path, names: &[String]) -> Result<(), Error> {
    let shared = location(root, None)?.directory;
    for name in names {
        let path = shared.join(name);
        if fs::symlink_metadata(&path).is_ok_and(|meta| !meta.is_file()) {
            return Err("previous shared entry is not a regular file".into());
        }
        remove_file(&path)?;
    }
    Ok(())
}

/// Remove the previous layout's evidence database and its companions under its lock, so
/// no writer still has it open.
pub fn discard_database(root: &Path) -> Result<(), Error> {
    let shared = location(root, None)?.directory;
    let database = shared.join(DATABASE[0]);
    let guard = crate::repository::database_guard(&database)?;
    let lock = format!("{}.lock", DATABASE[0]);
    // Companions first: a database left without them is still a valid one.
    for name in DATABASE.iter().rev().filter(|name| **name != lock) {
        remove_file(&shared.join(name))?;
    }
    // Windows cannot remove a file still open; the lock goes last, once released.
    drop(guard);
    remove_file(&shared.join(lock))
}

/// Set a directory aside under its staged name; a staged directory stays where it is.
pub fn stage(found: &Directory) -> Result<PathBuf, Error> {
    if found.staged {
        return Ok(found.path.clone());
    }
    let staged = found.path.with_file_name(format!("{STAGED}{}", found.id));
    if fs::symlink_metadata(&staged).is_ok() {
        return Err("an earlier cleanup of this session is unfinished".into());
    }
    fs::rename(&found.path, &staged)?;
    Ok(staged)
}

/// Return a staged directory to its name unless a writer has created a new one there.
pub fn restore(found: &Directory, staged: &Path) -> Result<(), Error> {
    match fs::symlink_metadata(found.original()) {
        Err(error) if error.kind() == ErrorKind::NotFound => {
            Ok(fs::rename(staged, found.original())?)
        }
        Err(error) => Err(error.into()),
        Ok(_) => Err("a new session directory replaced the one being cleaned".into()),
    }
}

/// Whether a staged directory still holds exactly the inspected entries.
pub fn unchanged(found: &Directory, staged: &Path) -> Result<bool, Error> {
    let again = inspect(staged, &found.id, found.layout, true)?;
    Ok(again.complete && again.entries == found.entries)
}

/// Remove the inspected files of a staged directory, then the directory; anything a
/// writer added meanwhile makes the final removal fail and keeps it. The manifest goes
/// last, so an interrupted removal still describes the session's workspaces.
pub fn purge(found: &Directory, staged: &Path) -> Result<(), Error> {
    let (manifest, records): (Vec<_>, Vec<_>) = found
        .entries
        .iter()
        .partition(|entry| entry.name == MANIFEST);
    for entry in records.into_iter().chain(manifest) {
        match fs::remove_file(staged.join(&entry.name)) {
            Err(error) if error.kind() != ErrorKind::NotFound => return Err(error.into()),
            _ => {}
        }
    }
    Ok(fs::remove_dir(staged)?)
}

#[cfg(test)]
mod tests;
