//! `state-clean`: preview, or with `apply` remove, idle session directories of the
//! worktree's private state and what the previous layout left in `shared/` once nothing
//! depends on it. Current shared records are never removed here.
use crate::failure::Failure;
use crate::session::read_record;
use crate::telemetry::now_ms;
use craft_engine::retention::{self, Leftover, SESSION_RETENTION_DAYS, Session, Workspaces};
use craft_engine::scratch::{self, MANIFEST_BYTES, Policy};
use craft_platform::scratch as files;
use craft_platform::state::{self, sweep};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::path::Path;

pub fn run(root: &Path, settings: &Value, apply: bool) -> Result<Value, Failure> {
    let now = now_ms();
    let inventory = sweep::inventory(root)?;
    let policy = Policy::from_settings(settings);
    let workspaces = open_workspaces(root, &inventory, &policy, now)?;
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
    let outcomes = previous_shared(root, &inventory, &policy, now, apply)?;
    let previous: Vec<Value> = inventory
        .leftovers
        .iter()
        .map(|leftover| {
            let mut item = json!({
                "name": leftover.name,
                "directory": leftover.directory,
                "bytes": leftover.bytes,
            });
            match &outcomes[&leftover.name] {
                Ok(()) => item["action"] = json!(if apply { "removed" } else { "remove" }),
                Err(reason) => {
                    item["action"] = json!("keep");
                    item["reason"] = json!(reason);
                }
            }
            item
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

/// Whether each entry the previous layout left in `shared/` may go, removing it with
/// `apply`: copied records once the copy is current, its receipts, the rebuildable evidence
/// database when unlocked, and workspaces under workspace retention, then their guard.
fn previous_shared(
    root: &Path,
    inventory: &sweep::Inventory,
    policy: &Policy,
    now: i64,
    apply: bool,
) -> Result<HashMap<String, Result<(), String>>, Failure> {
    let text = |error: craft_platform::Error| error.to_string();
    let mut outcomes = HashMap::new();
    let mut database = None;
    let mut workspaces_left = false;
    for leftover in &inventory.leftovers {
        let name = &leftover.name;
        if outcomes.contains_key(name) {
            continue;
        }
        let outcome = match retention::leftover(name) {
            Leftover::Record => match state::retirable(root, name) {
                Ok(Some(names)) => {
                    let outcome = if apply {
                        sweep::discard(root, &names).map_err(text)
                    } else {
                        Ok(())
                    };
                    for other in names {
                        outcomes.insert(other, outcome.clone());
                    }
                    outcome
                }
                Ok(None) => Err("unrecognized".into()),
                Err(error) => Err(error.to_string()),
            },
            Leftover::Receipt if apply => {
                sweep::discard(root, std::slice::from_ref(name)).map_err(text)
            }
            Leftover::Receipt => Ok(()),
            Leftover::Database => database
                .get_or_insert_with(|| {
                    if apply {
                        sweep::discard_database(root).map_err(text)
                    } else {
                        Ok(())
                    }
                })
                .clone(),
            Leftover::Workspaces => {
                let outcome = previous_workspaces(root, inventory, policy, now, apply);
                workspaces_left = outcome.is_err();
                outcome
            }
            // Decided once every workspace it guards is.
            Leftover::Guard => continue,
            Leftover::Lock => Err("held by a writer".into()),
            Leftover::Unknown => Err("unrecognized".into()),
        };
        outcomes.insert(name.clone(), outcome);
    }
    for leftover in &inventory.leftovers {
        if retention::leftover(&leftover.name) == Leftover::Guard {
            let outcome = if workspaces_left {
                Err("a workspace of the previous layout remains".into())
            } else {
                retire_guard(root, apply)
            };
            outcomes.insert(leftover.name.clone(), outcome);
        }
    }
    Ok(outcomes)
}

/// Previous-layout workspaces go when their owner is verified, they are inactive and their
/// evidence retention expired, as `workspace-clean` decides for current ones.
fn previous_workspaces(
    root: &Path,
    inventory: &sweep::Inventory,
    policy: &Policy,
    now: i64,
    apply: bool,
) -> Result<(), String> {
    if !inventory.workspaces_complete {
        return Err("too many workspaces to inspect".into());
    }
    let shared = state::location(root, None)
        .map_err(|e| e.to_string())?
        .directory;
    let folder = shared.join("sessions");
    let project = craft_engine::hash(
        state::canonical_root(root)
            .map_err(|e| e.to_string())?
            .to_string_lossy()
            .as_bytes(),
    );
    let _lock = if apply {
        Some(
            files::lock(&shared.join("workspace-guard.json"))
                .map_err(|e| e.to_string())?
                .ok_or("workspace guard is busy")?,
        )
    } else {
        None
    };
    let mut kept = 0;
    for (entry, path) in &inventory.workspaces {
        if !path.starts_with(&folder) {
            continue;
        }
        let owner = read_record(&path.join("owner.json"), MANIFEST_BYTES)
            .ok()
            .filter(|owner| scratch::validate(owner, &project, entry).is_ok());
        let removable = owner.is_some_and(|owner| {
            !scratch::active(&owner, now, policy) && scratch::evidence_expired(&owner, now, policy)
        }) && files::names(path, scratch::MANIFEST_ENTRIES).is_ok_and(|names| {
            names
                .iter()
                .all(|name| matches!(name.as_str(), "owner.json" | "scratch" | "evidence"))
        });
        if !removable {
            kept += 1;
        } else if apply {
            files::remove(path).map_err(|e| e.to_string())?;
        }
    }
    if kept > 0 {
        return Err(format!(
            "{kept} workspace(s) open, unverified or within evidence retention"
        ));
    }
    if apply {
        files::remove(&folder).map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// The previous workspace guard, once no workspace of that layout remains.
fn retire_guard(root: &Path, apply: bool) -> Result<(), String> {
    if !apply {
        return Ok(());
    }
    let text = |error: craft_platform::Error| error.to_string();
    let shared = state::location(root, None).map_err(text)?.directory;
    let _lock = files::lock(&shared.join("workspace-guard.json"))
        .map_err(|e| e.to_string())?
        .ok_or("workspace guard is busy")?;
    sweep::discard(root, &["workspace-guard.json".to_string()]).map_err(text)
}
