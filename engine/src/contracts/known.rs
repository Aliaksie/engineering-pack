//! The fields of a project file this build knows, read from the published project schema.
//! A project file another build wrote can hold a field this one does not know; hooks leave
//! such fields out and name them instead of refusing the whole file, while commands that
//! validate the file still report them.
use serde_json::{Map, Value};

const SCHEMA: &str = include_str!("../../../schemas/project.schema.json");

/// `project` without the fields the schema does not declare, and each left-out field by its
/// place (`standards[2].when`). A field under a choice of shapes is kept as written.
pub fn known_project(project: &Value) -> (Value, Vec<String>) {
    let schema: Value = serde_json::from_str(SCHEMA).expect("bundled project schema");
    let mut ignored = Vec::new();
    let kept = prune(project, &schema, &schema, "", &mut ignored);
    (kept, ignored)
}

fn resolved<'a>(schema: &'a Value, root: &'a Value) -> &'a Value {
    match schema["$ref"]
        .as_str()
        .and_then(|link| link.strip_prefix("#/"))
    {
        Some(pointer) => root.pointer(&format!("/{pointer}")).unwrap_or(&Value::Null),
        None => schema,
    }
}

fn prune(
    value: &Value,
    schema: &Value,
    root: &Value,
    path: &str,
    ignored: &mut Vec<String>,
) -> Value {
    let schema = resolved(schema, root);
    match value {
        Value::Object(map) => {
            let properties = schema["properties"].as_object();
            let other = &schema["additionalProperties"];
            if properties.is_none() && !other.is_object() {
                return value.clone();
            }
            let mut kept = Map::new();
            for (key, item) in map {
                let place = if path.is_empty() {
                    key.clone()
                } else {
                    format!("{path}.{key}")
                };
                match properties.and_then(|properties| properties.get(key)) {
                    Some(field) => {
                        kept.insert(key.clone(), prune(item, field, root, &place, ignored));
                    }
                    None if other.is_object() => {
                        kept.insert(key.clone(), prune(item, other, root, &place, ignored));
                    }
                    None if *other == Value::Bool(false) => ignored.push(place),
                    None => {
                        kept.insert(key.clone(), item.clone());
                    }
                }
            }
            Value::Object(kept)
        }
        Value::Array(items) if schema["items"].is_object() => Value::Array(
            items
                .iter()
                .enumerate()
                .map(|(index, item)| {
                    prune(
                        item,
                        &schema["items"],
                        root,
                        &format!("{path}[{index}]"),
                        ignored,
                    )
                })
                .collect(),
        ),
        _ => value.clone(),
    }
}

/// The line the user is shown once a session starts on a project file with fields this build
/// leaves out: at most three are named.
pub fn ignored_line(ignored: &[String]) -> String {
    let named = ignored
        .iter()
        .take(3)
        .map(|field| format!("`{field}`"))
        .collect::<Vec<_>>()
        .join(", ");
    let more = match ignored.len().saturating_sub(3) {
        0 => String::new(),
        more => format!(" and {more} more"),
    };
    format!(
        "Craft: this version does not know {named}{more} in .craft/project.json and reads the rest of the file without it; `craft validate` reports it."
    )
}
