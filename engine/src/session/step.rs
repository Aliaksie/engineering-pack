//! `task-step`: one agent-declared acceptance receipt for the bound task, its effect on the
//! task record (status, completion, verification, structured evidence) and the bounded
//! continuation a checkpoint saves. Nothing here reads or judges the evidence artifact.
use super::{
    acceptance, append_attempt, attempt_receipt, contract, stage, task_acceptance, text, unresolved,
};
use crate::values::truthy;
use crate::{checkpoint, digest};
use serde_json::{Map, Value, json};

type Result<T = ()> = std::result::Result<T, String>;

const FIELDS: &[&str] = &[
    "stage",
    "target",
    "approach",
    "evidence",
    "outcome",
    "result",
    "nextStep",
    "complete",
    "profile",
    "maxAttempts",
    "acceptanceEvidence",
    "expectedTaskContract",
    "expectedEvidenceDigest",
];
/// The bound on accumulated completed work and on the continuation's changed files.
const TEXT_BOUND: usize = 1024;

pub struct Step {
    pub receipt: Value,
    pub continuation: Value,
    pub acceptance: Value,
}

/// The last `units` UTF-16 code units of `text`, never splitting a character.
fn tail(text: &str, units: usize) -> String {
    let mut taken = 0;
    let mut start = text.len();
    for (index, c) in text.char_indices().rev() {
        taken += c.len_utf16();
        if taken > units {
            break;
        }
        start = index;
    }
    text[start..].to_string()
}

/// The first `units` UTF-16 code units of `text`, never splitting a character.
fn head(text: &str, units: usize) -> String {
    let mut taken = 0;
    let mut end = 0;
    for (index, c) in text.char_indices() {
        taken += c.len_utf16();
        if taken > units {
            break;
        }
        end = index + c.len_utf8();
    }
    text[..end].to_string()
}

/// `value ?? fallback`, where JSON null is absent.
fn given(value: Option<&Value>) -> Option<&Value> {
    value.filter(|v| !v.is_null())
}

