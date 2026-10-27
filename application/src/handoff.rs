//! Handoff of an unfinished task to a new session: offered when a session starts without a
//! task, adopted only on request. The earlier session's records are copied, never changed.
use crate::failure::Failure;
use crate::session::{self as records, BINDING_BYTES, STATE_BYTES, read_record};
use crate::telemetry::{Operation, Recorder, now_ms};
use craft_engine::session::{self, handoff};
use craft_engine::values::truthy;
use craft_platform::repository::snapshot;
use craft_platform::state::{self, sweep};
use serde_json::{Value, json};
use std::collections::HashMap;

/// Record families that stay with the session whose native work they describe.
const NOT_CARRIED: &[(&str, &str)] = &[("workers-", "workers"), ("control-", "control")];

/// An earlier session's task binding and the task records it holds, with when each changed.
fn saved(directory: &sweep::Directory) -> Result<(Value, Vec<(Value, i64)>), Failure> {
    let binding = read_record(&directory.path.join("task.json"), BINDING_BYTES)?;
    let mut states = Vec::new();
    if binding.is_null() {
        return Ok((binding, states));
    }
    for entry in &directory.entries {
        if entry.file && entry.name.starts_with("activity-") && entry.name.ends_with(".json") {
            let state = read_record(&directory.path.join(&entry.name), STATE_BYTES)?;
            states.push((state, entry.modified));
        }
    }
    Ok((binding, states))
}

/// The checkout a binding is compared with: of the root, or of its selected repositories.
fn checkout(root: &std::path::Path, binding: &Value) -> Result<Value, Failure> {
    let repositories = binding.get("repositories").filter(|r| truthy(*r));
    Ok(snapshot::workspace(root, repositories, true)?["checkout"].clone())
}

/// Unfinished tasks of earlier sessions in this worktree that `session` may adopt, newest
/// first. A directory that cannot be read offers nothing.
pub fn offer(root: &std::path::Path, session: &str) -> Result<Vec<Value>, Failure> {
    let own = state::location(root, Some(session))?.session_id;
    let now = now_ms();
    let mut checkouts: HashMap<String, Value> = HashMap::new();
    let mut candidates = Vec::new();
    for directory in sweep::sessions(root, handoff::SCANNED)? {
        if directory.id == own || !directory.complete {
            continue;
        }
        let found = (|| -> Result<Option<Value>, Failure> {
            let (binding, states) = saved(&directory)?;
            if binding.is_null() {
                return Ok(None);
            }
            let key = binding
                .get("repositories")
                .map_or_else(String::new, Value::to_string);
            let current = match checkouts.get(&key) {
                Some(current) => current.clone(),
                None => {
                    let current = checkout(root, &binding)?;
                    checkouts.insert(key, current.clone());
                    current
                }
            };
            let saved = handoff::Saved {
                from: &directory.id,
                binding: &binding,
                states: &states,
            };
            Ok(handoff::candidate(&saved, &current)?)
        })();
        if let Ok(Some(candidate)) = found {
            candidates.push(candidate);
        }
    }
    Ok(handoff::offer(candidates, now))
}

/// Copy the unfinished task of the earlier session directory `from` into `session` and bind
/// it: task record first, then its checkpoint as saved, then the binding. Repeating an
/// interrupted or completed adoption converges; nothing of another task is overwritten.
pub fn adopt(
    root: &std::path::Path,
    session: &str,
    from: &str,
    recorder: &mut Recorder,
    parent: &Operation,
) -> Result<Value, Failure> {
    session::identity(&json!(session), &json!("handoff"))?;
    if state::location(root, Some(session))?.session_id == from {
        return Err("a session cannot adopt its own task".into());
    }
    let directory = sweep::session(root, from)?
        .filter(|d| d.complete)
        .ok_or("no earlier session with that id in this worktree")?;
    let (binding, states) = saved(&directory)?;
    let current = checkout(root, &binding)?;
    let offered = handoff::candidate(
        &handoff::Saved {
            from,
            binding: &binding,
            states: &states,
        },
        &current,
    )?
    .filter(|candidate| handoff::open(candidate, now_ms()))
    .ok_or("that session hands off no unfinished task for this checkout")?;
    let task = offered["task"].as_str().unwrap_or_default().to_string();
    let (source, _) = states
        .iter()
        .find(|(state, _)| state["taskRecord"]["id"] == offered["task"])
        .expect("offered task has its record");
    let checkpoint = read_record(
        &directory.path.join("checkpoint.json"),
        crate::checkpoint::CHECKPOINT_BYTES,
    )?;
    craft_engine::checkpoint::validate(&checkpoint)?;
    let checkpoint = Some(checkpoint).filter(|c| c.get("taskId") == Some(&offered["task"]));

    let file = records::binding_file(root, session, true)?;
    let Some(_lock) = state::lock(&file)? else {
        return Err("active task writer is busy".into());
    };
    let bound = records::active_task(root, Some(session))?;
    if let Some(other) = bound.as_ref().filter(|t| **t != task) {
        return Err(format!(
            "task {other} is bound to this session; adopt the earlier task in a new session"
        )
        .into());
    }
    let adopted = bound.is_none();
    if adopted {
        records::update_state(root, session, &task, recorder, parent, |state| {
            if truthy(&state["taskRecord"]) && *state != *source {
                return Err("this session already has a different record of the task".into());
            }
            *state = source.clone();
            Ok(())
        })?;
        if let Some(checkpoint) = &checkpoint {
            crate::checkpoint::adopt(root, session, checkpoint, &task)?;
        }
        state::write(&file, &binding, BINDING_BYTES, false)?;
    }
    let state = records::read_state(root, session, &task, recorder, parent)?;
    let repository = snapshot::workspace(root, state["taskRecord"].get("repositories"), false)?;
    let recovery = match &checkpoint {
        Some(saved) => {
            let current = if saved.get("repositories") == state["taskRecord"].get("repositories") {
                repository.clone()
            } else {
                snapshot::workspace(root, saved.get("repositories"), false)?
            };
            craft_engine::checkpoint::compare(saved, &current)
        }
        None => Value::Null,
    };
    let not_carried: Vec<&str> = NOT_CARRIED
        .iter()
        .filter(|(prefix, _)| directory.entries.iter().any(|e| e.name.starts_with(prefix)))
        .map(|(_, family)| *family)
        .collect();
    Ok(json!({
        "session": session,
        "task": task,
        "from": from,
        "adopted": adopted,
        "context": session::context(&state, &repository)?,
        "checkpoint": recovery,
        "notCarried": not_carried,
        "authorization": "Copied the earlier session's task record and checkpoint; the earlier session is unchanged and no authorization was renewed.",
    }))
}
