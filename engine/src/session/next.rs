//! The one next step a turn-end notice names, from facts Craft already holds, in this order: a
//! declared check that covers files the turn changed and has not run since they changed, a
//! required acceptance criterion of the session's task with no evidence yet, the next ready
//! task of the Craft work plan whose task the session completed, and a context window past
//! its compaction threshold. The first fact that applies wins; without one the notice says
//! nothing more. Nothing here runs a check, claims a task, compacts or saves a checkpoint.
use super::status;
use crate::timestamp;
use serde_json::{Value, json};

/// How old a host's context observation may be and still speak for the session now.
const OBSERVATION_MS: i64 = 300_000;
const COMPACT_AT: f64 = 0.75;

/// Where the session's checkpoint stands against the commit the project is on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Checkpoint {
    /// Saved on the commit the project is on now.
    Current,
    /// Saved on another commit, or where the commit cannot be told.
    Behind,
    Missing,
}

#[derive(Debug, Default, Clone, PartialEq)]
pub struct Facts {
    /// Declared checks of the commit stage that cover files changed since the session last
    /// ran them.
    pub unrun_checks: usize,
    /// `(criterion, task)`: a required criterion of the session's open task that no evidence
    /// record names.
    pub unevidenced: Option<(String, String)>,
    /// The ready task the session's work plan offers once its own task is completed.
    pub ready: Option<Ready>,
    /// Observed context occupancy in percent, at or past the compaction threshold, and the
    /// checkpoint a compaction would resume from.
    pub context: Option<(u64, Checkpoint)>,
}

/// A plan task ready to start: `{plan, task, goal}`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ready {
    pub plan: String,
    pub task: String,
    pub goal: String,
}

/// Longest part of a task's goal a next step quotes.
const GOAL_CHARS: usize = 80;

/// Whether turn-end notices name a next step: unless `efficiency.advice.enabled` or
/// `efficiency.advice.nextStep` is false, the switches session advice already has.
pub fn enabled(settings: &Value) -> bool {
    let advice = &settings["efficiency"]["advice"];
    advice["enabled"] != false && advice["nextStep"] != false
}

/// Whether a turn's changed files can call for a check: next steps are on, Craft runs
/// declared checks and the project declares one for the commit stage or before it.
pub fn tracks_changes(settings: &Value) -> bool {
    enabled(settings)
        && crate::checks::enabled(settings)
        && crate::routing::scope(settings, &[])
            .and_then(|scope| {
                crate::checks::plan(settings, &scope, crate::checks::DEFAULT_STAGE, true)
            })
            .is_ok_and(|plan| plan["run"].as_array().is_some_and(|run| !run.is_empty()))
}

/// The observed context occupancy in percent when it is at or past `efficiency.context
/// .compactAt`; `None` without a fresh current-context observation of this session, which is
/// never estimated.
pub fn pressure(settings: &Value, usage: &Value, session: &str, now: i64) -> Option<u64> {
    let compact_at = settings["efficiency"]["context"]["compactAt"]
        .as_f64()
        .unwrap_or(COMPACT_AT);
    let observed = status::usage(
        Some(usage).filter(|u| !u.is_null()),
        Some(&json!(session)),
        now,
        OBSERVATION_MS,
        None,
    )
    .ok()?;
    let ratio = observed["usedRatio"].as_f64()?;
    (ratio >= compact_at).then(|| (ratio * 100.0).floor() as u64)
}

/// Whether a check is still to run: none of the session's runs at `stage` or a later one, or
/// the newest of them older than the newest change. A change whose time is unknown (a
/// deleted file) counts only when no run is recorded.
pub fn unrun(history: &Value, stage: &str, changed_ms: Option<i64>) -> bool {
    let rank = |stage: &str| crate::checks::STAGES.iter().position(|s| *s == stage);
    let Some(at) = rank(stage) else {
        return false;
    };
    let last = history["runs"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|run| run["stage"].as_str().and_then(rank) >= Some(at))
        .filter_map(|run| run["at"].as_str().and_then(timestamp::parse))
        .max();
    match (last, changed_ms) {
        (None, _) => true,
        (Some(last), Some(changed)) => changed > last,
        (Some(_), None) => false,
    }
}

