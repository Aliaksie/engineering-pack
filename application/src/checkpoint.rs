//! Explicit local checkpoints and read-only recovery comparison.
use crate::cli::Failure;
use crate::session::{active_task, read_record, read_state};
use crate::telemetry::{Operation, Recorder, now_ms};
use craft_engine::{checkpoint, hash, session, timestamp};
use craft_platform::{repository::snapshot, state};
use serde_json::{Map, Value, json};
use std::path::{Path, PathBuf};

const CHECKPOINT_BYTES: usize = 65536;

fn file(root: &Path, session: &str, create: bool) -> Result<PathBuf, Failure> {
    let name = format!("{}.json", &hash(session.as_bytes())[..24]);
    Ok(state::file(root, &name, create, Some(session))?)
}

/// The saved checkpoint, or `Null`.
pub fn read(
    root: &Path,
    session: &str,
    recorder: &mut Recorder,
    parent: &Operation,
) -> Result<Value, Failure> {
    let operation = recorder.start("checkpoint.read", Some(parent));
    let result = (|| -> Result<Value, Failure> {
        let saved = read_record(&file(root, session, false)?, CHECKPOINT_BYTES)?;
        checkpoint::validate(&saved)?;
        Ok(saved)
    })();
    let (attributes, failure) = match &result {
        Ok(saved) => (
            Map::from_iter([("available".to_string(), json!(!saved.is_null()))]),
            None,
        ),
        Err(error) => (Map::new(), Some(error.code("CHECKPOINT_UNAVAILABLE"))),
    };
    recorder.complete(operation, attributes, failure);
    result
}

/// Present and not `null`, as `??` sees a field.
fn given<'a>(map: &'a Map<String, Value>, field: &str) -> Option<&'a Value> {
    map.get(field).filter(|v| !v.is_null())
}

fn truthy(value: Option<&Value>) -> bool {
    match value {
        None | Some(Value::Null | Value::Bool(false)) => false,
        Some(Value::String(s)) => !s.is_empty(),
        Some(Value::Number(n)) => n.as_f64().is_some_and(|f| f != 0.0),
        Some(_) => true,
    }
}

/// Save a checkpoint unless it is unchanged; `{file, state, unchanged}`.
pub fn save(
    root: &Path,
    session: &str,
    metadata: &Value,
    recorder: &mut Recorder,
    parent: &Operation,
) -> Result<Value, Failure> {
    let operation = recorder.start("checkpoint.saved", Some(parent));
    let result = save_locked(root, session, metadata, recorder, &operation);
    let (attributes, failure) = match &result {
        Ok(saved) => {
            let count = if saved["unchanged"] == true { 0 } else { 1 };
            (
                Map::from_iter([("checkpoints".to_string(), json!(count))]),
                None,
            )
        }
        Err(error) => (Map::new(), Some(error.code("CHECKPOINT_UNAVAILABLE"))),
    };
    recorder.complete(operation, attributes, failure);
    result
}

fn save_locked(
    root: &Path,
    session: &str,
    metadata: &Value,
    recorder: &mut Recorder,
    operation: &Operation,
) -> Result<Value, Failure> {
    let mut metadata = metadata
        .as_object()
        .cloned()
        .ok_or("checkpoint metadata must be a JSON object")?;
    checkpoint::validate_metadata(&metadata)?;
    let target = file(root, session, true)?;
    let Some(_lock) = state::lock(&target)? else {
        recorder.report("checkpoint", "STATE_BUSY", Map::new(), operation);
        return Err("checkpoint writer is busy; retry at the next milestone".into());
    };
    let previous = read(root, session, recorder, operation)?;
    let task = match given(&metadata, "taskId") {
        Some(task) => Some(task.clone()),
        None => active_task(root, Some(session))?.map(Value::String),
    };
    let repositories = match given(&metadata, "repositories") {
        Some(repositories) => Some(repositories.clone()),
        None if truthy(task.as_ref()) => {
            let task = task.as_ref().and_then(Value::as_str).unwrap_or_default();
            let state = read_state(root, session, task, recorder, operation)?;
            state["taskRecord"].get("repositories").cloned()
        }
        None => previous.get("repositories").cloned(),
    }
    .filter(|r| truthy(Some(r)));
    let repository = snapshot::workspace(root, repositories.as_ref(), false)?;
    if let Some(repositories) = repositories {
        metadata.insert("repositories".into(), repositories);
    }
    let earlier = if previous.is_null() {
        json!({})
    } else {
        previous.clone()
    };
    let updated = timestamp::format(now_ms());
    let next = checkpoint::state(&earlier, &metadata, &repository, &updated)?;
    if checkpoint::unchanged(&previous, &next) {
        return Ok(json!({"file": target, "state": previous, "unchanged": true}));
    }
    state::write(&target, &next, CHECKPOINT_BYTES, true)?;
    Ok(json!({"file": target, "state": next, "unchanged": false}))
}

/// Saved checkpoint against current source, with the task state as current source sees it.
/// A historical checkpoint cannot restore or certify acceptance.
pub fn recover(
    root: &Path,
    session: &str,
    recorder: &mut Recorder,
    parent: &Operation,
) -> Result<Value, Failure> {
    let operation = recorder.start("checkpoint.recovered", Some(parent));
    let result = (|| -> Result<Value, Failure> {
        let saved = read(root, session, recorder, &operation)?;
        let current = snapshot::workspace(root, saved.get("repositories"), false)?;
        let mut recovered = checkpoint::compare(&saved, &current);
        let current = recovered["current"].clone();
        recovered["taskRecovery"] = task_recovery(
            root, session, &saved, &current, recorder, &operation,
        )
        .unwrap_or_else(|_| {
            recorder.report("state", "STATE_UNAVAILABLE", Map::new(), &operation);
            json!({"status": "unavailable"})
        });
        Ok(recovered)
    })();
    let (attributes, failure) = match &result {
        Ok(recovered) => (
            Map::from_iter([
                (
                    "available".to_string(),
                    json!(!recovered["saved"].is_null()),
                ),
                (
                    "complete".to_string(),
                    recovered["changeDetection"]["complete"].clone(),
                ),
            ]),
            None,
        ),
        Err(error) => (Map::new(), Some(error.code("CHECKPOINT_UNAVAILABLE"))),
    };
    recorder.complete(operation, attributes, failure);
    result
}

fn task_recovery(
    root: &Path,
    session: &str,
    saved: &Value,
    current: &Value,
    recorder: &mut Recorder,
    operation: &Operation,
) -> Result<Value, Failure> {
    let Some(task) = active_task(root, Some(session))? else {
        return Ok(json!({"status": "unavailable"}));
    };
    let state = read_state(root, session, &task, recorder, operation)?;
    let record = &state["taskRecord"];
    if !truthy(Some(record)) {
        return Ok(json!({"status": "unavailable"}));
    }
    let repository = if record.get("repositories") == saved.get("repositories") {
        current.clone()
    } else {
        snapshot::workspace(root, record.get("repositories"), false)?
    };
    let context = session::context(&state, &repository)?;
    let matches = saved.get("taskId") == Some(&json!(task))
        && saved.get("taskContract") == Some(&context["taskContract"]);
    let mut recovery = json!({"status": "available"});
    for (key, value) in context.as_object().unwrap() {
        recovery[key] = value.clone();
    }
    recovery["checkpointTaskMatches"] = json!(matches);
    Ok(recovery)
}
