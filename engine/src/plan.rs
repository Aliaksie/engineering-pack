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
    escalate, finished, integrable, merged, owned, progress, ready, release_task, settle,
    state_after,
};
pub use scope::{Observed, baseline, changed, completion, covers, overlaps};
pub use views::{brief, closed, envelope, listing, markdown, queue, summary, view};

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

/// What a user approves of a plan: its goal and each task that is not abandoned, as
/// authored. Task progress leaves it unchanged; a changed, added or abandoned task changes
/// it. `None` for a plan with no such task, which there is nothing to approve of.
pub fn approval(plan: &Value) -> Option<String> {
    let authored: Vec<Value> = tasks(plan)
        .iter()
        .filter(|t| t["state"] != "abandoned")
        .map(|t| {
            let keys = [
                "id",
                "goal",
                "dependsOn",
                "acceptance",
                "files",
                "parent",
                "scope",
                "contract",
            ];
            let fields: serde_json::Map<String, Value> = keys
                .iter()
                .filter_map(|key| Some((key.to_string(), t.get(*key)?.clone())))
                .collect();
            Value::Object(fields)
        })
        .collect();
    if authored.is_empty() {
        return None;
    }
    Some(crate::digest(
        &serde_json::json!({"goal": plan["goal"], "tasks": authored}),
    ))
}

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
mod tests;
