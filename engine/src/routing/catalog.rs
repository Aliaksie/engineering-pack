//! The playbook catalog: intents, extension inference and the guidance entries routes select.
//! The catalog ships inside the executable; it is validated once, on first use.
use crate::{
    contracts::{LANGUAGES, LAYERS, kebab, relative_file},
    values::{js_blank, object},
};
use serde_json::{Map, Value};
use std::collections::HashSet;
use std::sync::OnceLock;

type Result<T = ()> = std::result::Result<T, String>;

pub(crate) const SOURCE: &str = include_str!("../../routing/catalog.json");
const KINDS: [&str; 7] = [
    "workflow",
    "practice",
    "language",
    "runtime",
    "framework",
    "provider",
    "layer",
];
const DIMENSIONS: [&str; 8] = [
    "intents",
    "risks",
    "languages",
    "runtimes",
    "frameworks",
    "providers",
    "layers",
    "paths",
];
pub const VALUE_FIELDS: [&str; 7] = [
    "problem",
    "contribution",
    "inputs",
    "result",
    "boundaries",
    "consumer",
    "evidence",
];
const ENTRY_FIELDS: [&str; 9] = [
    "id",
    "skills",
    "fallbackSkills",
    "references",
    "when",
    "value",
    "unavailable",
    "documentationQuestion",
    "contextOnly",
];

/// The validated embedded catalog.
pub fn catalog() -> Result<&'static Value> {
    static CATALOG: OnceLock<Result<Value>> = OnceLock::new();
    CATALOG
        .get_or_init(|| {
            let catalog = serde_json::from_str(SOURCE).map_err(|e| e.to_string())?;
            validate(&catalog)?;
            Ok(catalog)
        })
        .as_ref()
        .map_err(Clone::clone)
}

/// Skill identifiers the catalog can select, which telemetry may record by name.
pub fn skills(catalog: &Value) -> Vec<&str> {
    entries(catalog)
        .flat_map(|entry| entry["skills"].as_array().into_iter().flatten())
        .filter_map(Value::as_str)
        .collect()
}

pub(super) fn entries(catalog: &Value) -> impl Iterator<Item = &Value> {
    catalog["entries"].as_array().into_iter().flatten()
}

/// Whether `catalog[field]` lists `value`.
pub(super) fn lists(catalog: &Value, field: &str, value: &str) -> bool {
    catalog[field]
        .as_array()
        .is_some_and(|items| items.iter().any(|item| item == value))
}

fn text<'a>(value: &'a Value, label: &str) -> Result<&'a str> {
    value
        .as_str()
        .filter(|text| !js_blank(text))
        .ok_or_else(|| format!("{label} must be nonempty text"))
}

fn skill(text: &str) -> bool {
    text.split_once(':')
        .is_some_and(|(package, name)| kebab(package) && kebab(name))
}

/// `package:skill/references/[dir/]*name.md`.
fn reference(text: &str) -> bool {
    let Some((target, path)) = text.split_once("/references/") else {
        return false;
    };
    let Some(path) = path.strip_suffix(".md") else {
        return false;
    };
    skill(target) && path.split('/').all(kebab)
}

fn identity(text: &str) -> bool {
    text.split_once(':')
        .is_some_and(|(kind, name)| KINDS.contains(&kind) && kebab(name))
}

fn strings<'a>(
    value: &'a Value,
    label: &str,
    empty: bool,
    valid: impl Fn(&str) -> bool,
) -> Result<Vec<&'a str>> {
    let items = value
        .as_array()
        .filter(|items| empty || !items.is_empty())
        .ok_or_else(|| {
            format!(
                "{label} must be {} array",
                if empty { "an" } else { "a nonempty" }
            )
        })?;
    let mut seen = Vec::with_capacity(items.len());
    for item in items {
        let item = text(item, label)?;
        if !valid(item) {
            return Err(format!("invalid {label}: {item}"));
        }
        if seen.contains(&item) {
            return Err(format!("duplicate {label}"));
        }
        seen.push(item);
    }
    Ok(seen)
}

fn mappings(catalog: &Map<String, Value>) -> Result {
    for field in ["extensions", "layerExtensions", "runtimeImplications"] {
        let mapping = catalog
            .get(field)
            .and_then(Value::as_object)
            .ok_or_else(|| format!("{field} must be an object"))?;
        for (key, value) in mapping {
            let value = text(value, field)?;
            let key_valid = if field == "runtimeImplications" {
                LANGUAGES.contains(&key.as_str())
            } else {
                key.strip_prefix('.').is_some_and(|rest| {
                    !rest.is_empty()
                        && rest
                            .bytes()
                            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
                })
            };
            if !key_valid {
                return Err(format!("invalid {field} key: {key}"));
            }
            let value_valid = match field {
                "extensions" => LANGUAGES.contains(&value),
                "layerExtensions" => LAYERS.contains(&value),
                _ => kebab(value),
            };
            if !value_valid {
                return Err(format!("invalid {field} value: {value}"));
            }
        }
    }
    Ok(())
}

