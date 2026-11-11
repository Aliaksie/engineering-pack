//! Project configuration use cases: effective settings with value origins, private
//! state placement and the current worker budget.
use crate::failure::Failure;
use crate::telemetry::{Operation, Recorder, now_ms, observe};
use craft_engine::layers::{self, Layer};
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

/// Where the organization's configuration is read from: the file `CRAFT_ORGANIZATION_CONFIG`
/// names (an empty value means none), else the location an administrator manages on this
/// platform. `None` when there is none to read.
pub fn organization_file() -> Result<Option<PathBuf>, Failure> {
    match std::env::var_os(ORGANIZATION_VARIABLE) {
        Some(path) if path.is_empty() => Ok(None),
        Some(path) => {
            let path = PathBuf::from(path);
            match path.is_absolute() {
                true => Ok(Some(path)),
                false => Err(format!("{ORGANIZATION_VARIABLE} must be an absolute path").into()),
            }
        }
        None => Ok(managed_file()),
    }
}

pub const ORGANIZATION_VARIABLE: &str = "CRAFT_ORGANIZATION_CONFIG";

#[cfg(target_os = "macos")]
fn managed_file() -> Option<PathBuf> {
    Some(PathBuf::from(
        "/Library/Application Support/Craft/organization.json",
    ))
}

/// Windows has no default: its shared data folder lets any account create the file, and
/// ownership cannot be checked there, so the organization names the file explicitly.
#[cfg(windows)]
fn managed_file() -> Option<PathBuf> {
    None
}

#[cfg(not(any(target_os = "macos", windows)))]
fn managed_file() -> Option<PathBuf> {
    Some(PathBuf::from("/etc/craft/organization.json"))
}

/// The layer files of a request: `None` reads the default location of each.
#[derive(Clone, Copy, Default)]
pub struct Sources<'a> {
    pub organization: Option<&'a Path>,
    pub preferences: Option<&'a Path>,
}

pub struct Settings<'a> {
    pub preferences: Option<&'a Path>,
    pub organization: Option<&'a Path>,
    pub overrides: Value,
    /// Fresh worker observations and the session they must belong to.
    pub workers: Option<(Value, Option<&'a str>)>,
}

/// A baseline layer as read: its file, when there is one to read, and its document.
struct Read {
    origin: &'static str,
    file: Option<PathBuf>,
    document: Value,
}

impl Read {
    fn label(&self) -> String {
        let file = self.file.as_deref().unwrap_or(Path::new(""));
        format!("{} layer {}", self.origin, file.display())
    }

    fn summary(&self, history: &Map<String, Value>) -> Value {
        let state = match &self.file {
            None => "none",
            Some(file) if fs_exists(file) => "loaded",
            Some(_) => "absent",
        };
        json!({
            "name": self.origin,
            "file": self.file.as_ref().map(|file| file.to_string_lossy()),
            "state": state,
            "values": layers::contributed(history, self.origin),
        })
    }
}

fn fs_exists(file: &Path) -> bool {
    std::fs::symlink_metadata(file).is_ok()
}

/// The organization and user layers, earliest first. A file that cannot be read or parsed is
/// an error naming the layer and the file.
fn baseline(sources: Sources) -> Result<[Read; 2], Failure> {
    let organization = match sources.organization {
        Some(file) => Some(file.to_path_buf()),
        None => organization_file()?,
    };
    let user = sources
        .preferences
        .map_or_else(preference_file, Path::to_path_buf);
    let mut layers = [
        Read {
            origin: "organization",
            file: organization,
            document: json!({}),
        },
        Read {
            origin: "user",
            file: Some(user),
            document: json!({}),
        },
    ];
    for layer in &mut layers {
        let Some(file) = &layer.file else { continue };
        let read = match layer.origin {
            "organization" => documents::organization(file),
            _ => documents::preferences(file),
        };
        layer.document = document(read).map_err(|error| error.about(&layer.label()))?;
    }
    Ok(layers)
}

fn project_file(root: &Path) -> PathBuf {
    root.join(".craft").join("project.json")
}