/// The first required criterion of a task record's acceptance contract that no evidence
/// record names, whatever that evidence says; `None` for a completed task or one without a
/// contract. Whether evidence is current needs the whole tree's fingerprint, which a turn's
/// end does not take, so only the absence of any is a fact here.
pub fn unevidenced(record: &Value) -> Option<String> {
    if record["status"] == "completed" || record["goalComplete"] == true {
        return None;
    }
    let named = |id: &Value| {
        record["acceptanceEvidence"]["records"]
            .as_array()
            .into_iter()
            .flatten()
            .any(|entry| {
                entry["criterionIds"]
                    .as_array()
                    .is_some_and(|ids| ids.contains(id))
            })
    };
    record["engineeringContract"]["criteria"]
        .as_array()?
        .iter()
        .filter(|criterion| criterion["required"] == true)
        .find(|criterion| !named(&criterion["id"]))
        .and_then(|criterion| criterion["id"].as_str().map(str::to_owned))
}

/// The task a plan offers next once `held` is completed there: `queue` is the plan's
/// coordinator view for the session. `None` while the held task is still open.
pub fn ready(plan: &Value, held: &str, queue: &Value) -> Option<Ready> {
    let tasks = plan["tasks"].as_array()?;
    let find = |id: &str| tasks.iter().find(|task| task["id"] == id);
    if find(held)?["state"] != "completed" {
        return None;
    }
    let task = queue["next"].as_str()?;
    let goal = find(task)?["goal"].as_str().unwrap_or_default();
    let mut quoted: String = goal.chars().take(GOAL_CHARS).collect();
    if quoted.len() < goal.len() {
        quoted.push('…');
    }
    Some(Ready {
        plan: plan["id"].as_str()?.to_owned(),
        task: task.to_owned(),
        goal: quoted,
    })
}

