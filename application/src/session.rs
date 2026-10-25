//! Session records shared with the remaining JS callers: the active task
//! binding, task state reads and locked task state updates.
use crate::cli::{Failure, observe};
use crate::telemetry::{Operation, Recorder};
use craft_engine::{contracts, digest, session};
use craft_platform::{repository::snapshot, state};
use serde_json::{Map, Value, json};
use std::path::{Path, PathBuf};

pub(crate) const BINDING_BYTES: usize = 16384;
const STATE_BYTES: usize = 131_072;

fn short(value: &Value) -> String {
    digest(value)[..24].to_string()
}

pub(crate) fn binding_file(root: &Path, session: &str, create: bool) -> Result<PathBuf, Failure> {
    session::identity(&json!(session), &json!("active-binding"))?;
    let name = format!("active-{}.json", short(&json!(session)));
    Ok(state::file(root, &name, create, Some(session))?)
}

pub(crate) fn state_file(
    root: &Path,
    session: &str,
    task: &str,
    create: bool,
) -> Result<PathBuf, Failure> {
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
    let saved = read_record(&binding_file(root, session, false)?, BINDING_BYTES)?;
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

/// Append one attempt receipt bound to the task's current source; the attempt budget is
/// the profile's, else the project default. Returns the updated state.
pub fn record_attempt(
    root: &Path,
    session: &str,
    task: &str,
    attempt: &Value,
    settings: &Value,
    recorder: &mut Recorder,
    parent: &Operation,
) -> Result<Value, Failure> {
    let profile = attempt.get("profile").and_then(Value::as_str);
    let limit = profile
        .and_then(|profile| settings["execution"]["profiles"][profile].get("maxAttempts"))
        .filter(|limit| !limit.is_null())
        .or_else(|| settings.pointer("/efficiency/attempts/maxAttempts"));
    let mut bounded = Map::new();
    if let Some(limit) = limit {
        bounded.insert("maxAttempts".into(), limit.clone());
    }
    if let Some(fields) = attempt.as_object() {
        bounded.extend(fields.clone());
    }
    let attempt = if attempt.is_object() {
        Value::Object(bounded)
    } else {
        attempt.clone()
    };
    let state = read_state(root, session, task, recorder, parent)?;
    let repository = snapshot::workspace(root, state["taskRecord"].get("repositories"), false)?;
    let id = craft_platform::telemetry::uuid()?;
    let at = craft_engine::timestamp::format(crate::telemetry::now_ms());
    let receipt = session::attempt_receipt(&attempt, &repository, &id, &at)?;
    update_state(root, session, task, recorder, parent, |state| {
        Ok(session::append_attempt(state, receipt)?)
    })
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

/// The task's feature report, after saving the agent declaration `input` when given. A
/// declaration needs the active binding, held for the update.
pub fn features(
    root: &Path,
    session: &str,
    task: &str,
    input: Option<&Value>,
    recorder: &mut Recorder,
    parent: &Operation,
) -> Result<Value, Failure> {
    session::identity(&json!(session), &json!(task))?;
    let source = |state: &Value| -> Result<Value, Failure> {
        let repository = snapshot::workspace(root, state["taskRecord"].get("repositories"), false)?;
        Ok(session::source(&repository))
    };
    let Some(input) = input else {
        let state = read_state(root, session, task, recorder, parent)?;
        return Ok(session::feature_report(&state, &source(&state)?)?);
    };
    let Some(_lock) = state::lock(&binding_file(root, session, true)?)? else {
        return Err("active task writer is busy".into());
    };
    if active_task(root, Some(session))?.as_deref() != Some(task) {
        return Err("feature outcome requires the active task binding".into());
    }
    let source = source(&read_state(root, session, task, recorder, parent)?)?;
    let at = craft_engine::timestamp::format(crate::telemetry::now_ms());
    let mut recorded = Value::Null;
    let state = update_state(root, session, task, recorder, parent, |state| {
        recorded = session::record_feature(state, input, &source, &at, "agent-declared")?;
        Ok(())
    })?;
    let mut result = Map::from_iter([("recorded".to_string(), recorded)]);
    if let Value::Object(report) = session::feature_report(&state, &source)? {
        result.extend(report);
    }
    Ok(Value::Object(result))
}
