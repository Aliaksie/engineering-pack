//! `state-clean`: preview, or with `apply` remove, idle session directories of the
//! worktree's private state. Shared families are only measured, never removed here.
use crate::failure::Failure;
use crate::session::read_record;
use crate::telemetry::now_ms;
use craft_engine::retention::{self, SESSION_RETENTION_DAYS, SHARED_FAMILIES, Session, Workspaces};
use craft_engine::scratch::{self, MANIFEST_BYTES, Policy};
use craft_platform::state::{self, sweep};
use serde_json::{Value, json};
use std::path::Path;

pub fn run(root: &Path, settings: &Value, apply: bool) -> Result<Value, Failure> {
    let now = now_ms();
    let inventory = sweep::inventory(root)?;
    let policy = Policy::from_settings(settings);
    let workspaces = open_workspaces(root, &inventory, &policy, now)?;
    let plans = crate::plan::retained(root, apply)?;
    let (mut removable, mut bytes) = (0, 0);
    let mut sessions = Vec::new();
    for found in &inventory.sessions {
        let session = Session {
            id: &found.id,
            entries: &found.entries,
            complete: found.complete,
            modified: found.modified,
            staged: found.staged,
            described: found.described.clone(),
        };
        let mut item = json!({
            "session": found.id,
            "bytes": session.bytes(),
        });
        if !(found.staged && found.entries.is_empty()) {
            item["idleDays"] = json!(retention::idle_days(&session, now));
        }
        if found.staged {
            item["staged"] = json!(true);
        }
        // A take-over copies the records of the session holding a plan task.
        let holder = plans
            .owners
            .contains(&found.id)
            .then_some("holds a task of an open plan");
        let kept = match retention::keep(&session, now, &workspaces).or(holder) {
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
    let shared: Vec<Value> = inventory
        .families
        .iter()
        .zip(SHARED_FAMILIES)
        .map(|(family, (_, bounded))| match &family.usage {
            Ok(usage) => json!({
                "family": family.name,
                "bytes": usage.bytes,
                "files": usage.files,
                "bounded": bounded,
            }),
            Err(reason) => json!({"family": family.name, "bounded": bounded, "reason": reason}),
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
        "plans": plans.plans,
        "shared": shared,
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

/// Session identities of open workspaces. A workspace whose ownership
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
