//! `state-clean`: preview, or with `apply` remove, idle session directories of the
//! worktree's private state. Shared records are only reported.
use crate::failure::Failure;
use crate::session::read_record;
use crate::telemetry::now_ms;
use craft_engine::retention::{self, SESSION_RETENTION_DAYS, Session, Workspaces};
use craft_engine::scratch::{self, MANIFEST_BYTES, Policy};
use craft_platform::state::{self, sweep};
use serde_json::{Value, json};
use std::path::Path;

pub fn run(root: &Path, settings: &Value, apply: bool) -> Result<Value, Failure> {
    let now = now_ms();
    let inventory = sweep::inventory(root)?;
    let workspaces = open_workspaces(root, &inventory, &Policy::from_settings(settings), now)?;
    let (mut removable, mut bytes) = (0, 0);
    let mut sessions = Vec::new();
    for found in &inventory.sessions {
        let session = Session {
            id: &found.id,
            layout: found.layout,
            entries: &found.entries,
            complete: found.complete,
            modified: found.modified,
            described: found.described.clone(),
        };
        let mut item = json!({
            "session": found.id,
            "layout": found.layout.name(),
            "bytes": session.bytes(),
            "idleDays": retention::idle_days(&session, now),
        });
        if found.staged {
            item["staged"] = json!(true);
        }
        let kept = match retention::keep(&session, now, &workspaces) {
            // An interrupted cleanup set this directory aside; it no longer qualifies.
            Some(reason) if apply && found.staged => sweep::restore(found, &found.path)
                .map_or_else(|e| Some(e.to_string()), |()| Some(reason.to_string())),
            Some(reason) => Some(reason.to_string()),
            None if apply => remove(found).err(),
            None => None,
        };
        match kept {
            Some(reason) => {
                item["action"] = json!("keep");
                item["reason"] = json!(reason);
            }
            None => {
                item["action"] = json!(if apply { "removed" } else { "remove" });
                removable += 1;
                bytes += session.bytes();
            }
        }
        sessions.push(item);
    }
    let previous: Vec<Value> = inventory
        .leftovers
        .iter()
        .map(|leftover| {
            json!({"name": leftover.name, "directory": leftover.directory, "bytes": leftover.bytes})
        })
        .collect();
    Ok(json!({
        "status": if apply { "applied" } else { "preview" },
        "retentionDays": SESSION_RETENTION_DAYS,
        "complete": inventory.complete,
        "summary": {
            "inspected": sessions.len(),
            (if apply { "removed" } else { "removable" }): removable,
            "bytes": bytes,
        },
        "sessions": sessions,
        "previousShared": previous,
    }))
}

/// Set the directory aside, confirm nothing changed since inventory, then remove it.
fn remove(found: &sweep::Directory) -> Result<(), String> {
    let text = |error: craft_platform::Error| error.to_string();
    let staged = sweep::stage(found).map_err(text)?;
    if !sweep::unchanged(found, &staged).map_err(text)? {
        sweep::restore(found, &staged).map_err(text)?;
        return Err("changed during cleanup".into());
    }
    sweep::purge(found, &staged).map_err(text)
}

/// Session identities of open workspaces in either layout. A workspace whose ownership
/// cannot be verified, or an incomplete listing, may hold any session open.
fn open_workspaces(
    root: &Path,
    inventory: &sweep::Inventory,
    policy: &Policy,
    now: i64,
) -> Result<Workspaces, Failure> {
    let project = craft_engine::hash(state::canonical_root(root)?.to_string_lossy().as_bytes());
    let mut open = Workspaces {
        unverified: !inventory.workspaces_complete,
        ..Workspaces::default()
    };
    for (entry, folder) in &inventory.workspaces {
        let owner = read_record(&folder.join("owner.json"), MANIFEST_BYTES)
            .ok()
            .filter(|owner| scratch::validate(owner, &project, entry).is_ok());
        match owner {
            Some(owner) if scratch::active(&owner, now, policy) => {
                open.active
                    .insert(owner["session"].as_str().unwrap_or_default().to_string());
            }
            Some(_) => {}
            None => open.unverified = true,
        }
    }
    Ok(open)
}
