//! Project configuration use cases: effective settings with value origins, private
//! state placement and the current worker budget.
use crate::failure::Failure;
use crate::telemetry::{Operation, Recorder, now_ms, observe};
use craft_engine::{contracts, values, workers};
use craft_platform::{documents, state};
use serde_json::{Map, Value, json};
use std::path::{Component, Path, PathBuf};

pub fn preference_file() -> PathBuf {
    std::env::home_dir()
        .unwrap_or_default()
        .join(".config")
        .join("craft")
        .join("preferences.json")
}

fn document(read: Result<Value, documents::DocumentError>) -> Result<Value, Failure> {
    let mut value = read?;
    values::normalize_numbers(&mut value);
    Ok(value)
}

pub fn input(file: &Path) -> Result<Value, Failure> {
    document(documents::input(file))
}

/// `input` recorded as an `input.loaded` child operation of `parent`.
pub(crate) fn read_input(
    recorder: &mut Recorder,
    parent: &Operation,
    file: &str,
) -> Result<Value, Failure> {
    observe(
        recorder,
        "input.loaded",
        parent,
        "INVALID_INPUT",
        Map::new(),
        |_, _| input(Path::new(file)),
    )
}

/// `path.resolve`: absolute against the working directory, with `.` and `..` removed lexically.
pub(crate) fn absolute(path: &str) -> PathBuf {
    let joined = std::env::current_dir().unwrap_or_default().join(path);
    let mut resolved = PathBuf::new();
    for component in joined.components() {
        match component {
            Component::ParentDir => {
                resolved.pop();
            }
            Component::CurDir => {}
            other => resolved.push(other),
        }
    }
    resolved
}

/// Paths in the form `stateLocation` reports them; failures are reported, not raised.
fn state_location(root: &Path) -> Value {
    let located = state::canonical_root(root)
        .and_then(|canonical| Ok((state::location(&canonical, None)?, canonical)));
    match located {
        Ok((location, canonical)) => json!({
            "directory": location.directory.to_string_lossy(),
            "scope": "workspace-local",
            "formatVersion": 2,
            "worktreeId": location.worktree_id,
            "sessionId": location.session_id,
            "legacyDirectory": canonical.join(".craft").join("state").to_string_lossy(),
        }),
        Err(reason) => json!({ "status": "unavailable", "reason": reason.to_string() }),
    }
}

pub struct Settings<'a> {
    pub preferences: Option<&'a Path>,
    pub overrides: Value,
    /// Fresh worker observations and the session they must belong to.
    pub workers: Option<(Value, Option<&'a str>)>,
}

fn resolve(root: &Path, preferences: Option<&Path>, overrides: &Value) -> Result<Value, Failure> {
    let project = document(documents::project(root))?;
    let preferences = document(documents::preferences(
        preferences
            .map_or_else(preference_file, Path::to_path_buf)
            .as_path(),
    ))?;
    Ok(contracts::effective_settings(
        &project,
        &preferences,
        overrides,
    )?)
}

/// Validated effective settings without task overrides, as commands consume them.
pub fn settings(root: &Path, preferences: Option<&Path>) -> Result<Value, Failure> {
    Ok(resolve(root, preferences, &json!({}))?["settings"].take())
}

/// `settings` recorded as a `project.loaded` child operation of `parent`.
pub(crate) fn loaded(
    recorder: &mut Recorder,
    parent: &Operation,
    root: &Path,
    preferences: Option<&Path>,
) -> Result<Value, Failure> {
    observe(
        recorder,
        "project.loaded",
        parent,
        "CONFIG_INVALID",
        Map::new(),
        |_, _| settings(root, preferences),
    )
}

pub fn effective_settings(root: &Path, request: Settings) -> Result<Value, Failure> {
    let mut result = resolve(root, request.preferences, &request.overrides)?;
    let (facts, session) = request.workers.unwrap_or((json!({}), None));
    let budget = workers::budget(&result["settings"], &facts, session, now_ms())?;
    result["state"] = state_location(root);
    result["workers"] = budget;
    Ok(result)
}
