//! Revision-bound review artifacts: the diff snapshot a reviewer receives, the result it
//! returns and the comment-only plan an authorized publisher may act on. Checks are
//! structural; they never judge findings, prove line applicability or contact a code host.
use crate::contracts::{js_blank, safe_integer};
use serde_json::{Map, Value, json};

pub const PROVIDERS: [&str; 3] = ["local", "github", "gitlab"];
const RECOMMENDATIONS: [&str; 3] = ["approve", "request-changes", "needs-verification"];
const SEVERITIES: [&str; 3] = ["high", "medium", "low"];
pub const VERIFICATION: &str = "source snapshot only; no project code, tests or model executed";
const LIMITATION: &str = "Structural validation only. Remote head, line applicability and semantic claims require publisher/reviewer verification.";

/// A full SHA-1 or SHA-256 object ID in lowercase hex.
pub fn object_id(text: &str) -> bool {
    matches!(text.len(), 40 | 64) && text.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

/// A positive decimal change number: a digit string or a safe positive integer.
pub fn change_id(value: &Value) -> bool {
    match value {
        Value::String(text) => {
            text.bytes().next().is_some_and(|b| b != b'0')
                && text.bytes().all(|b| b.is_ascii_digit())
        }
        Value::Number(_) => safe_integer(value).is_some_and(|number| number > 0),
        _ => false,
    }
}

fn changed_path(value: &Value) -> bool {
    value.as_str().is_some_and(|path| {
        !path.is_empty()
            && !path.starts_with('/')
            && !path.contains('\\')
            && !path.split('/').any(|part| part == "..")
    })
}

fn nonblank(value: &Value) -> bool {
    value.as_str().is_some_and(|text| !js_blank(text))
}

fn nonblank_list(value: &Value) -> bool {
    value
        .as_array()
        .is_some_and(|items| items.iter().all(nonblank))
}

pub fn validate_snapshot(snapshot: &Value) -> Result<(), String> {
    let text = |name: &str| snapshot[name].as_str();
    if snapshot["formatVersion"] != json!(1)
        || !text("provider").is_some_and(|provider| PROVIDERS.contains(&provider))
        || !["baseSha", "headSha", "mergeBase"]
            .iter()
            .all(|name| text(name).is_some_and(object_id))
        || text("repository").is_none()
        || !snapshot["files"].is_array()
        || text("diff").is_none()
    {
        return Err("invalid review snapshot".into());
    }
    if text("provider") != Some("local")
        && (!change_id(&snapshot["changeId"]) || !nonblank(&snapshot["repository"]))
    {
        return Err("remote review identity required".into());
    }
    let files = snapshot["files"].as_array().expect("checked above");
    let unique = files
        .iter()
        .enumerate()
        .all(|(index, file)| !files[..index].contains(file));
    if !unique || !files.iter().all(changed_path) {
        return Err("invalid changed file inventory".into());
    }
    Ok(())
}

/// The structural verdict on a review result for `snapshot`, with severity counts.
pub fn validate_review(snapshot: &Value, review: &Value) -> Result<Value, String> {
    validate_snapshot(snapshot)?;
    if review.get("baseSha") != Some(&snapshot["baseSha"])
        || review.get("headSha") != Some(&snapshot["headSha"])
    {
        return Err("review revisions do not match snapshot; regenerate stale findings".into());
    }
    let verification = &review["verification"];
    if !nonblank(&review["summary"])
        || !review["findings"].is_array()
        || !nonblank_list(verification)
        || verification.as_array().is_some_and(Vec::is_empty)
    {
        return Err("review summary, findings and verification required".into());
    }
    let recommendation = review["recommendation"].as_str().unwrap_or_default();
    if !RECOMMENDATIONS.contains(&recommendation) {
        return Err(
            "review recommendation must be approve, request-changes or needs-verification".into(),
        );
    }
    if review
        .get("context")
        .is_some_and(|context| !nonblank_list(context))
    {
        return Err("review context must be an array of nonempty strings".into());
    }
    let files = snapshot["files"].as_array().expect("validated snapshot");
    let findings = review["findings"].as_array().expect("checked above");
    let mut ids: Vec<&str> = Vec::new();
    let mut counts = [0u64; 3];
    for finding in findings {
        let id = finding["id"]
            .as_str()
            .filter(|id| !id.is_empty() && !ids.contains(id));
        let Some(id) = id else {
            return Err("finding IDs must be unique".into());
        };
        ids.push(id);
        let severity = finding["severity"]
            .as_str()
            .and_then(|severity| SEVERITIES.iter().position(|known| *known == severity));
        let located = finding.get("path").is_some_and(|path| files.contains(path))
            && safe_integer(&finding["line"]).is_some_and(|line| line >= 1)
            && matches!(finding["side"].as_str(), Some("old" | "new"));
        let Some(severity) = severity.filter(|_| located) else {
            return Err("finding location/severity is invalid".into());
        };
        for field in ["summary", "evidence"] {
            if !nonblank(&finding[field]) {
                return Err(format!("finding {field} required"));
            }
        }
        counts[severity] += 1;
    }
    if recommendation == "approve" && counts[0] + counts[1] > 0 {
        return Err("approve recommendation conflicts with high or medium findings".into());
    }
    Ok(json!({
        "valid": true,
        "headSha": snapshot["headSha"],
        "findings": findings.len(),
        "recommendation": recommendation,
        "severityCounts": {"high": counts[0], "medium": counts[1], "low": counts[2]},
        "limitation": LIMITATION,
    }))
}

/// The remote revisions a publisher observed just before acting.
pub struct Target<'a> {
    pub current_base: &'a str,
    pub current_head: &'a str,
    pub authorized: bool,
}

