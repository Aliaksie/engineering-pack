//! Session records shared with the JS session owner until W2 completes: the active task
//! binding, task state reads and locked task state updates.
use crate::cli::{Failure, observe};
use crate::telemetry::{Operation, Recorder};
use craft_engine::{contracts, digest, session};
use craft_platform::{repository::snapshot, state};
use serde_json::{Map, Value, json};
use std::path::{Path, PathBuf};

const BINDING_BYTES: usize = 16384;
const STATE_BYTES: usize = 131_072;

fn short(value: &Value) -> String {
    digest(value)[..24].to_string()
}

fn binding_file(root: &Path, session: &str) -> Result<PathBuf, Failure> {
    session::identity(&json!(session), &json!("active-binding"))?;
    let name = format!("active-{}.json", short(&json!(session)));
    Ok(state::file(root, &name, false, Some(session))?)
}

fn state_file(root: &Path, session: &str, task: &str, create: bool) -> Result<PathBuf, Failure> {
    session::identity(&json!(session), &json!(task))?;
    let name = format!("session-{}.json", short(&json!([session, task])));
    Ok(state::file(root, &name, create, Some(session))?)
}

/// A bounded record as JS reads it: absent or JSON `null` is `Null`; integral floats
/// are integers, so digests and comparisons match `JSON.stringify`.
pub fn read_record(file: &Path, max_bytes: usize) -> Result<Value, Failure> {
    let mut value = state::read(file, max_bytes)?.unwrap_or(Value::Null);
    contracts::normalize_numbers(&mut value);
    Ok(value)
}

/// The task bound to `session` for the current checkout, if any.
pub fn active_task(root: &Path, session: Option<&str>) -> Result<Option<String>, Failure> {
    let Some(session) = session else {
        return Ok(None);
    };
    let saved = read_record(&binding_file(root, session)?, BINDING_BYTES)?;
    let Some(binding) = session::binding(&saved, None)? else {
        return Ok(None);
    };
    let current = snapshot::workspace(root, binding.get("repositories"), true)?;
    Ok(session::binding(&binding, Some(&current["checkout"]))?
        .and_then(|binding| binding["task"].as_str().map(str::to_string)))
}

fn read(file: &Path) -> Result<Value, Failure> {
    let mut state = read_record(file, STATE_BYTES)?;
    if state.is_null() {
        state = session::empty();
    }
    session::validate(&state)?;
    Ok(state)
}

pub fn read_state(
    root: &Path,
    session: &str,
    task: &str,
    recorder: &mut Recorder,
    parent: &Operation,
) -> Result<Value, Failure> {
    let summary = Map::from_iter([("area".to_string(), json!("session"))]);
    observe(
        recorder,
        "state.read",
        parent,
        "STATE_UNAVAILABLE",
        summary,
        |_, _| read(&state_file(root, session, task, false)?),
    )
}

/// Apply `update` to the task state under its lock.
pub fn update_state(
    root: &Path,
    session: &str,
    task: &str,
    recorder: &mut Recorder,
    parent: &Operation,
    update: impl FnOnce(&mut Value) -> Result<(), Failure>,
) -> Result<Value, Failure> {
    let summary = Map::from_iter([("area".to_string(), json!("session"))]);
    observe(
        recorder,
        "state.updated",
        parent,
        "STATE_UNAVAILABLE",
        summary,
        |recorder, operation| {
            let file = state_file(root, session, task, true)?;
            let Some(_lock) = state::lock(&file)? else {
                recorder.report("state", "STATE_BUSY", Map::new(), operation);
                return Err("session state writer is busy".into());
            };
            let mut state = read(&file)?;
            update(&mut state)?;
            state::write(&file, &state, STATE_BYTES, false)?;
            Ok(state)
        },
    )
}

/// Add activity counters to the task state; a failure is reported and returned as a
/// warning, never raised.
pub fn add_activity(
    root: &Path,
    session: &str,
    task: &str,
    values: &Map<String, Value>,
    recorder: &mut Recorder,
    parent: &Operation,
) -> Option<String> {
    let result = update_state(root, session, task, recorder, parent, |state| {
        Ok(session::add_activity(state, values)?)
    });
    match result {
        Ok(_) => None,
        Err(failure) => {
            recorder.report("state", "STATE_UNAVAILABLE", Map::new(), parent);
            Some(failure.message)
        }
    }
}
