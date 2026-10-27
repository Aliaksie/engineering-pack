//! Selected repository scopes and the workspace identity of their combined Git snapshots.
use crate::values::line_text;
use crate::{digest, discovery::scope, routing::private_path};
use serde_json::{Value, json};

pub fn scopes(value: &Value) -> Result<Vec<String>, String> {
    let roots = value
        .as_array()
        .filter(|roots| !roots.is_empty() && roots.len() <= 8)
        .ok_or("repositories require 1..8 workspace-relative directories")?;
    let mut scopes = Vec::new();
    for value in roots {
        let text = value
            .as_str()
            .filter(|s| line_text(s, 1024))
            .ok_or("repositories require 1..8 workspace-relative directories")?;
        let scope = scope(&json!(text))?;
        if private_path(&scope) {
            return Err("private workspace cannot be a repository scope".into());
        }
        if scopes.contains(&scope) {
            return Err("duplicate repository scope".into());
        }
        scopes.push(scope);
    }
    scopes.sort_by(|a, b| a.encode_utf16().cmp(b.encode_utf16()));
    Ok(scopes)
}

/// A set of checkouts; deliberately not a Git ref.
pub fn checkout(entries: &[(String, Value)]) -> String {
    format!("workspace:{}", digest(&json!(entries)))
}

/// One snapshot over the selected repositories, each `{path, ...state}`, in scope order.
pub fn snapshot(entries: &[Value]) -> Result<Value, String> {
    let paths = scopes(&Value::Array(
        entries.iter().map(|e| e["path"].clone()).collect(),
    ))?;
    let repositories: Vec<&Value> = paths
        .iter()
        .map(|path| {
            entries
                .iter()
                .find(|e| e["path"] == path.as_str())
                .expect("validated path")
        })
        .collect();
    let fingerprint = |field: &str| {
        digest(&json!(
            repositories
                .iter()
                .map(|e| json!([
                    e["path"],
                    e["head"],
                    e["checkout"],
                    e["fingerprints"][field]
                ]))
                .collect::<Vec<_>>()
        ))
    };
    let prefixed = |field: &str| -> Vec<Value> {
        repositories
            .iter()
            .flat_map(|e| {
                let items = match field {
                    "status" => &e["status"],
                    _ => &e["fingerprints"]["issues"],
                };
                items.as_array().into_iter().flatten().map(move |item| {
                    json!(format!(
                        "{}: {}",
                        e["path"].as_str().unwrap_or_default(),
                        item.as_str().unwrap_or_default()
                    ))
                })
            })
            .collect()
    };
    let checkouts: Vec<_> = repositories
        .iter()
        .map(|e| {
            (
                e["path"].as_str().unwrap_or_default().to_string(),
                e["checkout"].clone(),
            )
        })
        .collect();
    let heads: Vec<_> = repositories
        .iter()
        .map(|e| json!([e["path"], e["head"]]))
        .collect();
    let sum = |field: &str| {
        repositories
            .iter()
            .filter_map(|e| e["fingerprints"][field].as_u64())
            .sum::<u64>()
    };
    Ok(json!({
        "repositories": repositories,
        "checkout": checkout(&checkouts),
        "head": format!("workspace:{}", digest(&json!(heads))),
        "status": prefixed("status"),
        "gitAvailable": repositories.iter().all(|e| e["gitAvailable"] == true),
        "fingerprints": {
            "index": fingerprint("index"),
            "workingTree": fingerprint("workingTree"),
            "complete": repositories.iter().all(|e| e["fingerprints"]["complete"] == true),
            "issues": prefixed("issues"),
            "files": sum("files"),
            "bytes": sum("bytes"),
        }
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn selected_scopes_are_bounded_normalized_and_nonprivate() {
        assert_eq!(scopes(&json!(["./z/", "a"])).unwrap(), vec!["a", "z"]);
        for input in [
            json!([]),
            json!(["../outside"]),
            json!([".craft/state"]),
            json!(["src", "./src"]),
            json!([null]),
            json!(["a\u{0}b"]),
            json!(vec!["a"; 9]),
        ] {
            assert!(scopes(&input).is_err(), "{input}");
        }
    }
}