fn resolve(
    root: &Path,
    sources: Sources,
    overrides: &Value,
    explain: bool,
) -> Result<(Value, [Read; 2]), Failure> {
    let named = |error: Failure| error.about(&project_file(root).to_string_lossy());
    let project = document(documents::project(root)).map_err(named)?;
    let baseline = baseline(sources)?;
    // The catalog files are read only once the project names them validly.
    contracts::validate_project(&project).map_err(|error| named(error.into()))?;
    let catalogs = contracts::anchor_catalogs(&project)
        .into_iter()
        .map(|path| {
            let document = document(documents::anchor_catalog(root, &path))?;
            Ok((path, document))
        })
        .collect::<Result<Vec<_>, Failure>>()?;
    // Only the front matter of declared skills: the host lists and loads the skill itself.
    let skills = craft_engine::skills::declared_files(&project)
        .into_iter()
        .map(|path| {
            let header = documents::skill_header(root, &path, craft_engine::skills::MAX_HEADER);
            (path, header)
        })
        .collect();
    let documents = contracts::Documents { catalogs, skills };
    let layers: Vec<Layer> = baseline
        .iter()
        .map(|layer| Layer {
            origin: layer.origin,
            label: layer.label(),
            document: &layer.document,
        })
        .collect();
    let settings = contracts::layered_settings(&project, &layers, overrides, &documents, explain)?;
    drop(layers);
    Ok((settings, baseline))
}

/// Validated effective settings without task overrides, as commands consume them.
pub fn settings(root: &Path, preferences: Option<&Path>) -> Result<Value, Failure> {
    let sources = Sources {
        preferences,
        ..Sources::default()
    };
    Ok(resolve(root, sources, &json!({}), false)?.0["settings"].take())
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
    let sources = Sources {
        organization: request.organization,
        preferences: request.preferences,
    };
    let (mut result, _) = resolve(root, sources, &request.overrides, false)?;
    let (facts, session) = request.workers.unwrap_or((json!({}), None));
    let budget = workers::budget(&result["settings"], &facts, session, now_ms())?;
    result["state"] = state_location(root);
    result["workers"] = budget;
    result["features"] = features(root);
    Ok(result)
}

/// `craft config`: the layers a project's settings come from, each value one layer set over
/// another and, with `explain`, every effective value with its layer. `field` keeps the rows
/// at or beneath one dotted field.
pub fn layers(
    root: &Path,
    sources: Sources,
    explain: bool,
    field: Option<&str>,
) -> Result<Value, Failure> {
    let (mut result, baseline) = resolve(root, sources, &json!({}), true)?;
    let history = match result["history"].take() {
        Value::Object(history) => history,
        _ => Map::new(),
    };
    let project = project_file(root);
    let mut read = vec![json!({
        "name": "bundled",
        "values": layers::contributed(&history, "bundled"),
    })];
    read.extend(baseline.iter().map(|layer| layer.summary(&history)));
    read.push(json!({
        "name": "project",
        "file": project.to_string_lossy(),
        "state": if fs_exists(&project) { "loaded" } else { "absent" },
        "values": layers::contributed(&history, "project"),
    }));
    let rows: Vec<Value> = layers::explanation(&history)
        .into_iter()
        .filter(|row| {
            let name = row["field"].as_str().unwrap_or_default();
            field.is_none_or(|field| layers::beneath(name, field))
        })
        .collect();
    // What a reader acts on: a value set over a declared one, or an entry that changes nothing.
    let overrides: Vec<&Value> = rows
        .iter()
        .filter(|row| {
            row["disablesNothing"] == true
                || row["overrides"]
                    .as_array()
                    .is_some_and(|hidden| hidden.iter().any(|step| step["origin"] != "bundled"))
        })
        .collect();
    let mut report = json!({
        "order": &layers::ORDER[..4],
        "layers": read,
        "overrides": overrides,
        "rule": "A later layer wins field by field; lists are replaced whole, except standards and safety.deny, which merge by id. Layers are defaults, not enforcement.",
    });
    if explain {
        report["values"] = json!(rows);
    }
    Ok(report)
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
