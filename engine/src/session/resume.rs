//! Task decisions and the resume view: what a session needs to continue a task after its
//! context was compacted, or in a new session that adopted it. Records only; nothing here
//! judges evidence or renews authorization.
use crate::values::{js_length, text, truthy};
use serde_json::{Map, Value, json};

type Result<T = ()> = std::result::Result<T, String>;

/// Decisions one task keeps; recording more is refused rather than silently dropping one.
pub const DECISIONS: usize = 32;
/// Rejected alternatives per decision.
const REJECTED: usize = 4;
/// Current decisions and recent steps a resume view lists.
const SHOWN_DECISIONS: usize = 8;
const SHOWN_STEPS: usize = 8;
/// Serialized bytes of a resume view; steps, then the oldest decisions, give way first.
pub const RESUME_BYTES: usize = 4096;
const BY: &[&str] = &["user", "agent"];
const FIELDS: &[&str] = &["text", "why", "rejected", "by", "supersedes"];

/// Record one decision `{text, why?, rejected?, by, supersedes?}` on the task state at
/// `at`; returns the stored decision. A superseded decision stays, marked, so the record
/// shows what changed.
pub fn decide(state: &mut Value, input: &Value, at: &str) -> Result<Value> {
    let input = input
        .as_object()
        .filter(|m| m.keys().all(|k| FIELDS.contains(&k.as_str())))
        .ok_or("a decision is {text, why?, rejected?, by, supersedes?}")?;
    if !truthy(&state["taskRecord"]) {
        return Err("task-decide requires task-start for this session and task".into());
    }
    let line = |name: &str| -> Result<Option<Value>> {
        match input.get(name) {
            None | Some(Value::Null) => Ok(None),
            Some(value) if text(value) => Ok(Some(value.clone())),
            Some(_) => Err(format!(
                "decision {name} must be 1..512 characters without control characters"
            )),
        }
    };
    let what = line("text")?.ok_or("a decision needs its text")?;
    let by = input
        .get("by")
        .and_then(Value::as_str)
        .filter(|b| BY.contains(b))
        .ok_or("a decision is made by user or agent")?;
    let rejected = match input.get("rejected") {
        None | Some(Value::Null) => vec![],
        Some(Value::Array(items)) if items.len() <= REJECTED && items.iter().all(text) => {
            items.clone()
        }
        Some(_) => {
            return Err(format!(
                "at most {REJECTED} rejected alternatives of 1..512 characters"
            ));
        }
    };
    let decisions = state
        .as_object_mut()
        .ok_or("invalid session state")?
        .entry("decisions")
        .or_insert_with(|| json!([]))
        .as_array_mut()
        .ok_or("invalid task decisions")?;
    if decisions.len() >= DECISIONS {
        return Err(format!(
            "a task holds at most {DECISIONS} decisions; supersede one in a new task"
        ));
    }
    let id = format!("d{}", decisions.len() + 1);
    let supersedes = line("supersedes")?;
    if let Some(earlier) = &supersedes {
        let found = decisions
            .iter_mut()
            .find(|d| d["id"] == *earlier)
            .ok_or_else(|| format!("no decision {earlier} to supersede"))?;
        if found.get("supersededBy").is_some() {
            return Err(format!("decision {earlier} is already superseded"));
        }
        found["supersededBy"] = json!(id);
    }
    let mut decision = Map::from_iter([("id".to_string(), json!(id)), ("text".to_string(), what)]);
    if let Some(why) = line("why")? {
        decision.insert("why".into(), why);
    }
    if !rejected.is_empty() {
        decision.insert("rejected".into(), json!(rejected));
    }
    if let Some(earlier) = supersedes {
        decision.insert("supersedes".into(), earlier);
    }
    decision.insert("by".into(), json!(by));
    decision.insert("at".into(), json!(at));
    let decision = Value::Object(decision);
    decisions.push(decision.clone());
    Ok(decision)
}

/// Whether saved decisions are well formed; absent is valid.
pub fn validate(state: &Value) -> Result {
    let Some(decisions) = state.get("decisions") else {
        return Ok(());
    };
    let valid = decisions.as_array().is_some_and(|items| {
        items.len() <= DECISIONS
            && items.iter().all(|d| {
                text(&d["id"])
                    && text(&d["text"])
                    && d["by"].as_str().is_some_and(|b| BY.contains(&b))
                    && d.get("why").is_none_or(text)
            })
    });
    if valid {
        Ok(())
    } else {
        Err("invalid task decisions".into())
    }
}

/// How the current tree relates to the checkpoint a resume starts from.
pub enum Source {
    Unchanged,
    Changed,
    Unknown,
}

