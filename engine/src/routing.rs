//! Module selection over validated effective settings: which declared modules own a set of
//! project-relative files. Declarations are not verified against the actual project.
use crate::contracts::relative_file;
use serde_json::{Value, json};

type Result<T> = std::result::Result<T, String>;

const DECLARED: &str = "declarations-only; verify paths and checks in the actual project";
const PRIVATE: &str = "private workspace excluded";

/// Private state and Git internals, which no lookup, route or review may select.
pub fn private_path(path: &str) -> bool {
    let parts: Vec<&str> = path.split('/').collect();
    parts.contains(&".git")
        || parts
            .windows(2)
            .any(|pair| [".craft", ".capabilities"].contains(&pair[0]) && pair[1] == "state")
}

fn member(file: &str, directory: &str) -> bool {
    directory == "."
        || file == directory
        || file
            .strip_prefix(directory)
            .is_some_and(|rest| rest.starts_with('/'))
}

fn modules(settings: &Value) -> &[Value] {
    settings
        .get("modules")
        .and_then(Value::as_array)
        .map_or(&[], Vec::as_slice)
}

/// A declared module with its path normalized; settings are validated, so the path is too.
fn normalized(module: &Value) -> Value {
    let mut module = module.clone();
    module["path"] = json!(relative_file(&module["path"]).expect("validated module path"));
    module
}

/// Modules owning each file: the longest matching declared paths, in file then declaration
/// order, each module once.
pub fn matching_modules(settings: &Value, files: &[String]) -> Vec<Value> {
    let declared: Vec<Value> = modules(settings).iter().map(normalized).collect();
    let mut selected: Vec<Value> = Vec::new();
    for file in files {
        let matches: Vec<&Value> = declared
            .iter()
            .filter(|module| member(file, module["path"].as_str().unwrap_or_default()))
            .collect();
        let longest = matches
            .iter()
            .map(|module| module["path"].as_str().unwrap_or_default().len())
            .max();
        for module in matches {
            if Some(module["path"].as_str().unwrap_or_default().len()) == longest
                && !selected.iter().any(|item| item["name"] == module["name"])
            {
                selected.push(module.clone());
            }
        }
    }
    selected
}

/// Declared modules for `files`, for one `module` by name, or all of them. Private files
/// are excluded; when every given file is private, nothing is selected.
pub fn lookup(settings: &Value, files: &[Value], module: Option<&str>) -> Result<Value> {
    let result = |modules: Vec<Value>, availability: &str| json!({ "modules": modules, "availability": availability, "execution": "deterministic-lookup" });
    if let Some(name) = module {
        if !files.is_empty() {
            return Err("choose --file or --module for lookup".into());
        }
        let selected: Vec<Value> = modules(settings)
            .iter()
            .filter(|entry| entry["name"] == name)
            .map(normalized)
            .collect();
        if selected.is_empty() {
            return Err(format!("unknown module: {name}"));
        }
        return Ok(result(selected, DECLARED));
    }
    if files.is_empty() {
        return Ok(result(
            modules(settings).iter().map(normalized).collect(),
            DECLARED,
        ));
    }
    let files = files
        .iter()
        .map(relative_file)
        .collect::<Result<Vec<_>>>()?;
    let public: Vec<String> = files
        .into_iter()
        .filter(|file| !private_path(file))
        .collect();
    if public.is_empty() {
        return Ok(result(Vec::new(), PRIVATE));
    }
    Ok(result(matching_modules(settings, &public), DECLARED))
}

#[cfg(test)]
mod tests;
