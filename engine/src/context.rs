//! Guidance accounting for a route: which package resources it selects, and what reading them
//! would cost. Sizes are measured by the caller; live session usage stays unknown here.
use crate::contracts::safe_integer;
use serde_json::{Value, json};

type Result<T> = std::result::Result<T, String>;

const MAX_TARGETS: usize = 256;
const LIMITS: &str = "Measures package resources, not host-loaded overlay copies. Includes selected entry files, explicit references and required package instructions once per path. Excludes unloaded links, host skill-discovery metadata, tool schemas, conversation and tool output. Missing files make totals a lower bound.";

/// A guidance target and the selection that requested it.
pub struct Request {
    pub target: String,
    pub selected_by: String,
}

fn strings(value: &Value) -> impl Iterator<Item = &str> {
    value
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
}

/// Every skill and reference the route's selections load, then configured module skills.
pub fn requests(plan: &Value) -> Result<Vec<Request>> {
    let mut requests = Vec::new();
    for entry in plan["loading"]["selections"]
        .as_array()
        .into_iter()
        .flatten()
    {
        let selected_by = entry["id"].as_str().unwrap_or_default();
        for target in strings(&entry["skills"]).chain(strings(&entry["references"])) {
            requests.push(Request {
                target: target.into(),
                selected_by: selected_by.into(),
            });
        }
    }
    for target in strings(&plan["skills"]) {
        if !requests.iter().any(|request| request.target == target) {
            requests.push(Request {
                target: target.into(),
                selected_by: "module:configured-skills".into(),
            });
        }
    }
    if requests.len() > MAX_TARGETS {
        return Err(format!(
            "context report supports at most {MAX_TARGETS} selections"
        ));
    }
    Ok(requests)
}

/// A budget in bytes: a positive safe integer.
pub fn budget(value: &Value) -> Result<u64> {
    safe_integer(value)
        .filter(|bytes| *bytes >= 1)
        .map(|bytes| bytes as u64)
        .ok_or_else(|| "budgetBytes must be a positive safe integer".into())
}

fn total(entries: &[Value], field: &str) -> u64 {
    entries
        .iter()
        .filter_map(|entry| entry[field].as_u64())
        .sum()
}

/// The report over measured `entries` and `missing` targets, against an optional budget.
pub fn summary(entries: Vec<Value>, missing: Vec<Value>, budget: Option<u64>) -> Value {
    let bytes = total(&entries, "bytes");
    let repeated = total(&entries, "selections") - entries.len() as u64;
    let mut identical: Vec<(&Value, Vec<&Value>)> = Vec::new();
    for entry in &entries {
        match identical
            .iter_mut()
            .find(|(hash, _)| **hash == entry["sha256"])
        {
            Some((_, paths)) => paths.push(&entry["path"]),
            None => identical.push((&entry["sha256"], vec![&entry["path"]])),
        }
    }
    let identical: Vec<&Vec<&Value>> = identical
        .iter()
        .map(|(_, paths)| paths)
        .filter(|paths| paths.len() > 1)
        .collect();
    let status = match budget {
        None => "not-configured",
        Some(limit) if bytes > limit => "exceeded",
        Some(_) if !missing.is_empty() => "unknown",
        Some(_) => "within",
    };
    json!({
        "formatVersion": 1,
        "scope": "selected-guidance-files; not live session usage",
        "complete": missing.is_empty(),
        "files": entries,
        "missing": missing,
        "totals": { "files": entries.len(), "bytes": bytes, "estimatedTokens": total(&entries, "estimatedTokens"), "repeatedSelections": repeated },
        "identicalContent": identical,
        "estimator": "ceil(UTF-8 bytes / 4) per unique path; heuristic, not a tokenizer",
        "budget": { "bytes": budget, "status": status, "action": "report-only; no guidance omitted" },
        "sessionUsage": { "currentTokens": null, "capacityTokens": null, "status": "unknown; no host observation supplied" },
        "limits": LIMITS,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selections_are_bounded_and_module_skills_follow_catalog_targets() {
        let skills: Vec<String> = (0..=MAX_TARGETS).map(|n| format!("p:s{n}")).collect();
        let plan = json!({ "loading": { "selections": [] }, "skills": skills });
        assert!(requests(&plan).is_err());
        let plan = json!({ "loading": { "selections": [{ "id": "workflow:x", "skills": ["p:a"], "references": ["p:a/references/r.md"] }] }, "skills": ["p:a", "p:b"] });
        let selected: Vec<(String, String)> = requests(&plan)
            .unwrap()
            .into_iter()
            .map(|r| (r.target, r.selected_by))
            .collect();
        assert_eq!(
            selected,
            [
                ("p:a", "workflow:x"),
                ("p:a/references/r.md", "workflow:x"),
                ("p:b", "module:configured-skills")
            ]
            .map(|(t, s)| (t.to_string(), s.to_string()))
        );
    }

    #[test]
    fn a_budget_equal_to_the_total_is_within_and_missing_files_make_it_unknown() {
        let entries = || {
            vec![
                json!({ "path": "a", "bytes": 10, "sha256": "x", "estimatedTokens": 3, "selections": 1 }),
            ]
        };
        assert_eq!(
            summary(entries(), vec![], Some(10))["budget"]["status"],
            "within"
        );
        assert_eq!(
            summary(entries(), vec![], Some(9))["budget"]["status"],
            "exceeded"
        );
        assert_eq!(
            summary(entries(), vec![json!({})], Some(10))["budget"]["status"],
            "unknown"
        );
        assert_eq!(
            summary(entries(), vec![], None)["budget"],
            json!({ "bytes": null, "status": "not-configured", "action": "report-only; no guidance omitted" })
        );
    }
}
