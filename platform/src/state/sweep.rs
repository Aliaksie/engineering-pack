//! Bounded inventory of session directories and their verified removal: a directory is
//! renamed aside, listed again and removed only when nothing in it changed since inventory.
use super::{MANIFEST, MANIFEST_BYTES, directory, location, read};
use crate::Error;
use craft_engine::retention::{Described, Entry, SESSION_ENTRIES, SHARED_FAMILIES, described};
use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// Session directories inspected in one run.
pub const SESSION_DIRECTORIES: usize = 4096;
/// Workspace directories inspected in one run.
const SHARED_ENTRIES: usize = 256;
const STAGED: &str = ".sweep-";

/// One session directory as observed.
#[derive(Clone, Debug)]
pub struct Directory {
    pub id: String,
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

/// Size of one current shared family, or why it could not be measured.
#[derive(Debug)]
pub struct Family {
    pub name: &'static str,
    pub usage: Result<crate::scratch::Usage, String>,
}

#[derive(Debug, Default)]
pub struct Inventory {
    pub sessions: Vec<Directory>,
    /// Workspace directories, for the caller to verify ownership.
    pub workspaces: Vec<(String, PathBuf)>,
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
pub fn inspect(path: &Path, id: &str, staged: bool) -> Result<Directory, Error> {
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
    let described = if !manifest {
        Described::Absent
    } else {
        match read(&path.join(MANIFEST), MANIFEST_BYTES) {
            Ok(value) => described(value.as_ref()),
            Err(_) => Described::Invalid,
        }
    };
    Ok(Directory {
        id: id.to_string(),
        path: path.to_path_buf(),
        staged,
        entries,
        complete,
        modified: millis(meta.modified()),
        described,
    })
}

/// The worktree's state folder.
fn base(root: &Path) -> Result<PathBuf, Error> {
    Ok(location(root, None)?.directory)
}

/// Up to `limit` session directories of the worktree, unstaged; one that
/// cannot be inspected is skipped. Nothing is created.
pub fn sessions(root: &Path, limit: usize) -> Result<Vec<Directory>, Error> {
    let folder = base(root)?.join("sessions");
    let (names, _) = listing(&folder, limit)?;
    Ok(names
        .iter()
        .filter_map(|name| match session_id(name) {
            Some((id, false)) => inspect(&folder.join(name), id, false).ok(),
            _ => None,
        })
        .collect())
}

/// The session directory `id` of the worktree, if it exists.
pub fn session(root: &Path, id: &str) -> Result<Option<Directory>, Error> {
    if !craft_engine::values::lower_hex(id, 24) {
        return Err("invalid session directory id".into());
    }
    let path = base(root)?.join("sessions").join(id);
    match fs::symlink_metadata(&path) {
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(None),
        _ => inspect(&path, id, false).map(Some),
    }
}

/// Up to `limit` plan folder names of the worktree, and whether that was all of them.
pub fn plans(root: &Path, limit: usize) -> Result<(Vec<String>, bool), Error> {
    let folder = base(root)?.join(super::PLANS);
    let (names, complete) = listing(&folder, limit)?;
    let plans = names
        .into_iter()
        .filter(|name| {
            craft_engine::plan::slug(name) && directory(&folder.join(name), false).unwrap_or(false)
        })
        .collect();
    Ok((plans, complete))
}

/// Remove a plan folder holding only the named files; anything else keeps it.
pub fn remove_plan(folder: &Path, names: &[&str]) -> Result<(), Error> {
    for name in names {
        remove_file(&folder.join(name))?;
    }
    fs::remove_dir(folder).map_err(Error::from)
}

/// Session directories under the worktree's state and its workspaces. Nothing is created.
pub fn inventory(root: &Path) -> Result<Inventory, Error> {
    let base = base(root)?;
    let mut inventory = Inventory {
        complete: true,
        workspaces_complete: true,
        ..Inventory::default()
    };
    let folder = base.join("sessions");
    let (names, complete) = listing(&folder, SESSION_DIRECTORIES + 2)?;
    inventory.complete = complete;
    for name in names {
        let Some((id, staged)) = session_id(&name) else {
            continue;
        };
        if inventory.sessions.len() == SESSION_DIRECTORIES {
            inventory.complete = false;
            break;
        }
        inventory
            .sessions
            .push(inspect(&folder.join(&name), id, staged)?);
    }
    let folder = base.join("workspaces");
    let (names, complete) = listing(&folder, SHARED_ENTRIES)?;
    inventory.workspaces_complete = complete;
    inventory.workspaces.extend(
        names
            .into_iter()
            .filter(|name| !name.starts_with("guard.json"))
            .map(|name| (name.clone(), folder.join(name))),
    );
    for (name, _) in SHARED_FAMILIES {
        inventory.families.push(Family {
            name,
            usage: crate::scratch::usage(&base.join(name)).map_err(|e| e.to_string()),
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
    let again = inspect(staged, &found.id, true)?;
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