/// Structured acceptance as current source sees it: criterion ids by status, stale evidence
/// and what blocks completion. Null without an engineering contract.
fn acceptance(acceptance: &Value) -> Value {
    let criteria = acceptance["criteria"]
        .as_array()
        .map_or(&[][..], Vec::as_slice);
    if criteria.is_empty() {
        return Value::Null;
    }
    let mut view = Map::from_iter([("complete".to_string(), acceptance["complete"].clone())]);
    for status in ["failed", "gap", "verified"] {
        let ids: Vec<&Value> = criteria
            .iter()
            .filter(|c| c["status"] == status)
            .map(|c| &c["id"])
            .collect();
        if !ids.is_empty() {
            view.insert(status.into(), json!(ids));
        }
    }
    let stale = acceptance["records"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|r| r["freshness"] == "stale")
        .count();
    if stale > 0 {
        view.insert("staleEvidence".into(), json!(stale));
    }
    for key in ["conflicts", "decisions"] {
        if acceptance[key].as_array().is_some_and(|a| !a.is_empty()) {
            let name = if key == "decisions" {
                "openDecisions"
            } else {
                key
            };
            view.insert(name.into(), acceptance[key].clone());
        }
    }
    Value::Object(view)
}

/// The resume view of a task: goal, where it stands, what was done and decided, what is
/// open and the next step, within `RESUME_BYTES`. `context` is the task as current source
/// sees it (`session::context`); `state` the saved task state.
pub fn view(state: &Value, context: &Value, source: Source) -> Value {
    let record = &state["taskRecord"];
    let task = &context["task"];
    let mut view = Map::new();
    let mut put = |key: &str, value: &Value| {
        if truthy(value) {
            view.insert(key.into(), value.clone());
        }
    };
    put("task", &record["id"]);
    put("goal", &record["goal"]);
    put("stage", &task["stage"]);
    put("status", &task["status"]);
    put("done", &record["completed"]);
    let open = matches!(task["status"].as_str(), Some("failed" | "blocked"));
    put(if open { "open" } else { "state" }, &record["summary"]);
    put("files", &task["files"]);
    put("next", &record["nextStep"]);
    put("acceptance", &acceptance(&context["acceptance"]));
    let current: Vec<Value> = state["decisions"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|d| d.get("supersededBy").is_none())
        .map(|d| {
            let mut shown = d.clone();
            if let Some(shown) = shown.as_object_mut() {
                shown.shift_remove("at");
            }
            shown
        })
        .collect();
    let steps: Vec<Value> = state["attempts"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|a| {
            let part = |name: &str| a[name].as_str().unwrap_or_default();
            let evidence = part("evidence");
            let mut step = format!(
                "{} {} {}",
                part("operation"),
                part("target"),
                part("outcome")
            );
            if !evidence.is_empty() {
                step.push_str(&format!(" ({evidence})"));
            }
            json!(step.split_whitespace().collect::<Vec<_>>().join(" "))
        })
        .collect();
    let stale = context["stale"] == true;
    let source = match source {
        _ if stale => "steps were accepted against other source; recheck before relying on them",
        Source::Unchanged => "unchanged since the last checkpoint",
        Source::Changed => "changed since the last checkpoint; recheck evidence for changed files",
        Source::Unknown => "unknown whether it changed; recheck evidence before relying on it",
    };
    view.insert("source".into(), json!(source));
    let mut decisions = current[current.len().saturating_sub(SHOWN_DECISIONS)..].to_vec();
    let mut steps = steps[steps.len().saturating_sub(SHOWN_STEPS)..].to_vec();
    let total = |decisions: &[Value], steps: &[Value]| {
        let mut probe = view.clone();
        probe.insert("decisions".into(), json!(decisions));
        probe.insert("steps".into(), json!(steps));
        js_length(&Value::Object(probe).to_string())
    };
    while total(&decisions, &steps) > RESUME_BYTES && !steps.is_empty() {
        steps.remove(0);
    }
    while total(&decisions, &steps) > RESUME_BYTES && !decisions.is_empty() {
        decisions.remove(0);
    }
    let hidden = current.len() - decisions.len();
    if !decisions.is_empty() {
        view.insert("decisions".into(), json!(decisions));
    }
    if hidden > 0 {
        view.insert("earlierDecisions".into(), json!(hidden));
    }
    if !steps.is_empty() {
        view.insert("steps".into(), json!(steps));
    }
    Value::Object(view)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn task() -> Value {
        json!({"formatVersion": 1, "activity": {}, "lookups": {}, "attempts": [],
            "taskRecord": {"id": "fix", "goal": "Fix it", "stage": "implement", "status": "working"}})
    }

    #[test]
    fn a_decision_is_recorded_with_its_reason_and_author() {
        let mut state = task();
        let input = json!({"text": "Keep the old API", "why": "Clients pin it",
            "rejected": ["Break it"], "by": "user"});
        let decision = decide(&mut state, &input, "2026-11-04T00:00:00.000Z").unwrap();
        assert_eq!(decision["id"], "d1");
        assert_eq!(state["decisions"][0]["rejected"][0], "Break it");
        validate(&state).unwrap();
    }

    #[test]
    fn a_superseded_decision_leaves_the_resume_view() {
        let mut state = task();
        let at = "2026-11-04T00:00:00.000Z";
        decide(&mut state, &json!({"text": "Use A", "by": "agent"}), at).unwrap();
        decide(
            &mut state,
            &json!({"text": "Use B", "by": "user", "supersedes": "d1"}),
            at,
        )
        .unwrap();
        assert_eq!(state["decisions"][0]["supersededBy"], "d2");
        let context = json!({"task": state["taskRecord"].clone(), "stale": false});
        let view = view(&state, &context, Source::Unchanged);
        assert_eq!(view["decisions"].as_array().unwrap().len(), 1);
        assert_eq!(view["decisions"][0]["text"], "Use B");
        let again = decide(
            &mut state,
            &json!({"text": "C", "by": "user", "supersedes": "d1"}),
            at,
        );
        assert!(again.unwrap_err().contains("already superseded"));
    }

    #[test]
    fn decisions_are_bounded_and_refused_rather_than_dropped() {
        let mut state = task();
        for n in 0..DECISIONS {
            decide(
                &mut state,
                &json!({"text": format!("D{n}"), "by": "agent"}),
                "t",
            )
            .unwrap();
        }
        let refused = decide(&mut state, &json!({"text": "one more", "by": "agent"}), "t");
        assert!(refused.unwrap_err().contains("at most"));
        assert!(decide(&mut task(), &json!({"text": "x", "by": "bot"}), "t").is_err());
    }

    #[test]
    fn the_resume_view_shows_an_open_failure_and_stays_within_its_budget() {
        let mut state = task();
        state["taskRecord"]["summary"] = json!("Edge case 0 fails");
        state["taskRecord"]["nextStep"] = json!("Handle zero");
        let long = "x".repeat(500);
        for n in 0..DECISIONS {
            decide(
                &mut state,
                &json!({"text": format!("{n}{long}"), "why": long, "by": "agent"}),
                "t",
            )
            .unwrap();
        }
        state["attempts"] = json!(
            (0..24)
                .map(|n| json!({"operation": "verify",
            "target": format!("acceptance:{n}{long}"), "outcome": "passed"}))
                .collect::<Vec<_>>()
        );
        let mut current = state["taskRecord"].clone();
        current["status"] = json!("failed");
        let view = view(
            &state,
            &json!({"task": current, "stale": false}),
            Source::Changed,
        );
        assert_eq!(view["open"], "Edge case 0 fails");
        assert_eq!(view["next"], "Handle zero");
        assert!(view.get("state").is_none());
        assert!(js_length(&view.to_string()) <= RESUME_BYTES);
        assert!(view["earlierDecisions"].as_u64().unwrap() > 0);
        assert!(view["source"].as_str().unwrap().starts_with("changed"));
    }

    #[test]
    fn structured_acceptance_shows_criteria_by_status_and_stale_evidence() {
        let state = task();
        let context = json!({"task": state["taskRecord"].clone(), "stale": false, "acceptance": {
            "complete": false,
            "criteria": [{"id": "a", "status": "verified"}, {"id": "b", "status": "gap"}],
            "records": [{"freshness": "stale"}, {"freshness": "current"}],
            "conflicts": [], "decisions": ["q1"]}});
        let view = view(&state, &context, Source::Unchanged);
        assert_eq!(
            view["acceptance"],
            json!({"complete": false, "gap": ["b"],
            "verified": ["a"], "staleEvidence": 1, "openDecisions": ["q1"]})
        );
        let plain = json!({"task": state["taskRecord"].clone(), "acceptance": {"criteria": []}});
        assert!(
            super::view(&state, &plain, Source::Unchanged)
                .get("acceptance")
                .is_none()
        );
    }

    #[test]
    fn stale_steps_are_flagged_whatever_the_tree_comparison() {
        let state = task();
        let context = json!({"task": state["taskRecord"].clone(), "stale": true});
        let view = view(&state, &context, Source::Unchanged);
        assert!(view["source"].as_str().unwrap().contains("other source"));
    }
}
