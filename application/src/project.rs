//! Project configuration use cases: effective settings with value origins, private
//! state placement and the current worker budget.
use crate::failure::Failure;
use crate::telemetry::{Operation, Recorder, now_ms, observe, valid_session};
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

/// The validated project configuration, for commands that read it without settings.
pub fn configuration(root: &Path) -> Result<Value, Failure> {
    let project = document(documents::project(root))?;
    contracts::validate_project(&project)?;
    Ok(project)
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
    let located =
        state::canonical_root(root).and_then(|canonical| state::location(&canonical, None));
    match located {
        Ok(location) => json!({
            "directory": location.directory.to_string_lossy(),
            "scope": "workspace-local",
            "formatVersion": 4,
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

/// The session record of the settings a hook last recorded as `project.loaded`.
const RECORDED: &str = "settings.json";
const RECORDED_BYTES: usize = 1024;

/// `settings` for a host hook. Hooks load them on every event, so a success is recorded as
/// `project.loaded` only when the session has not yet recorded these effective settings;
/// a failure is always recorded and makes the next success record again.
pub(crate) fn hook_loaded(
    recorder: &mut Recorder,
    parent: &Operation,
    root: &Path,
    session: Option<&str>,
) -> Result<Value, Failure> {
    let session = session.filter(|session| valid_session(session) && recorder.enabled());
    let Some(session) = session else {
        return loaded(recorder, parent, root, None);
    };
    let operation = recorder.start("project.loaded", Some(parent));
    let file = state::file(root, RECORDED, false, Some(session)).ok();
    let loaded = settings(root, None);
    let digest = match &loaded {
        Ok(settings) => craft_engine::digest(&craft_engine::canonical(settings)),
        Err(error) => {
            if let Some(file) = &file {
                let _ = std::fs::remove_file(file);
            }
            recorder.complete(operation, Map::new(), Some(error.code("CONFIG_INVALID")));
            return loaded;
        }
    };
    let recorded = file
        .as_deref()
        .and_then(|file| state::read(file, RECORDED_BYTES).ok().flatten())
        .is_some_and(|record| record["formatVersion"] == 1 && record["digest"] == digest);
    if !recorded {
        recorder.complete(operation, Map::new(), None);
        // Best effort: without the record the next hook records the settings again.
        let record = json!({ "formatVersion": 1, "digest": digest });
        let _ = state::file(root, RECORDED, true, Some(session))
            .and_then(|file| state::write(&file, &record, RECORDED_BYTES, false));
    }
    loaded
}

pub fn effective_settings(root: &Path, request: Settings) -> Result<Value, Failure> {
    let mut result = resolve(root, request.preferences, &request.overrides)?;
    let (facts, session) = request.workers.unwrap_or((json!({}), None));
    let budget = workers::budget(&result["settings"], &facts, session, now_ms())?;
    result["state"] = state_location(root);
    result["workers"] = budget;
    Ok(result)
}
