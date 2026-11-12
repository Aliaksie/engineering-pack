//! Configuration layers beneath the project: what its own files state, an organization
//! baseline and a user baseline. One merge rule for every layer, later wins:
//! bundled < detected < organization < user < project < task.
//! Objects merge key by key; a scalar or a plain list replaces the earlier value whole; a list
//! of entries with an `id` (`ENTRY_LISTS`) merges entry by entry, and an entry
//! `{"id", "disabled": true}` removes the one an earlier layer declared.
//! A layer holds defaults, never enforcement: the project can override every value, and
//! `explanation` shows which layer won and what it hid.
use crate::contracts;
use crate::values::{object, safe_integer};
use serde_json::{Map, Value, json};

/// Merge order, earliest first; a later layer wins.
pub const ORDER: [&str; 6] = [
    "bundled",
    "detected",
    "organization",
    "user",
    "project",
    "task",
];

/// The layer of standards read from the project's own files while it declares none. Its
/// entries may name those files, which a layer written for many projects may not.
pub const DETECTED: &str = "detected";

/// What a baseline layer may set. Everything else describes one repository (its modules,
/// stack, services, permissions and evidence scope) and stays in the project file.
pub const FIELDS: [&str; 12] = [
    "$schema",
    "formatVersion",
    "routing",
    "checkpoint",
    "workspace",
    "efficiency",
    "standards",
    "anchors",
    "skills",
    "safety",
    "execution",
    "review",
];

/// The format marker of a file: not a value a layer sets.
const VERSION: &str = "formatVersion";

/// Lists merged entry by entry on `id`, by dotted field.
pub const ENTRY_LISTS: [&str; 2] = ["standards", "safety.deny"];

type Result<T = ()> = std::result::Result<T, String>;

/// One baseline layer as read: `origin` is its name in `ORDER`, `label` names it and its
/// file in errors.
pub struct Layer<'a> {
    pub origin: &'static str,
    pub label: String,
    pub document: &'a Value,
}

/// Whether `entry` of an id-merged list removes the entry of that id.
pub fn disabled(entry: &Value) -> bool {
    entry["disabled"] == true
}

fn shape(layer: &Value, project_files: bool) -> Result {
    let map = object(layer, &FIELDS, "layer")?;
    if map.get("$schema").is_some_and(|link| !link.is_string()) {
        return Err("$schema must be a string".into());
    }
    if map
        .get("formatVersion")
        .is_some_and(|version| safe_integer(version) != Some(1))
    {
        return Err("unsupported formatVersion".into());
    }
    // Parts that name files or modules of one repository.
    for standard in map
        .get("standards")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        let id = standard["id"].as_str().unwrap_or("?");
        if project_files {
            continue;
        }
        for field in ["file", "folder"] {
            if standard.get(field).is_some() {
                return Err(format!(
                    "standard {id}: {field} names a project file; declare it in the project"
                ));
            }
        }
        if standard["scope"].get("modules").is_some() {
            return Err(format!(
                "standard {id}: scope.modules names project modules; declare it in the project"
            ));
        }
    }
    if map
        .get("anchors")
        .is_some_and(|anchors| anchors.get("catalogs").is_some())
    {
        return Err("anchors.catalogs names project files; declare it in the project".into());
    }
    if map
        .get("skills")
        .is_some_and(|skills| skills.get("declared").is_some())
    {
        return Err("skills.declared names project folders; declare it in the project".into());
    }
    let (mut switches, mut rest) = (Map::new(), Map::new());
    rest.insert("formatVersion".into(), json!(1));
    for (key, value) in map {
        if contracts::SWITCHES.contains(&key.as_str()) {
            switches.insert(key.clone(), value.clone());
        } else if !["$schema", "formatVersion"].contains(&key.as_str()) {
            rest.insert(key.clone(), value.clone());
        }
    }
    contracts::validate_switches(&Value::Object(switches))?;
    contracts::validate_project(&Value::Object(rest))
}

/// A baseline document without the fields this build does not know, and their dotted names.
/// Every installed build reads the same organization and user file, and builds differ in
/// version: a switch or a section a newer build added is left out here and named, so one file
/// serves both. A field that belongs to the project, and a known field with a value it cannot
/// take, are still errors.
pub fn known(document: &Value) -> (Value, Vec<String>) {
    let Some(map) = document.as_object() else {
        return (document.clone(), Vec::new());
    };
    let template = contracts::default_settings();
    let mut ignored = Vec::new();
    let mut kept = Map::new();
    for (key, value) in map {
        let name = key.as_str();
        if !FIELDS.contains(&name) && !contracts::PROJECT_FIELDS.contains(&name) {
            ignored.push(key.clone());
        } else if contracts::SWITCHES.contains(&name) {
            kept.insert(
                key.clone(),
                known_switches(value, &template[name], name, &mut ignored),
            );
        } else {
            kept.insert(key.clone(), value.clone());
        }
    }
    (Value::Object(kept), ignored)
}

fn known_switches(value: &Value, template: &Value, path: &str, ignored: &mut Vec<String>) -> Value {
    let (Some(map), Some(template)) = (value.as_object(), template.as_object()) else {
        return value.clone();
    };
    let mut kept = Map::new();
    for (key, item) in map {
        let field = format!("{path}.{key}");
        match template.get(key) {
            Some(expected) => {
                kept.insert(key.clone(), known_switches(item, expected, &field, ignored));
            }
            None => ignored.push(field),
        }
    }
    Value::Object(kept)
}

