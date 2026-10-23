use craft_engine::{hash, identity};
use craft_platform::repository::{eligible, now, read_source};
use serde_json::{Value, json};
use std::{collections::HashSet, path::PathBuf};

/// Explicit members share a response, never a database or an atomic snapshot.
pub fn run(operation: &str, input: &Value) -> Result<Value, String> {
    if operation == "refresh" {
        super::budget::Budget::from_input(input)?;
    }
    let members = input["members"]
        .as_array()
        .ok_or("members must be an array")?;
    if members.is_empty() || members.len() > 32 {
        return Err("select between 1 and 32 members".into());
    }
    let mut ids = HashSet::new();
    let mut roots = HashSet::new();
    let mut selected = Vec::new();
    // Validate the whole selection before refreshing any member.
    for member in members {
        let id = member["id"].as_str().ok_or("member id required")?;
        if id.is_empty()
            || id.len() > 128
            || !id
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"_-".contains(&b))
            || !ids.insert(id)
        {
            return Err("member ids must be unique ASCII names".into());
        }
        let root = PathBuf::from(member["root"].as_str().ok_or("member root required")?);
        if !root.is_absolute() {
            return Err("member roots must be absolute".into());
        }
        let canonical = root.canonicalize().ok();
        if let Some(ref root) = canonical
            && !roots.insert(root.clone())
        {
            return Err("duplicate member root".into());
        }
        selected.push((id, canonical));
    }
    for (_, a) in &selected {
        for (_, b) in &selected {
            if let (Some(a), Some(b)) = (a, b)
                && a != b
                && a.starts_with(b)
            {
                return Err("overlapping member roots are ambiguous".into());
            }
        }
    }
    let mut reports = Vec::new();
    for (id, root) in &selected {
        let Some(root) = root else {
            reports.push(json!({"member":id,"state":"unavailable"}));
            continue;
        };
        let mut request = input.clone();
        request
            .as_object_mut()
            .ok_or("input must be an object")?
            .remove("members");
        request["root"] = json!(root);
        request.as_object_mut().unwrap().remove("database");
        match super::evidence::run(operation, &request) {
            Ok(mut result) => {
                if let Some(rows) = result["results"].as_array_mut() {
                    for row in rows {
                        row["member"] = json!(id);
                        row["workspaceId"] =
                            json!(identity(&[id, row["id"].as_str().unwrap_or("")]));
                        let specifier = row["evidence"]["target"].as_str().unwrap_or("");
                        if !specifier.starts_with('.') {
                            continue;
                        }
                        let source = row["path"].as_str().ok_or("missing source path")?;
                        let target = root
                            .join(source)
                            .parent()
                            .ok_or("missing parent")?
                            .join(specifier);
                        row["resolution"] =
                            json!({"state":"unresolved","rule":"explicit-relative-file"});
                        if target.ancestors().any(|p| {
                            std::fs::symlink_metadata(p).is_ok_and(|m| m.file_type().is_symlink())
                        }) {
                            continue;
                        }
                        let Ok(target) = target.canonicalize() else {
                            continue;
                        };
                        for (target_id, target_root) in &selected {
                            let Some(target_root) = target_root else {
                                continue;
                            };
                            let Ok(relative) = target.strip_prefix(target_root) else {
                                continue;
                            };
                            let Some(relative) = relative.to_str() else {
                                continue;
                            };
                            let relative = relative.replace('\\', "/");
                            if !eligible(&relative) {
                                continue;
                            }
                            let Ok(bytes) = read_source(target_root, &relative) else {
                                continue;
                            };
                            row["resolution"] = json!({"state":"resolved","rule":"explicit-relative-file","derivation":"literal-import-path","member":target_id,"path":relative,"hash":hash(&bytes),"targetFreshness":"verified-selected-source","verifiedAt":now(),"sourceFreshness":row["verification"].as_str().unwrap_or("stored-snapshot"),"completeness":"unknown outside explicit relative files"});
                        }
                    }
                }
                reports.push(json!({"member":id,"state":"available","result":result}));
            }
            Err(error) => {
                reports.push(json!({"member":id,"state":"unavailable","diagnostic":error}))
            }
        }
    }
    Ok(
        json!({"members":reports,"snapshot":"independent-member-generations","resolutionCoverage":"explicit relative files only; package, extension and dynamic resolution unchecked"}),
    )
}
