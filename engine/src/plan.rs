//! Work plans: one goal of a work item (a feature, an issue, a backlog item) and its ordered
//! tasks with fixed states and dependencies, worked on across sessions. `ready` is derived
//! from dependencies and never stored, so completing a task propagates without a write.
mod format;
mod lifecycle;
mod scope;
mod views;

pub use format::{reference, slug, validate};
pub use lifecycle::{
    Assignment, Claim, Progress, apply, assign, claim, completable, conflicted, create, discard,
    escalate, integrable, merged, owned, progress, ready, release_task, state_after,
};
pub use scope::{Observed, baseline, changed, completion, covers, overlaps};
pub use views::{brief, envelope, listing, markdown, queue, summary, view};

use serde_json::Value;

type Result<T = ()> = std::result::Result<T, String>;

/// The plan format; each plan is written at the first version holding the fields it uses.
pub const FORMAT_VERSION: i64 = 4;
const FIRST_VERSION: i64 = 1;
const SCOPED_VERSION: i64 = 2;
const COORDINATED_VERSION: i64 = 3;
pub const RECORD_BYTES: usize = 131_072;
pub const TASKS: usize = 64;
const DEPENDENCIES: usize = 16;
const DECISIONS: usize = 32;
const REFERENCES: usize = 16;
const ITEMS: usize = 16;
const FILES: usize = 32;
const SCOPE_PATHS: usize = 32;
/// Uncommitted files a claim records as already changed; more leave the scope unchecked.
const BASE_FILES: usize = 64;
const ENVELOPE_BYTES: usize = 16_384;
const ENVELOPE_DECISIONS: usize = 8;
const ENVELOPE_EVIDENCE: usize = 3;
/// Open plans listed when a session starts, newest first.
pub const LISTED: usize = 3;
const LISTED_TASKS: usize = 8;
/// Claims a task allows unless its author sets `maxAttempts`, and the most it may set.
const ATTEMPTS: i64 = 3;
const MAX_ATTEMPTS: i64 = 10;
/// Minutes without a plan step after which a claim is shown stale; never released by age.
pub const STALE_MINUTES: i64 = 120;
/// Repositories one worker's isolated worktrees cover, and conflicted paths kept.
const WORK_REPOSITORIES: usize = 8;
const CONFLICT_PATHS: usize = 32;
/// Integration states a worktree task records; without one it waits to be integrated.
const INTEGRATION_STATES: &[&str] = &["conflict", "integrated", "escalated"];

pub const STATES: &[&str] = &[
    "pending",
    "running",
    "verifying",
    "completed",
    "blocked",
    "failed",
    "abandoned",
];
/// States a task is held in by the session that owns it.
const OWNED: &[&str] = &["running", "verifying", "blocked"];
/// States `plan-update` may set; the others follow from task work.
const EDITABLE_STATES: &[&str] = &["pending", "failed", "abandoned"];
const PLAN_STATUSES: &[&str] = &["open", "completed", "abandoned"];
const REFERENCE_KINDS: &[&str] = &["tracker", "document", "link"];
const PLAN_FIELDS: &[&str] = &[
    "formatVersion",
    "id",
    "goal",
    "status",
    "references",
    "decisions",
    "tasks",
    "revision",
    "created",
    "updated",
];
const TASK_FIELDS: &[&str] = &[
    "id",
    "goal",
    "state",
    "dependsOn",
    "files",
    "repositories",
    "acceptance",
    "owner",
    "result",
    "nextStep",
    "evidence",
    "updated",
    "parent",
    "scope",
    "contract",
    "returns",
    "scopeBase",
    "attempts",
    "maxAttempts",
    "verified",
    "work",
    "integration",
];
/// Task fields added at version 2, 3 and 4; a worker owner (`owner.parent`) also needs 4.
const SCOPED_FIELDS: &[&str] = &["parent", "scope", "contract", "returns", "scopeBase"];
const COORDINATED_FIELDS: &[&str] = &["attempts", "maxAttempts", "verified"];
const PARALLEL_FIELDS: &[&str] = &["work", "integration"];
/// Task fields a plan author sets; the rest follow from task work.
const AUTHORED: &[&str] = &[
    "id",
    "goal",
    "state",
    "dependsOn",
    "files",
    "repositories",
    "acceptance",
    "nextStep",
    "parent",
    "scope",
    "contract",
    "returns",
    "maxAttempts",
];
const CHANGE_FIELDS: &[&str] = &["goal", "status", "references", "decisions", "tasks"];
const BLOCKERS: &[&str] = &["permission-denied", "tool-unavailable", "blocked"];

fn tasks(plan: &Value) -> &[Value] {
    plan["tasks"].as_array().map_or(&[], Vec::as_slice)
}

fn find<'a>(plan: &'a Value, id: &str) -> Option<&'a Value> {
    tasks(plan).iter().find(|t| t["id"] == id)
}

fn find_mut<'a>(plan: &'a mut Value, id: &str) -> Option<&'a mut Value> {
    plan["tasks"]
        .as_array_mut()?
        .iter_mut()
        .find(|t| t["id"] == id)
}

/// The tasks `task` waits for: its dependencies and, since a parent completes only after
/// them, its children.
fn waits_for<'a>(tasks: &'a [Value], task: &'a Value) -> impl Iterator<Item = &'a Value> {
    let children = tasks
        .iter()
        .filter(move |t| t.get("parent") == Some(&task["id"]))
        .map(|t| &t["id"]);
    task["dependsOn"]
        .as_array()
        .into_iter()
        .flatten()
        .chain(children)
}

/// Whether a completed task's work has reached the coordinator: always, unless it worked
/// in isolated worktrees that are not integrated yet.
fn done(task: &Value) -> bool {
    task["state"] == "completed"
        && (task.get("work").is_none() || task["integration"]["state"] == "integrated")
}

