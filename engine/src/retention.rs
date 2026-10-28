//! Retention of private session directories: when a session's records may be removed.
//! Only idle, recognized and unheld directories qualify; everything else is kept with a reason.
use crate::values::lower_hex;
use serde_json::Value;
use std::collections::HashSet;

/// Days without any write after which an unheld session directory may be removed.
pub const SESSION_RETENTION_DAYS: i64 = 14;
const DAY_MS: i64 = 86_400_000;
/// Entries one session directory may hold; a fuller one is not inspected further.
pub const SESSION_ENTRIES: usize = 64;

/// Current families under `shared/` and what keeps each bounded. `state-clean` reports
/// their size and never removes them.
pub const SHARED_FAMILIES: &[(&str, &str)] = &[
    ("telemetry", "rotation: one bounded log and one backup"),
    ("intelligence", "rebuildable: project-evidence rebuild"),
    ("workspaces", "workspace-clean"),
];

/// One directory entry as observed without following links.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    pub name: String,
    /// A regular file, not a link, directory or special file.
    pub file: bool,
    pub bytes: u64,
    pub modified: i64,
}

/// What a session manifest says about the session's workspaces.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Described {
    Absent,
    Workspace(String),
    Invalid,
}

/// The workspace session identity a manifest names; any other shape is invalid.
pub fn described(manifest: Option<&Value>) -> Described {
    let Some(manifest) = manifest else {
        return Described::Absent;
    };
    match manifest["workspace"].as_str() {
        Some(workspace) if manifest["formatVersion"] == 1 && lower_hex(workspace, 64) => {
            Described::Workspace(workspace.to_string())
        }
        _ => Described::Invalid,
    }
}

/// Open workspaces by session identity. An unverified workspace may belong to any session.
#[derive(Clone, Debug, Default)]
pub struct Workspaces {
    pub active: HashSet<String>,
    pub unverified: bool,
}

pub struct Session<'a> {
    pub id: &'a str,
    pub entries: &'a [Entry],
    /// Whether every entry was listed within [`SESSION_ENTRIES`].
    pub complete: bool,
    /// Modification time of the directory itself.
    pub modified: i64,
    /// Set aside by a cleanup that may have removed some of its entries already.
    pub staged: bool,
    pub described: Described,
}

impl Session<'_> {
    pub fn bytes(&self) -> u64 {
        self.entries.iter().map(|entry| entry.bytes).sum()
    }

    /// The most recent write to the directory or any entry. A staged directory's own time
    /// records the cleanup's removals, so only its entries count; an empty one has none.
    pub fn newest(&self) -> i64 {
        let own = if self.staged { 0 } else { self.modified };
        self.entries
            .iter()
            .map(|entry| entry.modified)
            .fold(own, i64::max)
    }
}

fn hex_record(name: &str, prefix: &str, suffix: &str) -> bool {
    name.strip_prefix(prefix)
        .and_then(|rest| rest.strip_suffix(suffix))
        .is_some_and(|id| lower_hex(id, 24))
}

/// Records the runtime writes into a session directory.
fn recognized(name: &str) -> bool {
    matches!(name, "manifest.json" | "checkpoint.json" | "task.json")
        || hex_record(name, "routing-", ".json")
        || hex_record(name, "routing-", ".json.generation")
        || ["activity-", "workers-", "control-"]
            .iter()
            .any(|prefix| hex_record(name, prefix, ".json"))
}

/// `None` when the directory may be removed, otherwise why it is kept.
pub fn keep(session: &Session, now: i64, workspaces: &Workspaces) -> Option<&'static str> {
    let names = || session.entries.iter().map(|entry| entry.name.as_str());
    if !session.complete {
        return Some("too many entries to inspect");
    }
    if names().any(|name| name.ends_with(".lock") || name.contains(".lease-guard")) {
        return Some("held by a writer");
    }
    if session.entries.iter().any(|entry| !entry.file) {
        return Some("holds an entry that is not a regular file");
    }
    if !names().all(recognized) {
        return Some("holds an unrecognized file");
    }
    if now - session.newest() < SESSION_RETENTION_DAYS * DAY_MS {
        return Some("written within the retention period");
    }
    let open = match &session.described {
        Described::Invalid => return Some("manifest is invalid"),
        Described::Workspace(workspace) => workspaces.active.contains(workspace),
        // Without a manifest, any open workspace may be this session's.
        Described::Absent => !workspaces.active.is_empty(),
    };
    if open || workspaces.unverified {
        return Some("a workspace may still be open");
    }
    None
}

/// Whole days since the last write, for reports.
pub fn idle_days(session: &Session, now: i64) -> i64 {
    ((now - session.newest()) / DAY_MS).max(0)
}

#[cfg(test)]
mod tests;