/// The next step to name, or `None`.
pub fn line(facts: &Facts) -> Option<String> {
    if facts.unrun_checks > 0 {
        let checks = if facts.unrun_checks == 1 {
            "1 declared check covers".to_string()
        } else {
            format!("{} declared checks cover", facts.unrun_checks)
        };
        return Some(format!(
            "craft check --stage commit ({checks} files changed since it last ran)"
        ));
    }
    if let Some((criterion, task)) = &facts.unevidenced {
        return Some(format!(
            "acceptance \"{criterion}\" of task {task} has no evidence yet"
        ));
    }
    if let Some(Ready { plan, task, goal }) = &facts.ready {
        return Some(format!(
            "plan task {task} ({goal}) is ready: craft task-start --plan {plan} --ready"
        ));
    }
    let (percent, checkpoint) = facts.context?;
    Some(match checkpoint {
        Checkpoint::Current => format!(
            "context {percent} %: compact or start a new session; the session's checkpoint is on this commit"
        ),
        Checkpoint::Behind => format!(
            "context {percent} %: save a checkpoint before compacting; the last one is not on this commit"
        ),
        Checkpoint::Missing => {
            format!("context {percent} %: save a checkpoint before compacting")
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: i64 = 1_800_000_000_000;

    fn usage(current: u64) -> Value {
        json!({"kind": "current-context", "currentTokens": current, "capacityTokens": 1000,
            "source": "claude-statusline", "observedAt": timestamp::format(NOW - 1000),
            "sessionId": "S1"})
    }

    #[test]
    fn a_check_comes_before_the_context() {
        let facts = Facts {
            unrun_checks: 2,
            context: Some((80, Checkpoint::Current)),
            ..Facts::default()
        };
        assert_eq!(
            line(&facts).as_deref(),
            Some(
                "craft check --stage commit (2 declared checks cover files changed since it last ran)"
            )
        );
    }

    #[test]
    fn the_context_line_says_what_the_checkpoint_allows() {
        let at = |checkpoint| {
            line(&Facts {
                context: Some((78, checkpoint)),
                ..Facts::default()
            })
            .unwrap()
        };
        assert!(at(Checkpoint::Current).contains("compact or start a new session"));
        assert!(at(Checkpoint::Behind).contains("save a checkpoint before compacting"));
        assert!(!at(Checkpoint::Missing).contains("compact or start"));
        assert_eq!(line(&Facts::default()), None);
    }

    #[test]
    fn task_facts_come_after_the_check_and_before_the_context() {
        let ready = Ready {
            plan: "p".into(),
            task: "t2".into(),
            goal: "Add the export".into(),
        };
        let mut facts = Facts {
            unrun_checks: 1,
            unevidenced: Some(("ac-1".into(), "t1".into())),
            ready: Some(ready),
            context: Some((90, Checkpoint::Missing)),
        };
        assert!(line(&facts).unwrap().starts_with("craft check"));
        facts.unrun_checks = 0;
        assert_eq!(
            line(&facts).as_deref(),
            Some("acceptance \"ac-1\" of task t1 has no evidence yet")
        );
        facts.unevidenced = None;
        assert_eq!(
            line(&facts).as_deref(),
            Some("plan task t2 (Add the export) is ready: craft task-start --plan p --ready")
        );
        facts.ready = None;
        assert!(line(&facts).unwrap().starts_with("context 90 %"));
    }

    #[test]
    fn a_required_criterion_no_evidence_names_is_unevidenced_until_the_task_completes() {
        let record = |status: &str, evidence: Value| {
            json!({"status": status, "acceptanceEvidence": {"records": evidence},
                "engineeringContract": {"criteria": [
                    {"id": "optional", "required": false},
                    {"id": "ac-1", "required": true},
                    {"id": "ac-2", "required": true}]}})
        };
        assert_eq!(
            unevidenced(&record("working", json!([]))).as_deref(),
            Some("ac-1")
        );
        let failed_run = json!([{"criterionIds": ["ac-1"], "execution": "failed"}]);
        assert_eq!(
            unevidenced(&record("working", failed_run.clone())).as_deref(),
            Some("ac-2")
        );
        let both = json!([{"criterionIds": ["ac-1", "ac-2"]}]);
        assert_eq!(unevidenced(&record("working", both)), None);
        assert_eq!(unevidenced(&record("completed", json!([]))), None);
        assert_eq!(unevidenced(&json!({"status": "working"})), None);
    }

    #[test]
    fn a_plan_offers_its_next_task_only_once_the_held_one_is_completed() {
        let plan = |state: &str| {
            json!({"id": "p", "tasks": [
                {"id": "t1", "state": state, "goal": "First"},
                {"id": "t2", "state": "pending", "goal": "x".repeat(100)}]})
        };
        let queue = json!({"next": "t2"});
        assert_eq!(ready(&plan("running"), "t1", &queue), None);
        let offered = ready(&plan("completed"), "t1", &queue).unwrap();
        assert_eq!(offered.task, "t2");
        assert_eq!(offered.goal.chars().count(), GOAL_CHARS + 1);
        assert_eq!(
            ready(&plan("completed"), "t1", &json!({"next": null})),
            None
        );
        assert_eq!(ready(&plan("completed"), "gone", &queue), None);
    }

    #[test]
    fn pressure_needs_a_fresh_observation_of_this_session_past_the_threshold() {
        let settings = json!({});
        assert_eq!(pressure(&settings, &usage(780), "S1", NOW), Some(78));
        assert_eq!(pressure(&settings, &usage(700), "S1", NOW), None);
        assert_eq!(pressure(&settings, &usage(780), "S2", NOW), None);
        assert_eq!(pressure(&settings, &usage(780), "S1", NOW + 600_000), None);
        assert_eq!(pressure(&settings, &Value::Null, "S1", NOW), None);
        let lower = json!({"efficiency": {"context": {"compactAt": 0.6}}});
        assert_eq!(pressure(&lower, &usage(700), "S1", NOW), Some(70));
    }

    #[test]
    fn a_check_is_unrun_until_a_run_at_its_stage_or_later_follows_the_change() {
        let runs = |stages: &[(&str, i64)]| {
            json!({"runs": stages.iter().map(|(stage, at)|
                json!({"stage": stage, "at": timestamp::format(*at)})).collect::<Vec<_>>()})
        };
        assert!(unrun(&Value::Null, "commit", Some(NOW)));
        assert!(unrun(&Value::Null, "commit", None));
        assert!(!unrun(
            &runs(&[("commit", NOW)]),
            "commit",
            Some(NOW - 1000)
        ));
        assert!(!unrun(&runs(&[("pr", NOW)]), "commit", Some(NOW - 1000)));
        assert!(unrun(&runs(&[("step", NOW)]), "commit", Some(NOW - 1000)));
        assert!(unrun(
            &runs(&[("commit", NOW - 2000)]),
            "commit",
            Some(NOW - 1000)
        ));
        assert!(!unrun(&runs(&[("commit", NOW)]), "commit", None));
    }

    #[test]
    fn both_switches_of_session_advice_silence_it() {
        assert!(enabled(&json!({})));
        assert!(!enabled(
            &json!({"efficiency": {"advice": {"enabled": false}}})
        ));
        assert!(!enabled(
            &json!({"efficiency": {"advice": {"nextStep": false}}})
        ));
    }
}