/// Record one step against the saved task. `id` and `at` identify the attempt receipt.
pub fn step(
    state: &mut Value,
    input: &Value,
    repository: &Value,
    id: &str,
    at: &str,
) -> Result<Step> {
    let input = input
        .as_object()
        .filter(|m| m.keys().all(|k| FIELDS.contains(&k.as_str())))
        .ok_or("invalid task step")?;
    let field = |name: &str| input.get(name);
    let record = state
        .get("taskRecord")
        .filter(|r| truthy(*r))
        .ok_or("task-step requires task-start for this session and task")?;
    let selected = stage(
        given(field("stage"))
            .or(record.get("stage"))
            .unwrap_or(&Value::Null),
    )?;
    for name in ["result", "nextStep"] {
        if !text(field(name).unwrap_or(&Value::Null)) {
            return Err(format!(
                "task step {name} must be 1..512 characters without control characters"
            ));
        }
    }
    let complete = match field("complete") {
        None => false,
        Some(Value::Bool(complete)) => *complete,
        Some(_) => return Err("task completion must be boolean".into()),
    };
    let outcome = field("outcome").and_then(Value::as_str);
    if complete && outcome != Some("passed") {
        return Err("task completion requires a passed acceptance receipt".into());
    }
    let mut attempt = Map::from_iter([("operation".to_string(), json!(selected))]);
    for name in [
        "target",
        "approach",
        "evidence",
        "outcome",
        "profile",
        "maxAttempts",
    ] {
        if let Some(value) = field(name) {
            attempt.insert(name.into(), value.clone());
        }
    }
    let receipt = attempt_receipt(&Value::Object(attempt), repository, id, at)?;
    let structured = truthy(record.get("engineeringContract"));
    if let Some(evidence) = field("acceptanceEvidence") {
        if !structured {
            return Err("acceptance evidence requires an engineering contract".into());
        }
        if field("expectedTaskContract") != Some(&contract(Some(record))?) {
            return Err("acceptance evidence task contract changed; reobserve the task".into());
        }
        let saved = record.get("acceptanceEvidence").unwrap_or(&Value::Null);
        if field("expectedEvidenceDigest") != Some(&json!(digest(saved))) {
            return Err(
                "acceptance evidence ledger changed; reload before replacing evidence".into(),
            );
        }
        acceptance::validate_evidence(evidence)?;
        // Explicit whole-ledger replacement permits a documented correction without
        // choosing a passing receipt over a contradictory failure automatically.
        state["taskRecord"]["acceptanceEvidence"] = evidence.clone();
    }
    append_attempt(state, receipt.clone())?;
    let acceptance = task_acceptance(state, repository)?;
    if complete && structured && acceptance["complete"] != true {
        return Err("task completion requires current verified evidence for every required criterion and no open conflicts or decisions".into());
    }
    let attempts = state["attempts"].as_array().map_or(&[][..], Vec::as_slice);
    if complete && !unresolved(attempts).is_empty() {
        return Err("task completion has unresolved acceptance failures or blockers".into());
    }
    let status = match outcome {
        Some("passed") => "completed",
        Some("quality-failure") => "failed",
        Some("prerequisite-resolved") => "working",
        _ => "blocked",
    };
    let result = field("result").and_then(Value::as_str).unwrap_or_default();
    let next = field("nextStep")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let record = state["taskRecord"]
        .as_object_mut()
        .ok_or("invalid session state")?;
    for (key, value) in [
        ("stage", json!(selected)),
        ("status", json!(status)),
        ("goalComplete", json!(complete)),
    ] {
        record.insert(key.into(), value);
    }
    match repository.get("head") {
        Some(head) => record.insert("revision".into(), head.clone()),
        None => record.shift_remove("revision"),
    };
    for (key, value) in [
        ("source", receipt["source"].clone()),
        ("safeBoundary", json!(true)),
        ("summary", json!(result)),
        ("nextStep", json!(next)),
    ] {
        record.insert(key.into(), value);
    }
    // A blocker or a resolution means the check did not run; only a run records its verdict.
    if selected == "verify" && matches!(outcome, Some("passed" | "quality-failure")) {
        record.insert(
            "verification".into(),
            json!(if status == "completed" {
                "passed"
            } else {
                "failed"
            }),
        );
        match repository.get("head") {
            Some(head) => record.insert("verificationRevision".into(), head.clone()),
            None => record.shift_remove("verificationRevision"),
        };
    } else {
        record.shift_remove("verification");
        record.shift_remove("verificationRevision");
    }
    let accepted = contract(Some(&Value::Object(record.clone())))?;
    record.insert("acceptedContract".into(), accepted);
    let completed = record
        .get("completed")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    if outcome == Some("passed") && !completed.ends_with(result) {
        let joined = [completed.as_str(), result]
            .iter()
            .filter(|t| !t.is_empty())
            .copied()
            .collect::<Vec<_>>()
            .join("\n");
        record.insert("completed".into(), json!(tail(&joined, TEXT_BOUND)));
    }
    let files: Vec<&str> = record["files"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .collect();
    let field_text = |name: &str| field(name).and_then(Value::as_str).unwrap_or_default();
    let continuation = json!({
        "goal": record["goal"],
        "currentState": result,
        "completed": given(record.get("completed")).cloned().unwrap_or(json!("")),
        "approach": field_text("approach"),
        "evidence": field_text("evidence"),
        "filesChanged": head(&files.join(", "), TEXT_BOUND),
        "validation": format!("{}: {}", outcome.unwrap_or_default(), field_text("evidence")),
        "remainingWork": if complete { "" } else { next },
        "blockers": if status == "blocked" { result } else { "" },
        "nextStep": next,
    });
    let metadata = Map::from_iter([
        ("taskId".to_string(), record["id"].clone()),
        ("continuation".to_string(), continuation.clone()),
    ]);
    checkpoint::validate_metadata(&metadata)?;
    Ok(Step {
        receipt,
        continuation,
        acceptance,
    })
}
