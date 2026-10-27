//! Handoff of an unfinished task from an earlier session of the same worktree: which saved
//! records a new session may adopt. Records are copied, never shared between sessions.
use super::{binding, validate};
use crate::retention::SESSION_RETENTION_DAYS;
use crate::timestamp;
use crate::values::truthy;
use serde_json::{Value, json};

type Result<T> = std::result::Result<T, String>;

/// Session directories read when looking for handoffs.
pub const SCANNED: usize = 256;
/// Handoffs offered at once, newest first.
pub const OFFERED: usize = 3;
const DAY_MS: i64 = 86_400_000;

/// One earlier session's records as read: its task binding and the task records it holds,
/// each with the time it last changed.
pub struct Saved<'a> {
    pub from: &'a str,
    pub binding: &'a Value,
    pub states: &'a [(Value, i64)],
}

/// The task an earlier session holds for `checkout`: its binding names exactly one valid
/// record bound to this checkout. `Ok(None)` when it holds none.
pub fn candidate(saved: &Saved, checkout: &Value) -> Result<Option<Value>> {
    let Some(bound) = binding(saved.binding, Some(checkout))? else {
        return Ok(None);
    };
    // A plan task is continued through its plan, which records who holds it.
    if bound.get("plan").is_some() {
        return Ok(None);
    }
    let task = &bound["task"];
    let mut records = saved
        .states
        .iter()
        .filter(|(state, _)| state["taskRecord"]["id"] == *task);
    let (Some((state, updated)), None) = (records.next(), records.next()) else {
        return Ok(None);
    };
    validate(state)?;
    let record = &state["taskRecord"];
    let field = |name: &str| record.get(name).cloned().unwrap_or(Value::Null);
    Ok(Some(json!({
        "from": saved.from,
        "task": task,
        "goal": field("goal"),
        "stage": field("stage"),
        "nextStep": field("nextStep"),
        "updated": timestamp::format(*updated),
        "repositories": truthy(&field("repositories")),
        "goalComplete": record["goalComplete"] == true,
    })))
}

/// Whether a candidate may still be adopted: not declared complete and changed within the
/// retention window.
pub fn open(candidate: &Value, now: i64) -> bool {
    let updated = candidate["updated"].as_str().and_then(timestamp::parse);
    candidate["goalComplete"] != true
        && updated.is_some_and(|updated| now - updated < SESSION_RETENTION_DAYS * DAY_MS)
}

/// The handoffs to offer, newest first, at most [`OFFERED`]. The most recent record of a
/// task (id and goal) decides: a task completed in a later session is no longer offered.
pub fn offer(mut candidates: Vec<Value>, now: i64) -> Vec<Value> {
    let updated = |c: &Value| c["updated"].as_str().and_then(timestamp::parse);
    candidates.sort_by(|a, b| {
        updated(b)
            .cmp(&updated(a))
            .then_with(|| a["from"].as_str().cmp(&b["from"].as_str()))
    });
    let mut seen: Vec<(Value, Value)> = Vec::new();
    let mut offered = Vec::new();
    for mut candidate in candidates {
        let task = (candidate["task"].clone(), candidate["goal"].clone());
        if seen.contains(&task) {
            continue;
        }
        seen.push(task);
        if offered.len() < OFFERED && open(&candidate, now) {
            candidate.as_object_mut().map(|c| c.remove("goalComplete"));
            offered.push(candidate);
        }
    }
    offered
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: i64 = 1_800_000_000_000;

    fn state(task: &str, complete: bool) -> Value {
        json!({"formatVersion": 1, "activity": {}, "lookups": {}, "attempts": [],
            "taskRecord": {"id": task, "goal": "Fix it", "stage": "implement", "status": "working",
                "goalComplete": complete, "nextStep": "Run checks"}})
    }

    fn found(binding: &Value, states: &[(Value, i64)]) -> Option<Value> {
        let saved = Saved {
            from: "aaaaaaaaaaaaaaaaaaaaaaaa",
            binding,
            states,
        };
        candidate(&saved, &json!("refs/heads/main"))
            .unwrap()
            .filter(|c| open(c, NOW))
    }

    fn bound(task: &str) -> Value {
        json!({"formatVersion": 1, "task": task, "checkout": "refs/heads/main"})
    }

    #[test]
    fn an_unfinished_bound_task_is_handed_off() {
        let offered = found(&bound("t1"), &[(state("t1", false), NOW - 1000)]).unwrap();
        assert_eq!(offered["task"], "t1");
        assert_eq!(offered["nextStep"], "Run checks");
        assert_eq!(offered["repositories"], false);
    }

    #[test]
    fn complete_expired_unbound_or_other_checkout_tasks_are_not() {
        assert!(found(&bound("t1"), &[(state("t1", true), NOW)]).is_none());
        let expired = NOW - SESSION_RETENTION_DAYS * DAY_MS;
        assert!(found(&bound("t1"), &[(state("t1", false), expired)]).is_none());
        assert!(found(&Value::Null, &[(state("t1", false), NOW)]).is_none());
        assert!(found(&bound("t2"), &[(state("t1", false), NOW)]).is_none());
        let other = json!({"formatVersion": 1, "task": "t1", "checkout": "refs/heads/other"});
        assert!(found(&other, &[(state("t1", false), NOW)]).is_none());
    }

    #[test]
    fn a_plan_task_is_continued_through_its_plan() {
        let planned = json!({"formatVersion": 2, "task": "t1", "plan": "work", "checkout": "refs/heads/main"});
        assert!(found(&planned, &[(state("t1", false), NOW)]).is_none());
    }

    #[test]
    fn two_records_of_the_bound_task_are_ambiguous() {
        let states = [(state("t1", false), NOW), (state("t1", false), NOW)];
        assert!(found(&bound("t1"), &states).is_none());
    }

    #[test]
    fn an_invalid_binding_or_record_is_an_error() {
        let saved = Saved {
            from: "a",
            binding: &json!({"task": "t1"}),
            states: &[],
        };
        assert!(candidate(&saved, &json!(null)).is_err());
        let mut broken = state("t1", false);
        broken["attempts"] = json!("none");
        let saved = Saved {
            from: "a",
            binding: &bound("t1"),
            states: &[(broken, NOW)],
        };
        assert!(candidate(&saved, &json!("refs/heads/main")).is_err());
    }

    #[test]
    fn the_newest_record_per_task_decides_up_to_the_bound() {
        let at = |from: &str, task: &str, ms: i64, complete: bool| json!({"from": from, "task": task, "goal": "Fix it", "updated": timestamp::format(ms), "goalComplete": complete});
        let offered = offer(
            vec![
                at("a", "t1", NOW - 5, false),
                at("b", "t1", NOW - 1, false),
                at("c", "t2", NOW - 2, false),
                at("d", "t3", NOW - 3, false),
                at("e", "t4", NOW - 4, false),
                // Completed in a later session: its earlier record is not offered again.
                at("f", "t5", NOW - 6, false),
                at("g", "t5", NOW - 1, true),
            ],
            NOW,
        );
        let from: Vec<_> = offered.iter().map(|o| o["from"].clone()).collect();
        assert_eq!(from, [json!("b"), json!("c"), json!("d")]);
        assert!(offered.iter().all(|o| o.get("goalComplete").is_none()));
        let completed = offer(
            vec![at("f", "t5", NOW - 6, false), at("g", "t5", NOW - 1, true)],
            NOW,
        );
        assert!(completed.is_empty());
    }
}