fn entry(entry: &Value, catalog: &Value, ids: &mut HashSet<String>) -> Result {
    let map = object(entry, &ENTRY_FIELDS, "entry")?;
    let id = entry["id"].as_str().unwrap_or_default();
    if !identity(id) || !ids.insert(id.to_string()) {
        return Err(format!("invalid or duplicate logical id: {}", entry["id"]));
    }
    let skills = strings(&entry["skills"], &format!("{id} skills"), true, skill)?;
    if let Some(fallbacks) = map.get("fallbackSkills") {
        let fallbacks = fallbacks
            .as_object()
            .ok_or_else(|| format!("{id} fallbackSkills must be an object"))?;
        for (primary, fallback) in fallbacks {
            let fallback = fallback.as_str().unwrap_or_default();
            if !skills.contains(&primary.as_str()) || !skill(fallback) || primary == fallback {
                return Err(format!("{id} invalid skill fallback"));
            }
        }
    }
    let references = strings(
        &entry["references"],
        &format!("{id} references"),
        true,
        reference,
    )?;
    let selector = object(&entry["when"], &DIMENSIONS, &format!("{id} selector"))?;
    if selector.is_empty() {
        return Err(format!("{id} requires selectors"));
    }
    let intents: Vec<&str> = catalog["intents"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .collect();
    for (dimension, values) in selector {
        let label = format!("{id} {dimension}");
        let values = strings(values, &label, false, |value| match dimension.as_str() {
            "paths" => true,
            "intents" => intents.contains(&value) && kebab(value),
            "languages" => LANGUAGES.contains(&value) && kebab(value),
            "layers" => LAYERS.contains(&value) && kebab(value),
            _ => kebab(value),
        })?;
        if dimension == "paths" {
            for value in values {
                let normalized = relative_file(&Value::from(value)).ok();
                if normalized.as_deref() != Some(value)
                    || value.contains(['*', '?', '[', ']', '{', '}'])
                {
                    return Err(format!("invalid selector path: {value}"));
                }
            }
        }
    }
    if id.starts_with("workflow:") && (!selector.contains_key("intents") || selector.len() != 1) {
        return Err(format!("{id} workflow requires only an intent selector"));
    }
    let specialization = !id.starts_with("workflow:") && !id.starts_with("practice:");
    let selects_non_stack = selector
        .get("intents")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .any(|intent| !lists(catalog, "stackIntents", intent));
    if specialization && selects_non_stack {
        return Err(format!(
            "{id} specialization cannot select non-stack intents"
        ));
    }
    let value = object(&entry["value"], &VALUE_FIELDS, &format!("{id} value"))?;
    for field in VALUE_FIELDS {
        text(
            value.get(field).unwrap_or(&Value::Null),
            &format!("{id} value.{field}"),
        )?;
    }
    for field in ["unavailable", "documentationQuestion"] {
        if let Some(value) = map.get(field) {
            text(value, &format!("{id} {field}"))?;
        }
    }
    let context_only = map.get("contextOnly");
    if context_only.is_some_and(|value| !value.is_boolean()) {
        return Err(format!("{id} contextOnly must be boolean"));
    }
    if skills.is_empty()
        && references.is_empty()
        && !map.contains_key("unavailable")
        && context_only != Some(&Value::Bool(true))
    {
        return Err(format!(
            "{id} requires load targets, unavailable guidance or explicit contextOnly"
        ));
    }
    Ok(())
}

/// Shape validation of a catalog document; targets are resolved by authoring tooling.
pub fn validate(catalog: &Value) -> Result {
    let map = object(
        catalog,
        &[
            "formatVersion",
            "intents",
            "stackIntents",
            "extensions",
            "layerExtensions",
            "runtimeImplications",
            "entries",
        ],
        "catalog",
    )?;
    if catalog["formatVersion"] != 1 {
        return Err("unsupported catalog formatVersion".into());
    }
    let intents = strings(&catalog["intents"], "intents", false, kebab)?;
    strings(&catalog["stackIntents"], "stackIntents", false, |intent| {
        intents.contains(&intent)
    })?;
    mappings(map)?;
    let entries = catalog["entries"]
        .as_array()
        .filter(|entries| !entries.is_empty())
        .ok_or("entries must be a nonempty array")?;
    let mut ids = HashSet::new();
    for item in entries {
        entry(item, catalog, &mut ids)?;
    }
    let implied = catalog["runtimeImplications"]
        .as_object()
        .into_iter()
        .flatten();
    for (_, runtime) in implied {
        let runtime = runtime.as_str().unwrap_or_default();
        if !ids.contains(&format!("runtime:{runtime}")) {
            return Err(format!("unresolved implied runtime: {runtime}"));
        }
    }
    Ok(())
}