/// Validate one layer document on its own; the error starts with the layer's label.
pub fn validate(layer: &Layer) -> Result {
    shape(layer.document, layer.origin == DETECTED)
        .map_err(|error| format!("{}: {error}", layer.label))
}

/// The layer's values as merged: without the editor link and the format version.
pub fn values(document: &Value) -> Map<String, Value> {
    let mut map = document.as_object().cloned().unwrap_or_default();
    map.remove("$schema");
    map.remove("formatVersion");
    map
}

/// Every effective value with the layer it came from and the values that layer hid, from
/// `history` (field → each value it took, in merge order). A row whose value equals the one
/// directly beneath it is `redundant`; a `disabled` entry no earlier layer declares
/// `disablesNothing`.
pub fn explanation(history: &Map<String, Value>) -> Vec<Value> {
    history
        .iter()
        .filter(|(field, _)| *field != VERSION)
        .filter_map(|(field, steps)| {
            let (last, earlier) = steps.as_array()?.split_last()?;
            let mut row = Map::new();
            row.insert("field".into(), json!(field));
            row.insert("origin".into(), last["origin"].clone());
            let off = last.get("disabled").is_some();
            if off {
                row.insert("disabled".into(), json!(true));
                if earlier
                    .last()
                    .is_none_or(|step| step.get("disabled").is_some())
                {
                    row.insert("disablesNothing".into(), json!(true));
                }
            } else {
                row.insert("value".into(), last["value"].clone());
                if earlier
                    .last()
                    .is_some_and(|step| step.get("value") == last.get("value"))
                {
                    row.insert("redundant".into(), json!(true));
                }
            }
            if !earlier.is_empty() {
                row.insert(
                    "overrides".into(),
                    Value::Array(earlier.iter().rev().cloned().collect()),
                );
            }
            Some(Value::Object(row))
        })
        .collect()
}

/// How many values `origin` set, whether or not a later layer hid them.
pub fn contributed(history: &Map<String, Value>, origin: &str) -> usize {
    history
        .iter()
        .filter(|(field, _)| *field != VERSION)
        .filter(|(_, steps)| {
            steps
                .as_array()
                .is_some_and(|steps| steps.iter().any(|step| step["origin"] == origin))
        })
        .count()
}

/// Whether `field` is `prefix` or lies beneath it (`routing`, `standards[api]`).
pub fn beneath(field: &str, prefix: &str) -> bool {
    field
        .strip_prefix(prefix)
        .is_some_and(|rest| rest.is_empty() || rest.starts_with(['.', '[']))
}

fn shown(value: &Value) -> String {
    const MAX: usize = 96;
    let text = value.to_string();
    match text.char_indices().nth(MAX) {
        Some((end, _)) => format!("{}…", &text[..end]),
        None => text,
    }
}

fn step(step: &Value) -> String {
    let origin = step["origin"].as_str().unwrap_or("?");
    match step.get("value") {
        Some(value) => format!("{} ({origin})", shown(value)),
        None => format!("disabled ({origin})"),
    }
}

fn line(row: &Value) -> String {
    let field = row["field"].as_str().unwrap_or("?");
    let mut text = match row.get("value") {
        Some(_) => format!("{field} = {}", step(row)),
        None => format!("{field} {}", step(row)),
    };
    let hidden: Vec<String> = row["overrides"]
        .as_array()
        .into_iter()
        .flatten()
        .map(step)
        .collect();
    if !hidden.is_empty() {
        text.push_str(&format!(" over {}", hidden.join(", ")));
    }
    if row["redundant"] == true {
        text.push_str(" — repeats the value beneath it");
    }
    if row["disablesNothing"] == true {
        text.push_str(" — disables nothing: no earlier layer declares it");
    }
    text
}

/// The `craft config` report as text: the layers, then the values a layer hid, then, when
/// asked for, every effective value.
pub fn text(report: &Value) -> String {
    let mut lines = vec![format!(
        "layers, later wins: {}",
        report["order"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .collect::<Vec<_>>()
            .join(" < ")
    )];
    for layer in report["layers"].as_array().into_iter().flatten() {
        let Some(file) = layer["file"].as_str() else {
            // What the project's files state has no file of its own to name.
            if layer["name"] == DETECTED && layer["values"].as_u64().is_some_and(|n| n > 0) {
                lines.push(format!(
                    "  {DETECTED}: standards read from the project's own files — {} value{} (`craft standards --propose` shows the evidence)",
                    layer["values"],
                    if layer["values"] == 1 { "" } else { "s" },
                ));
            }
            continue;
        };
        lines.push(format!(
            "  {}: {file} — {}",
            layer["name"].as_str().unwrap_or("?"),
            match layer["state"].as_str() {
                Some("loaded") if layer["values"] == 1 => "1 value".to_string(),
                Some("loaded") => format!("{} values", layer["values"]),
                Some(state) => state.to_string(),
                None => "?".to_string(),
            }
        ));
        let ignored: Vec<&str> = layer["ignored"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .collect();
        if !ignored.is_empty() {
            lines.push(format!(
                "    ignored, not known to this build: {}",
                ignored.join(", ")
            ));
        }
    }
    let rows = |name: &str| report[name].as_array().cloned().unwrap_or_default();
    let overrides = rows("overrides");
    if overrides.is_empty() {
        lines.push("no layer overrides another".into());
    } else {
        lines.push("overrides:".into());
        lines.extend(overrides.iter().map(|row| format!("  {}", line(row))));
    }
    if report.get("values").is_some() {
        lines.push("values:".into());
        lines.extend(rows("values").iter().map(|row| format!("  {}", line(row))));
    }
    lines.join("\n")
}

#[cfg(test)]
mod tests;