/// A comment-only plan for a validated review. It never posts or submits an approval.
pub fn publication_plan(
    snapshot: &Value,
    review: &Value,
    target: &Target,
) -> Result<Value, String> {
    validate_review(snapshot, review)?;
    if !target.authorized {
        return Err("publication requires explicit user or job authorization".into());
    }
    if snapshot["headSha"] != target.current_head || snapshot["baseSha"] != target.current_base {
        return Err("remote base or head changed; do not publish stale findings".into());
    }
    if snapshot["provider"] == "local" || snapshot["changeId"].is_null() {
        return Err("remote review identity required for publication".into());
    }
    let mut plan = Map::new();
    for name in ["provider", "repository", "changeId", "headSha"] {
        plan.insert(name.into(), snapshot[name].clone());
    }
    for name in [
        "summary",
        "recommendation",
        "context",
        "findings",
        "verification",
    ] {
        if let Some(value) = review.get(name) {
            plan.insert(name.into(), value.clone());
        }
    }
    plan.insert("action".into(), json!("comment-only"));
    plan.insert("execution".into(), json!("not-published"));
    Ok(Value::Object(plan))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn change_ids_are_positive_decimals_without_coercion() {
        for valid in [json!("7"), json!("120"), json!(42), json!(42.0)] {
            assert!(change_id(&valid), "{valid}");
        }
        for invalid in [
            json!("0"),
            json!("07"),
            json!("+3"),
            json!(""),
            json!(0),
            json!(-4),
            json!(4.5),
            json!(1e20),
            json!([7]),
            json!(null),
            json!(true),
        ] {
            assert!(!change_id(&invalid), "{invalid}");
        }
    }

    fn snapshot() -> Value {
        json!({"formatVersion": 1, "provider": "github", "repository": "sample/project",
            "changeId": 12, "baseSha": "a".repeat(40), "headSha": "b".repeat(40),
            "mergeBase": "a".repeat(40), "files": ["worker.mjs"], "diff": "fixture"})
    }

    fn review(field: &str, value: Value) -> Value {
        let mut review = json!({"baseSha": "a".repeat(40), "headSha": "b".repeat(40),
            "summary": "Not exercised.", "recommendation": "needs-verification",
            "findings": [], "verification": ["No checks ran."]});
        review[field] = value;
        review
    }

    fn finding(severity: Value) -> Value {
        json!({"id": "F", "severity": severity, "path": "worker.mjs", "side": "new", "line": 1,
            "summary": "Synthetic.", "evidence": "Fixture."})
    }

    #[test]
    fn recommendations_severities_and_context_use_canonical_values_only() {
        for recommendation in [
            json!(null),
            json!(""),
            json!("APPROVE"),
            json!("comment"),
            json!("request_changes"),
        ] {
            let error = validate_review(&snapshot(), &review("recommendation", recommendation));
            assert!(error.unwrap_err().contains("recommendation"));
        }
        for severity in [
            json!("error"),
            json!("warning"),
            json!("info"),
            json!(""),
            json!(null),
            json!(["high"]),
        ] {
            let error =
                validate_review(&snapshot(), &review("findings", json!([finding(severity)])));
            assert!(error.unwrap_err().contains("severity"));
        }
        for context in [
            json!(null),
            json!("note"),
            json!({}),
            json!([""]),
            json!(["  "]),
            json!([null]),
            json!([4]),
        ] {
            let error = validate_review(&snapshot(), &review("context", context));
            assert!(error.unwrap_err().contains("context"));
        }
        for context in [
            json!([]),
            json!(["A configured runtime could not be checked."]),
        ] {
            assert!(validate_review(&snapshot(), &review("context", context)).is_ok());
        }
    }

    #[test]
    fn publication_keeps_context_only_when_the_review_has_it() {
        let target = Target {
            current_base: &"a".repeat(40),
            current_head: &"b".repeat(40),
            authorized: true,
        };
        let plain = publication_plan(&snapshot(), &review("findings", json!([])), &target).unwrap();
        assert!(plain.get("context").is_none());
        let empty = publication_plan(&snapshot(), &review("context", json!([])), &target).unwrap();
        assert_eq!(empty["context"], json!([]));
        assert_eq!(empty["execution"], "not-published");
    }
}
