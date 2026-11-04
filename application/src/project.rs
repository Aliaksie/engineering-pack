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

/// `settings` for a host hook. Hooks load them on every event, so a success is recorded as
/// `project.loaded` only when the session has not yet recorded these effective settings;
/// a failure is always recorded and makes the next success record again.
pub(crate) fn hook_loaded(
    recorder: &mut Recorder,
    parent: &Operation,
    root: &Path,
) -> Result<Value, Failure> {
    let operation = recorder.start("project.loaded", Some(parent));
    let loaded = settings(root, None);
    match &loaded {
        Ok(settings) => {
            let digest = craft_engine::digest(&craft_engine::canonical(settings));
            if recorder.settings_changed(Some(&digest)) {
                recorder.complete(operation, Map::new(), None);
            }
        }
        Err(error) => {
            recorder.settings_changed(None);
            recorder.complete(operation, Map::new(), Some(error.code("CONFIG_INVALID")));
        }
    }
    loaded
}

pub fn effective_settings(root: &Path, request: Settings) -> Result<Value, Failure> {
    let mut result = resolve(root, request.preferences, &request.overrides)?;
    let (facts, session) = request.workers.unwrap_or((json!({}), None));
    let budget = workers::budget(&result["settings"], &facts, session, now_ms())?;
    result["state"] = state_location(root);
    result["workers"] = budget;
    result["features"] = features(root);
    Ok(result)
}

/// Switches read outside the merged settings, with where each value came from: Project
/// Intelligence from `.craft/project.json`, the background index warm-up from the host's
/// environment, telemetry from `.craft/telemetry.json`.
fn features(root: &Path) -> Value {
    let configured = configuration(root).ok().and_then(|project| {
        project
            .get("evidence")
            .and_then(|evidence| evidence.get("enabled"))
            .and_then(Value::as_bool)
    });
    let evidence = configured.unwrap_or(true);
    let warm_off = std::env::var_os("CRAFT_EVIDENCE_WARM").is_some_and(|value| value == "0");
    json!({
        "projectIntelligence": {
            "enabled": evidence,
            "origin": if configured.is_some() { "project" } else { "bundled" },
        },
        "backgroundWarm": {
            "enabled": evidence && !warm_off && !cfg!(windows),
            "origin": if warm_off { "environment" } else if cfg!(windows) { "platform" } else { "bundled" },
        },
        "telemetry": crate::telemetry::effective(root),
    })
}
