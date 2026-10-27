//! Worker capacity: a fresh host observation, independent of preferences and approval.
use crate::timestamp;
use crate::values::{js_length, safe_integer};
use serde_json::{Value, json};

const FACTS: &[&str] = &[
    "capacity",
    "occupiedSlots",
    "taskWorkers",
    "allowed",
    "sessionId",
    "observedAt",
    "source",
];
const FRESH_MS: i64 = 30_000;

/// Effective worker limit for one task tree from caller-supplied facts at `now_ms`.
/// `settings` are validated effective settings; `observedAt` must be ISO 8601 with a
/// UTC date or explicit offset.
pub fn budget(
    settings: &Value,
    facts: &Value,
    session: Option<&str>,
    now_ms: i64,
) -> Result<Value, String> {
    let facts = facts
        .as_object()
        .filter(|facts| facts.keys().all(|key| FACTS.contains(&key.as_str())))
        .ok_or("invalid worker capacity facts")?;
    let mut counts = [None; 3];
    for (index, field) in ["capacity", "occupiedSlots", "taskWorkers"]
        .into_iter()
        .enumerate()
    {
        if let Some(value) = facts.get(field) {
            let minimum = if field == "taskWorkers" { 0 } else { 1 };
            counts[index] = Some(
                safe_integer(value)
                    .filter(|n| *n >= minimum)
                    .ok_or_else(|| format!("invalid worker {field}"))?,
            );
        }
    }
    let [capacity, occupied, task_workers] = counts;
    // Contradictory counts are invalid input whether or not the observation is fresh.
    if let (Some(occupied), Some(task_workers)) = (occupied, task_workers)
        && task_workers > occupied - 1
    {
        return Err("worker slot counts exclude the main agent and must fit occupied slots".into());
    }
    if facts
        .get("allowed")
        .is_some_and(|allowed| !allowed.is_boolean())
    {
        return Err("worker allowed must be boolean".into());
    }
    let delegation = &settings["efficiency"]["delegation"];
    let requested = safe_integer(&delegation["maxWorkers"]).unwrap_or(3);
    let mut result = json!({
        "mode": delegation["mode"].as_str().unwrap_or("recommend"),
        "requestedLimit": requested,
        "effectiveLimit": null,
        "available": 0,
        "occupiedTaskWorkers": task_workers,
        "scope": "One task-wide tree, including idle/reusable and nested workers that still occupy host slots.",
        "evidence": "Caller-supplied host observations; this calculation neither discovers capacity nor grants action authorization."
    });
    let mut finish = |fields: Value| {
        for (key, value) in fields.as_object().expect("fields") {
            result[key] = value.clone();
        }
        Ok(result.clone())
    };
    if requested == 0 {
        return finish(json!({ "effectiveLimit": 0, "reason": "disabled" }));
    }
    if facts.get("allowed") == Some(&Value::Bool(false)) {
        return finish(json!({ "effectiveLimit": 0, "reason": "session-prohibited" }));
    }
    let source = facts
        .get("source")
        .and_then(Value::as_str)
        .filter(|s| (1..=512).contains(&js_length(s)));
    let observed = facts
        .get("observedAt")
        .and_then(Value::as_str)
        .and_then(timestamp::parse);
    let fresh = session.is_some_and(|s| {
        !s.is_empty() && facts.get("sessionId").and_then(Value::as_str) == Some(s)
    }) && source.is_some()
        && observed.is_some_and(|at| at <= now_ms && now_ms - at <= FRESH_MS);
    let (Some(capacity), Some(occupied), Some(task_workers)) = (capacity, occupied, task_workers)
    else {
        return finish(json!({ "reason": "fresh-host-capacity-or-session-policy-unavailable" }));
    };
    if !fresh || facts.get("allowed") != Some(&Value::Bool(true)) {
        return finish(json!({ "reason": "fresh-host-capacity-or-session-policy-unavailable" }));
    }
    let other = occupied - 1 - task_workers;
    let limit = requested.min((capacity - 1 - other).max(0));
    let reason = if task_workers > limit {
        "limit-reduced-no-new-assignments"
    } else if limit < requested {
        "host-capacity-limited"
    } else {
        "observed"
    };
    finish(json!({
        "effectiveLimit": limit, "available": (limit - task_workers).max(0), "reason": reason,
        "observedAt": facts["observedAt"], "source": source, "cancelActiveWorkers": false
    }))
}

pub mod lifecycle;
#[cfg(test)]
mod tests;