/// `text` cut to at most 512 UTF-16 units.
fn clip(text: &str) -> String {
    let mut out = String::new();
    for c in text.chars() {
        if crate::values::js_length(&out) + c.len_utf16() > 512 {
            break;
        }
        out.push(c);
    }
    out
}

fn plain(value: &Value) -> &str {
    value.as_str().unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::timestamp;
    use serde_json::json;

    const NOW: i64 = 1_800_000_000_000;
    const ONE: &str = "aaaaaaaaaaaaaaaaaaaaaaaa";
    const TWO: &str = "bbbbbbbbbbbbbbbbbbbbbbbb";

    fn sample() -> Value {
        create(
            &json!({"id": "work", "goal": "Managed sessions", "references": [{"kind": "tracker", "id": "PROJ-1"}],
                "tasks": [
                    {"id": "a", "goal": "First"},
                    {"id": "b", "goal": "Second", "dependsOn": ["a"]},
                    {"id": "c", "goal": "Third", "dependsOn": ["a", "b"], "acceptance": ["Works"]},
                ]}),
            NOW,
        )
        .unwrap()
    }

    fn step<'a>(stage: &'a str, outcome: &'a str, complete: bool) -> Progress<'a> {
        Progress {
            stage,
            outcome: Some(outcome),
            complete,
            exhausted: false,
            verified: None,
            heads: &[],
            result: Some("done"),
            next_step: Some("next"),
            evidence: Some("check.log"),
        }
    }

    #[test]
    fn a_new_plan_validates_and_derives_readiness() {
        let plan = sample();
        validate(&plan).unwrap();
        assert_eq!(plan["revision"], 1);
        let shown = view(&plan);
        let ready: Vec<_> = shown["tasks"]
            .as_array()
            .unwrap()
            .iter()
            .map(|t| t["ready"].clone())
            .collect();
        assert_eq!(ready, [json!(true), json!(false), json!(false)]);
        assert!(
            shown["tasks"][0].get("ready").is_some() && plan["tasks"][0].get("ready").is_none()
        );
    }

    #[test]
    fn identities_dependencies_and_bounds_are_checked() {
        for id in ["", ".hidden", "Upper", "a/b", &"x".repeat(65)] {
            assert!(
                create(&json!({"id": id, "goal": "G"}), NOW).is_err(),
                "{id}"
            );
        }
        let cycle = json!({"id": "p", "goal": "G", "tasks": [
            {"id": "a", "goal": "A", "dependsOn": ["b"]}, {"id": "b", "goal": "B", "dependsOn": ["a"]}]});
        assert!(create(&cycle, NOW).unwrap_err().contains("cycle"));
        let unknown = json!({"id": "p", "goal": "G", "tasks": [{"id": "a", "goal": "A", "dependsOn": ["z"]}]});
        assert!(create(&unknown, NOW).unwrap_err().contains("unknown task"));
        let many: Vec<Value> = (0..=TASKS)
            .map(|i| json!({"id": format!("t{i}"), "goal": "G"}))
            .collect();
        assert!(create(&json!({"id": "p", "goal": "G", "tasks": many}), NOW).is_err());
        let state = json!({"id": "p", "goal": "G", "tasks": [{"id": "a", "goal": "A", "state": "running"}]});
        assert!(create(&state, NOW).is_err());
    }

    #[test]
    fn claims_start_ready_tasks_and_refuse_others() {
        let mut plan = sample();
        assert!(
            claim(&mut plan, "b", ONE, false, None, NOW)
                .unwrap_err()
                .contains("waits for a")
        );
        assert_eq!(
            claim(&mut plan, "a", ONE, false, None, NOW).unwrap(),
            Claim::Started
        );
        assert_eq!(plan["revision"], 2);
        assert_eq!(
            claim(&mut plan, "a", ONE, false, None, NOW).unwrap(),
            Claim::Held
        );
        assert_eq!(plan["revision"], 2);
        assert!(
            claim(&mut plan, "a", TWO, false, None, NOW)
                .unwrap_err()
                .contains("--take-over")
        );
        assert_eq!(
            claim(&mut plan, "a", TWO, true, None, NOW).unwrap(),
            Claim::TakenOver(ONE.into())
        );
        assert_eq!(plan["tasks"][0]["owner"]["session"], TWO);
        assert!(claim(&mut plan, "z", ONE, false, None, NOW).is_err());
        validate(&plan).unwrap();
    }

    #[test]
    fn steps_map_onto_fixed_states() {
        let cases = [
            ("implement", "passed", false, "running"),
            ("verify", "passed", false, "verifying"),
            ("verify", "quality-failure", false, "running"),
            ("implement", "blocked", false, "blocked"),
            ("implement", "permission-denied", false, "blocked"),
            ("verify", "prerequisite-resolved", false, "running"),
            ("verify", "passed", true, "completed"),
        ];
        for (stage, outcome, complete, expected) in cases {
            assert_eq!(
                state_after(&step(stage, outcome, complete)),
                expected,
                "{stage} {outcome}"
            );
        }
    }

    #[test]
    fn completing_a_task_readies_its_dependents_without_a_write_to_them() {
        let mut plan = sample();
        claim(&mut plan, "a", ONE, false, None, NOW).unwrap();
        assert!(progress(&mut plan, "a", TWO, &step("verify", "passed", true), NOW).is_err());
        progress(&mut plan, "a", ONE, &step("verify", "passed", false), NOW).unwrap();
        assert_eq!(plan["tasks"][0]["state"], "verifying");
        let before = plan["tasks"][1].clone();
        progress(
            &mut plan,
            "a",
            ONE,
            &step("verify", "passed", true),
            NOW + 1,
        )
        .unwrap();
        assert_eq!(plan["tasks"][0]["state"], "completed");
        assert!(plan["tasks"][0]["owner"].is_null());
        assert_eq!(plan["tasks"][0]["evidence"], json!(["check.log"]));
        assert_eq!(plan["tasks"][1], before);
        assert!(ready(&plan, &plan["tasks"][1]));
        assert!(!ready(&plan, &plan["tasks"][2]));
    }

    #[test]
    fn authored_changes_are_bounded_by_ownership_and_completion() {
        let mut plan = sample();
        claim(&mut plan, "a", ONE, false, None, NOW).unwrap();
        let abandon = json!({"tasks": [{"id": "a", "state": "abandoned"}]});
        assert!(
            apply(&mut plan, &abandon, Some(TWO), false, NOW)
                .unwrap_err()
                .contains("--take-over")
        );
        let goal = json!({"tasks": [{"id": "a", "goal": "Other"}]});
        assert!(
            apply(&mut plan, &goal, Some(ONE), false, NOW)
                .unwrap_err()
                .contains("goal is fixed")
        );
        let running = json!({"tasks": [{"id": "b", "state": "running"}]});
        assert!(apply(&mut plan, &running, None, false, NOW).is_err());
        assert!(apply(&mut plan, &json!({"status": "completed"}), None, false, NOW).is_err());
        apply(&mut plan, &abandon, Some(ONE), false, NOW).unwrap();
        assert!(plan["tasks"][0]["owner"].is_null());
        let rest = json!({"tasks": [{"id": "b", "state": "abandoned"}, {"id": "c", "state": "abandoned"}],
            "decisions": [{"id": "d1", "text": "Stop here"}]});
        apply(&mut plan, &rest, None, false, NOW).unwrap();
        apply(&mut plan, &json!({"status": "completed"}), None, false, NOW).unwrap();
        assert_eq!(plan["status"], "completed");
        assert_eq!(plan["decisions"][0]["at"], timestamp::format(NOW));
        assert!(claim(&mut plan, "a", ONE, false, None, NOW).is_err());
        let cycle = json!({"tasks": [{"id": "a", "dependsOn": ["c"]}]});
        assert!(
            apply(&mut sample(), &cycle, None, false, NOW)
                .unwrap_err()
                .contains("cycle")
        );
    }

    #[test]
    fn a_listing_shows_open_plans_newest_first() {
        let mut first = sample();
        claim(&mut first, "a", ONE, false, None, NOW - 120_000).unwrap();
        let mut second = create(
            &json!({"id": "x", "goal": "Other", "tasks": [{"id": "t", "goal": "T"}]}),
            NOW,
        )
        .unwrap();
        second["updated"] = json!(timestamp::format(NOW + 1));
        let mut closed = create(&json!({"id": "y", "goal": "Done"}), NOW).unwrap();
        closed["status"] = json!("completed");
        let listed = listing(vec![first, second, closed], ONE, NOW);
        assert_eq!(listed.len(), 2);
        assert_eq!(listed[0]["plan"], "x");
        assert_eq!(
            listed[1]["held"],
            json!([{"task": "a", "state": "running", "here": true, "idleMinutes": 2, "stale": false}])
        );
        assert_eq!(listed[1]["ready"], json!([]));
    }

    #[test]
    fn the_markdown_view_shows_tasks_and_decisions() {
        let mut plan = sample();
        apply(
            &mut plan,
            &json!({"decisions": [{"id": "d1", "text": "Use | pipes"}]}),
            None,
            false,
            NOW,
        )
        .unwrap();
        let text = markdown(&plan);
        assert!(text.starts_with("# work: Managed sessions"));
        assert!(text.contains("| a | ready | — | — | — |"));
        assert!(text.contains("| b | pending | a |"));
        assert!(text.contains("Use \\| pipes"));
        assert!(text.contains("References: tracker:PROJ-1"));
    }

    #[test]
    fn a_task_binding_names_its_plan_only_at_version_2() {
        use crate::session::binding;
        let plain = json!({"formatVersion": 1, "task": "a"});
        assert!(binding(&plain, None).unwrap().is_some());
        let planned = json!({"formatVersion": 2, "task": "a", "plan": "work"});
        assert!(binding(&planned, None).unwrap().is_some());
        for invalid in [
            json!({"formatVersion": 1, "task": "a", "plan": "work"}),
            json!({"formatVersion": 2, "task": "a"}),
            json!({"formatVersion": 2, "task": "a", "plan": "WORK"}),
        ] {
            assert!(binding(&invalid, None).is_err(), "{invalid}");
        }
    }

    fn nested() -> Value {
        create(
            &json!({"id": "children", "goal": "Child contracts", "tasks": [
                {"id": "p", "goal": "Parent", "scope": {"paths": ["src"]}},
                {"id": "c1", "goal": "First child", "parent": "p", "acceptance": ["Works"],
                    "scope": {"paths": ["src/a/**"]}, "returns": "The new API"},
                {"id": "c2", "goal": "Second child", "parent": "p", "dependsOn": ["c1"],
                    "acceptance": ["Uses the API"], "scope": {"paths": ["src/b/*.rs"], "worktree": "wt/c2"},
                    "contract": "contracts/c2.json"},
                {"id": "other", "goal": "Unrelated", "scope": {"paths": ["src/a/x.rs"]}},
            ], "decisions": [{"id": "d1", "text": "Keep it small"}]}),
            NOW,
        )
        .unwrap()
    }

    fn observed(head: &str, committed: &[&str], uncommitted: &[(&str, &str)]) -> Observed {
        Observed {
            path: String::new(),
            head: json!(head),
            committed: Some(committed.iter().map(|c| c.to_string()).collect()),
            uncommitted: uncommitted
                .iter()
                .map(|(p, d)| (p.to_string(), d.to_string()))
                .collect(),
            complete: true,
        }
    }

    const MINUTE: i64 = 60_000;

    fn complete(plan: &mut Value, task: &str, owner: &str) {
        claim(plan, task, owner, false, None, NOW).unwrap();
        progress(plan, task, owner, &step("verify", "passed", true), NOW).unwrap();
    }

    #[test]
    fn the_queue_groups_tasks_in_order_and_names_the_next_one() {
        let mut plan = create(
            &json!({"id": "q", "goal": "Queue", "tasks": [
                {"id": "a", "goal": "A"},
                {"id": "b", "goal": "B"},
                {"id": "c", "goal": "C", "dependsOn": ["a"]},
                {"id": "p", "goal": "Parent"},
                {"id": "k", "goal": "Child", "parent": "p"},
            ]}),
            NOW,
        )
        .unwrap();
        let shown = queue(&plan, ONE, &[], NOW);
        assert_eq!(shown["ready"], json!(["a", "b", "p", "k"]));
        assert_eq!(shown["next"], "a");
        assert_eq!(
            shown["waiting"],
            json!([{"task": "c", "on": [{"id": "a", "state": "pending"}], "stuck": false}])
        );
        // Children of a parent held here come first.
        claim(&mut plan, "p", ONE, false, None, NOW).unwrap();
        let shown = queue(&plan, ONE, &[], NOW);
        assert_eq!(shown["ready"], json!(["k", "a", "b"]));
        assert_eq!(shown["held"], json!([{"task": "p", "state": "running"}]));
        // Elsewhere for another session: stale after the threshold, orphaned when gone.
        let later = NOW + STALE_MINUTES * MINUTE;
        let other = queue(&plan, TWO, &[ONE], later);
        assert_eq!(
            other["elsewhere"],
            json!([{"task": "p", "state": "running", "owner": ONE,
            "idleMinutes": STALE_MINUTES, "stale": true, "orphaned": true}])
        );
        assert_eq!(
            queue(&plan, TWO, &[], NOW + MINUTE)["elsewhere"][0]["stale"],
            false
        );
        complete(&mut plan, "k", ONE);
        assert_eq!(
            queue(&plan, ONE, &[], NOW)["parents"],
            json!([{"task": "p", "state": "running"}])
        );
        // A dependency that failed or was abandoned leaves its dependents stuck.
        apply(
            &mut plan,
            &json!({"tasks": [{"id": "a", "state": "abandoned"}]}),
            None,
            false,
            NOW,
        )
        .unwrap();
        let shown = queue(&plan, ONE, &[], NOW);
        assert_eq!(
            shown["waiting"][0],
            json!({"task": "c", "on": [{"id": "a", "state": "abandoned"}], "stuck": true})
        );
        assert_eq!(shown["next"], "b");
    }

    #[test]
    fn starts_count_attempts_until_the_budget_is_spent() {
        let mut plan = sample();
        let fail = |plan: &mut Value| {
            claim(plan, "a", ONE, false, None, NOW).unwrap();
            apply(
                plan,
                &json!({"tasks": [{"id": "a", "state": "failed"}]}),
                Some(ONE),
                false,
                NOW,
            )
            .unwrap();
            apply(
                plan,
                &json!({"tasks": [{"id": "a", "state": "pending"}]}),
                Some(ONE),
                false,
                NOW,
            )
            .unwrap();
        };
        assert_eq!(plan["formatVersion"], 1);
        fail(&mut plan);
        assert_eq!(plan["formatVersion"], 3);
        // A take-over continues the same attempt.
        claim(&mut plan, "a", ONE, false, None, NOW).unwrap();
        claim(&mut plan, "a", TWO, true, None, NOW).unwrap();
        assert_eq!(plan["tasks"][0]["attempts"], 2);
        apply(
            &mut plan,
            &json!({"tasks": [{"id": "a", "state": "failed"}]}),
            Some(TWO),
            false,
            NOW,
        )
        .unwrap();
        assert_eq!(
            queue(&plan, ONE, &[], NOW)["retry"],
            json!([{"task": "a", "state": "failed", "attempts": 2, "maxAttempts": 3}])
        );
        apply(
            &mut plan,
            &json!({"tasks": [{"id": "a", "state": "pending"}]}),
            None,
            false,
            NOW,
        )
        .unwrap();
        fail(&mut plan);
        assert!(!ready(&plan, &plan["tasks"][0]));
        assert_eq!(queue(&plan, ONE, &[], NOW)["exhausted"][0]["attempts"], 3);
        let refused = claim(&mut plan, "a", ONE, false, None, NOW).unwrap_err();
        assert!(
            refused.contains("attempt budget exhausted (3 of 3)"),
            "{refused}"
        );
        apply(
            &mut plan,
            &json!({"tasks": [{"id": "a", "maxAttempts": 4}]}),
            None,
            false,
            NOW,
        )
        .unwrap();
        assert_eq!(
            claim(&mut plan, "a", ONE, false, None, NOW).unwrap(),
            Claim::Started
        );
        for bad in [json!(0), json!(11), json!("3")] {
            assert!(
                apply(
                    &mut plan.clone(),
                    &json!({"tasks": [{"id": "b", "maxAttempts": bad}]}),
                    None,
                    false,
                    NOW
                )
                .is_err()
            );
        }
    }

    #[test]
    fn a_spent_session_budget_fails_the_task_and_completion_records_its_verification() {
        let mut spent = step("verify", "quality-failure", false);
        spent.exhausted = true;
        assert_eq!(state_after(&spent), "failed");
        let mut plan = sample();
        claim(&mut plan, "a", ONE, false, None, NOW).unwrap();
        progress(&mut plan, "a", ONE, &spent, NOW).unwrap();
        assert_eq!(plan["tasks"][0]["state"], "failed");
        assert!(plan["tasks"][0]["owner"].is_null());
        apply(
            &mut plan,
            &json!({"tasks": [{"id": "a", "state": "pending"}]}),
            None,
            false,
            NOW,
        )
        .unwrap();
        claim(&mut plan, "a", ONE, false, None, NOW).unwrap();
        let verified =
            json!({"revision": "abc123", "contract": "0".repeat(64), "at": timestamp::format(NOW)});
        let mut passed = step("verify", "passed", false);
        passed.verified = Some(verified.clone());
        progress(&mut plan, "a", ONE, &passed, NOW).unwrap();
        assert!(
            plan["tasks"][0].get("verified").is_none(),
            "only a completion records it"
        );
        passed.complete = true;
        progress(&mut plan, "a", ONE, &passed, NOW).unwrap();
        assert_eq!(plan["tasks"][0]["verified"], verified);
        assert!(markdown(&plan).contains("Verified: abc123"));
        let mut broken = plan.clone();
        broken["tasks"][0]["verified"]["contract"] = json!("short");
        assert!(validate(&broken).is_err());
    }

    #[test]
    fn a_release_returns_a_held_task_to_pending() {
        let mut plan = create(
            &json!({"id": "r", "goal": "Release", "tasks": [
                {"id": "p", "goal": "Parent"}, {"id": "k", "goal": "Child", "parent": "p"}]}),
            NOW,
        )
        .unwrap();
        assert!(
            release_task(&mut plan, "p", ONE, false, None, NOW)
                .unwrap_err()
                .contains("only a held task")
        );
        claim(&mut plan, "p", ONE, false, None, NOW).unwrap();
        claim(&mut plan, "k", ONE, false, None, NOW).unwrap();
        assert!(
            release_task(&mut plan, "p", ONE, false, None, NOW)
                .unwrap_err()
                .contains("held child tasks: k")
        );
        assert!(
            release_task(&mut plan, "k", TWO, false, None, NOW)
                .unwrap_err()
                .contains("--take-over")
        );
        assert_eq!(
            release_task(&mut plan, "k", ONE, false, Some("Needs input"), NOW).unwrap(),
            ONE
        );
        assert_eq!(
            release_task(&mut plan, "p", TWO, true, None, NOW).unwrap(),
            ONE
        );
        let parent = &plan["tasks"][0];
        assert_eq!(
            (parent["state"].as_str(), parent["attempts"].as_i64()),
            (Some("pending"), Some(1))
        );
        assert_eq!(plan["tasks"][1]["nextStep"], "Needs input");
        assert_eq!(queue(&plan, ONE, &[], NOW)["ready"], json!(["p", "k"]));
        validate(&plan).unwrap();
    }

    #[test]
    fn later_fields_write_the_second_version_and_first_version_plans_stay_readable() {
        let plain = sample();
        assert_eq!(plain["formatVersion"], 1);
        let mut later = sample();
        apply(
            &mut later,
            &json!({"tasks": [{"id": "a", "returns": "A result"}]}),
            None,
            false,
            NOW,
        )
        .unwrap();
        assert_eq!(later["formatVersion"], 2);
        let mut downgraded = later.clone();
        downgraded["formatVersion"] = json!(1);
        assert!(validate(&downgraded).unwrap_err().contains("version"));
        let mut future = plain.clone();
        future["formatVersion"] = json!(FORMAT_VERSION + 1);
        assert!(validate(&future).unwrap_err().contains("older writers"));
        validate(&nested()).unwrap();
        assert_eq!(nested()["formatVersion"], 2);
    }

    #[test]
    fn parents_nest_one_level_and_wait_for_their_children() {
        let mut plan = nested();
        let deep = json!({"tasks": [{"id": "g", "goal": "Grandchild", "parent": "c1"}]});
        assert!(
            apply(&mut plan.clone(), &deep, None, false, NOW)
                .unwrap_err()
                .contains("one level")
        );
        let unknown = json!({"tasks": [{"id": "g", "goal": "G", "parent": "z"}]});
        assert!(
            apply(&mut plan.clone(), &unknown, None, false, NOW)
                .unwrap_err()
                .contains("unknown parent")
        );
        let own = json!({"tasks": [{"id": "g", "goal": "G", "parent": "g"}]});
        assert!(
            apply(&mut plan.clone(), &own, None, false, NOW)
                .unwrap_err()
                .contains("own parent")
        );
        // A child waiting for its parent could never start.
        let waits = json!({"tasks": [{"id": "c1", "dependsOn": ["p"]}]});
        assert!(
            apply(&mut plan.clone(), &waits, None, false, NOW)
                .unwrap_err()
                .contains("cycle")
        );
        claim(&mut plan, "p", ONE, false, None, NOW).unwrap();
        assert_eq!(
            claim(&mut plan, "c1", ONE, false, None, NOW).unwrap(),
            Claim::Started
        );
        assert!(
            completion(&plan, "p", Some(&[]))
                .unwrap_err()
                .contains("c1, c2")
        );
        completion(&plan, "c1", Some(&["src/a/lib.rs".into()])).unwrap();
        let text = markdown(&plan);
        assert!(text.contains("| ↳ c1 | running |"));
        assert!(text.contains("Part of: p"));
        assert!(text.contains("Owns: src/b/*.rs"));
    }

    #[test]
    fn scope_patterns_cover_files_and_directories() {
        for (pattern, path, covered) in [
            ("src", "src/a/b.rs", true),
            ("src/a.rs", "src/a.rs", true),
            ("src/a.rs", "src/a.rs.bak", false),
            ("src/*.rs", "src/a.rs", true),
            ("src/*.rs", "src/a/b.rs", false),
            ("src/*", "src/a/b.rs", true),
            ("src/*.rs", "src/a.txt", false),
            ("**/*.md", "docs/x/y.md", true),
            ("**/*.md", "y.md", true),
            ("docs/**/z.md", "docs/z.md", true),
            ("docs/**/z.md", "src/z.md", false),
            ("src", "srcs/a", false),
        ] {
            assert_eq!(covers(pattern, path), covered, "{pattern} {path}");
        }
        for invalid in ["/abs", "../up", "a//b", "a/**b", "./a"] {
            let plan = json!({"id": "p", "goal": "G", "tasks": [{"id": "a", "goal": "A", "scope": {"paths": [invalid]}}]});
            assert!(create(&plan, NOW).is_err(), "{invalid}");
        }
        let empty = json!({"id": "p", "goal": "G", "tasks": [{"id": "a", "goal": "A", "scope": {"paths": []}}]});
        assert!(create(&empty, NOW).is_err());
    }

    #[test]
    fn completion_counts_only_changes_after_the_claim_outside_owned_paths() {
        let head = "a".repeat(40);
        let before = [observed(
            &head,
            &[],
            &[
                ("README.md", "1111111111111111aaaa"),
                ("gone.txt", "missing"),
            ],
        )];
        let base = baseline(&before);
        assert_eq!(base["complete"], true);
        let mut plan = nested();
        claim(&mut plan, "other", ONE, false, Some(base.clone()), NOW).unwrap();
        assert_eq!(find(&plan, "other").unwrap()["scopeBase"], base);
        // Already changed and untouched since: not counted. Edited again: counted.
        let same = [observed(
            &head,
            &["src/a/x.rs"],
            &[("README.md", "1111111111111111aaaa")],
        )];
        assert_eq!(changed(&base, &same), Some(vec!["src/a/x.rs".to_string()]));
        completion(&plan, "other", changed(&base, &same).as_deref()).unwrap();
        let edited = [observed(&head, &[], &[("README.md", "2222222222222222")])];
        let refused = completion(&plan, "other", changed(&base, &edited).as_deref()).unwrap_err();
        assert!(
            refused.starts_with("scope exceeded") && refused.contains("README.md"),
            "{refused}"
        );
        // Unknown baselines are reported, not guessed.
        let mut unknown = observed(&head, &[], &[]);
        unknown.committed = None;
        assert_eq!(changed(&base, &[unknown]), None);
        let report = completion(&plan, "other", None).unwrap();
        assert_eq!(report["checked"], false);
        let crowded: Vec<(String, String)> = (0..=BASE_FILES)
            .map(|i| (format!("f{i}"), "0".repeat(64)))
            .collect();
        let full = Observed {
            uncommitted: crowded,
            ..observed(&head, &[], &[])
        };
        assert_eq!(baseline(&[full])["complete"], false);
        // Completion releases the task and drops what its claim saw.
        progress(
            &mut plan,
            "other",
            ONE,
            &step("verify", "passed", true),
            NOW,
        )
        .unwrap();
        assert!(find(&plan, "other").unwrap().get("scopeBase").is_none());
        // A task without owned paths is not checked.
        assert_eq!(
            completion(&sample(), "a", Some(&["any".into()])).unwrap(),
            Value::Null
        );
    }

    #[test]
    fn overlaps_list_tasks_that_may_work_at_once_on_shared_paths() {
        let plan = nested();
        // c1 and other share src/a; p is the parent of both children; c2 waits for c1.
        assert_eq!(
            json!(overlaps(&plan)),
            json!([{"tasks": ["p", "other"], "paths": ["src ~ src/a/x.rs"]},
                   {"tasks": ["c1", "other"], "paths": ["src/a/** ~ src/a/x.rs"]}])
        );
        assert!(view(&plan)["overlaps"].is_array());
        assert!(view(&sample()).get("overlaps").is_none());
        assert!(markdown(&plan).contains("## Overlapping scopes"));
    }

    #[test]
    fn an_envelope_carries_the_task_context_and_converts_to_a_worker_brief() {
        let mut plan = nested();
        claim(&mut plan, "p", ONE, false, None, NOW).unwrap();
        claim(&mut plan, "c1", ONE, false, None, NOW).unwrap();
        let mut done = step("verify", "passed", true);
        done.result = Some("Added a::api");
        progress(&mut plan, "c1", ONE, &done, NOW).unwrap();
        let checkpoint = json!({"session": ONE, "head": "h", "updated": "t", "nextStep": "n"});
        let shown = envelope(&plan, "c2", checkpoint.clone()).unwrap();
        assert_eq!(shown["task"]["id"], "c2");
        assert_eq!(shown["task"]["ready"], true);
        assert_eq!(shown["task"]["contract"], "contracts/c2.json");
        assert_eq!(shown["dependencies"][0]["result"], "Added a::api");
        assert_eq!(shown["dependencies"][0]["evidence"], json!(["check.log"]));
        assert_eq!(shown["parent"]["id"], "p");
        assert_eq!(shown["parent"]["checkpoint"], checkpoint);
        assert_eq!(shown["decisions"][0]["text"], "Keep it small");
        assert!(shown.get("tasks").is_none() && shown.to_string().len() <= ENVELOPE_BYTES);
        let parent = envelope(&plan, "p", Value::Null).unwrap();
        assert_eq!(parent["children"].as_array().unwrap().len(), 2);
        let brief = brief(&shown).unwrap();
        crate::workers::lifecycle::input(&json!({
            "id": "w1", "offer": "0".repeat(64), "scope": "task", "brief": brief,
        }))
        .unwrap();
        assert_eq!(brief["sources"], json!(["c1 returned: Added a::api"]));
        assert_eq!(
            brief["restrictions"],
            json!([
                "Change only files matching src/b/*.rs",
                "Work in the worktree wt/c2"
            ])
        );
        assert!(
            self::brief(&envelope(&plan, "p", Value::Null).unwrap())
                .unwrap_err()
                .contains("acceptance")
        );
        assert!(envelope(&plan, "zz", Value::Null).is_err());
    }

    #[test]
    fn references_parse_from_command_lines() {
        assert_eq!(
            reference("tracker:PROJ-1").unwrap(),
            json!({"kind": "tracker", "id": "PROJ-1"})
        );
        assert!(reference("PROJ-1").is_err());
        assert!(reference("wiki:x").is_err());
    }

    const THREE: &str = "cccccccccccccccccccccccc";
    const BASE: &str = "1111111111111111111111111111111111111111";
    const HEAD: &str = "2222222222222222222222222222222222222222";

    fn parallel() -> Value {
        create(
            &json!({"id": "parallel", "goal": "Parallel", "tasks": [
                {"id": "a", "goal": "A", "acceptance": ["A works"], "scope": {"paths": ["src/a/**"]}},
                {"id": "b", "goal": "B", "scope": {"paths": ["src/b/**"]}},
                {"id": "c", "goal": "C", "dependsOn": ["a"], "scope": {"paths": ["src/c/**"]}},
                {"id": "d", "goal": "D", "scope": {"paths": ["src/a/inner.rs"]}},
                {"id": "free", "goal": "Owns nothing"},
            ]}),
            NOW,
        )
        .unwrap()
    }

    fn worktrees(task: &str) -> Value {
        json!({"owner": ONE, "repositories": [{"path": "", "worktree": format!(".craft/worktrees/parallel/{task}"),
            "branch": format!("craft/parallel/{task}"), "base": BASE}]})
    }

    fn assigned<'a>(worker: &'a str, limit: i64, work: Option<Value>) -> Assignment<'a> {
        Assignment {
            parent: ONE,
            worker,
            limit,
            base: None,
            work,
        }
    }

    fn finish(plan: &mut Value, task: &str, owner: &str) {
        let heads = [(String::new(), HEAD.to_string())];
        let mut done = step("verify", "passed", true);
        done.heads = &heads;
        progress(plan, task, owner, &done, NOW).unwrap();
    }

    #[test]
    fn workers_take_owned_ready_tasks_within_the_limit_and_without_shared_paths() {
        let mut plan = parallel();
        let refused = |plan: &mut Value, task: &str, worker: &str, limit: i64| {
            assign(plan, task, assigned(worker, limit, None), NOW).unwrap_err()
        };
        assert!(refused(&mut plan, "free", TWO, 3).contains("owns no paths"));
        assert!(refused(&mut plan, "a", TWO, 0).contains("delegation is disabled"));
        assert!(refused(&mut plan, "a", ONE, 3).contains("its own session"));
        assert!(refused(&mut plan, "c", TWO, 3).contains("not ready"));
        assign(&mut plan, "a", assigned(TWO, 2, Some(worktrees("a"))), NOW).unwrap();
        let a = find(&plan, "a").unwrap();
        assert_eq!(
            (a["state"].as_str(), a["owner"]["parent"].as_str()),
            (Some("running"), Some(ONE))
        );
        assert_eq!(plan["formatVersion"], 4);
        assert_eq!(plan["revision"], 2);
        // d owns a path inside a's paths; b owns its own.
        assert!(
            refused(&mut plan, "d", THREE, 3)
                .contains("running tasks also own: a (src/a/inner.rs ~ src/a/**)")
        );
        assign(&mut plan, "b", assigned(THREE, 2, None), NOW).unwrap();
        let mut more = plan.clone();
        apply(
            &mut more,
            &json!({"tasks": [{"id": "e", "goal": "E", "scope": {"paths": ["docs"]}}]}),
            None,
            false,
            NOW,
        )
        .unwrap();
        assert!(refused(&mut more, "e", "dddddddddddddddddddddddd", 2).contains("2 of 2 workers"));
        assert_eq!(queue(&plan, ONE, &[], NOW)["workers"], 2);
        assert_eq!(queue(&plan, ONE, &[], NOW)["elsewhere"][0]["worker"], true);
        // The assigning session may take its worker's task back; the worktrees go with it.
        assert_eq!(
            release_task(&mut plan, "a", ONE, false, None, NOW).unwrap(),
            TWO
        );
        let a = find(&plan, "a").unwrap();
        assert!(a.get("work").is_none() && a["owner"].is_null());
        validate(&plan).unwrap();
    }

    #[test]
    fn a_worktree_task_readies_its_dependents_only_once_integrated() {
        let mut plan = parallel();
        assign(&mut plan, "a", assigned(TWO, 3, Some(worktrees("a"))), NOW).unwrap();
        assert!(integrable(&plan, "a", ONE).unwrap_err().contains("running"));
        assert!(
            progress(
                &mut plan.clone(),
                "a",
                TWO,
                &step("verify", "passed", true),
                NOW
            )
            .unwrap_err()
            .contains("head of its worktree branch")
        );
        finish(&mut plan, "a", TWO);
        assert_eq!(
            find(&plan, "a").unwrap()["work"]["repositories"][0]["head"],
            HEAD
        );
        assert!(!ready(&plan, find(&plan, "c").unwrap()));
        assert!(!completable(&plan));
        let shown = queue(&plan, ONE, &[], NOW);
        assert_eq!(shown["integrate"], json!([{"task": "a"}]));
        assert_eq!(
            shown["waiting"][0],
            json!({"task": "c", "on": [{"id": "a", "state": "completed"}], "stuck": false})
        );
        assert!(
            integrable(&plan, "a", TWO)
                .unwrap_err()
                .contains("session that assigned it")
        );
        integrable(&plan, "a", ONE).unwrap();
        assert!(merged(&mut plan, "a", &[(String::new(), BASE.to_string())], NOW).unwrap());
        let a = find(&plan, "a").unwrap();
        assert_eq!(
            (
                a["integration"]["state"].as_str(),
                a["work"]["repositories"][0]["merged"].as_str()
            ),
            (Some("integrated"), Some(BASE))
        );
        assert!(ready(&plan, find(&plan, "c").unwrap()));
        assert!(
            integrable(&plan, "a", ONE)
                .unwrap_err()
                .contains("integrated already")
        );
        assert!(markdown(&plan).contains("Integration: integrated"));
        validate(&plan).unwrap();
    }

    #[test]
    fn conflicts_return_the_task_to_its_integrator_until_the_budget_escalates_it() {
        let mut plan = parallel();
        apply(
            &mut plan,
            &json!({"tasks": [{"id": "a", "goal": "A", "maxAttempts": 2}]}),
            None,
            false,
            NOW,
        )
        .unwrap();
        assign(&mut plan, "a", assigned(TWO, 3, Some(worktrees("a"))), NOW).unwrap();
        finish(&mut plan, "a", TWO);
        let paths = vec!["src/a/x.rs".to_string()];
        assert!(!conflicted(&mut plan, "a", ONE, &paths, NOW).unwrap());
        let a = find(&plan, "a").unwrap().clone();
        assert_eq!(
            (
                a["state"].as_str(),
                a["owner"]["session"].as_str(),
                a["attempts"].as_i64()
            ),
            (Some("running"), Some(ONE), Some(2))
        );
        assert_eq!(a["integration"]["paths"], json!(paths));
        assert!(a.get("verified").is_none());
        finish(&mut plan, "a", ONE);
        assert!(conflicted(&mut plan, "a", ONE, &paths, NOW).unwrap());
        let a = find(&plan, "a").unwrap();
        assert_eq!(
            (a["state"].as_str(), a["integration"]["state"].as_str()),
            (Some("completed"), Some("escalated"))
        );
        assert_eq!(queue(&plan, ONE, &[], NOW)["needsUser"][0]["task"], "a");
        assert_eq!(
            listing(vec![plan.clone()], ONE, NOW)[0]["needsUser"],
            json!(["a"])
        );
        // Discarding drops the result: pending again, its worktrees returned for removal.
        assert!(
            discard(&mut plan.clone(), "a", TWO, NOW)
                .unwrap_err()
                .contains("no worktrees")
        );
        let work = discard(&mut plan, "a", ONE, NOW).unwrap();
        assert_eq!(work["repositories"][0]["branch"], "craft/parallel/a");
        let a = find(&plan, "a").unwrap();
        assert!(
            a["state"] == "pending" && a.get("work").is_none() && a.get("integration").is_none()
        );
        validate(&plan).unwrap();
    }

    #[test]
    fn an_integrator_escalates_by_hand_and_parents_wait_for_integration() {
        let mut plan = create(
            &json!({"id": "parallel", "goal": "Parallel", "tasks": [
                {"id": "p", "goal": "Parent", "scope": {"paths": ["src"]}},
                {"id": "k", "goal": "Child", "parent": "p", "scope": {"paths": ["src/k/**"]}},
            ]}),
            NOW,
        )
        .unwrap();
        claim(&mut plan, "p", ONE, false, None, NOW).unwrap();
        // A child's paths lie inside its parent's; they never block each other.
        assign(&mut plan, "k", assigned(TWO, 3, Some(worktrees("k"))), NOW).unwrap();
        finish(&mut plan, "k", TWO);
        assert!(
            completion(&plan, "p", None)
                .unwrap_err()
                .contains("not integrated: k")
        );
        escalate(&mut plan, "k", ONE, "Needs a product decision", NOW).unwrap();
        assert_eq!(
            find(&plan, "k").unwrap()["integration"]["reason"],
            "Needs a product decision"
        );
        merged(&mut plan, "k", &[(String::new(), BASE.to_string())], NOW).unwrap();
        completion(&plan, "p", None).unwrap();
        // Setting a worktree task back by hand drops its worktrees too.
        apply(
            &mut plan,
            &json!({"tasks": [{"id": "k", "goal": "Child", "state": "pending"}]}),
            Some(ONE),
            false,
            NOW,
        )
        .unwrap();
        assert!(find(&plan, "k").unwrap().get("work").is_none());
        validate(&plan).unwrap();
    }

    #[test]
    fn worker_fields_write_the_fourth_version_only_when_used() {
        let mut plan = parallel();
        claim(&mut plan, "a", ONE, false, None, NOW).unwrap();
        assert_eq!(plan["formatVersion"], 3);
        let mut forged = plan.clone();
        forged["tasks"][0]["owner"]["parent"] = json!(TWO);
        assert!(
            validate(&forged)
                .unwrap_err()
                .contains("version does not match")
        );
        forged["formatVersion"] = json!(4);
        validate(&forged).unwrap();
        forged["tasks"][0]["owner"]["parent"] = json!(ONE);
        assert!(validate(&forged).unwrap_err().contains("owner is invalid"));
        forged["tasks"][0]["owner"]["parent"] = json!(TWO);
        forged["tasks"][0]["work"] = json!({"owner": ONE, "repositories": [{"path": "../x", "worktree": "w", "branch": "b", "base": BASE}]});
        assert!(
            validate(&forged)
                .unwrap_err()
                .contains("worktree record is invalid")
        );
    }
}
