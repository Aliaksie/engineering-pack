//! Task work over a plan: authored edits, claims, attempts, progress, worker assignment and
//! integration of isolated worktrees.
use super::format::{decision, format_version, identity, line, references, task, validate, work};
use super::scope::{ids, unowned};
use super::{
    ATTEMPTS, AUTHORED, BLOCKERS, CHANGE_FIELDS, CONFLICT_PATHS, EDITABLE_STATES, FIRST_VERSION,
    ITEMS, OWNED, PLAN_STATUSES, Result, TASKS, clip, done, find, find_mut, tasks,
};
use crate::timestamp;
use crate::values::safe_integer;
use serde_json::{Map, Value, json};

/// A new plan from `{id, goal, references?, decisions?, tasks?}` at `now`.
pub fn create(input: &Value, now: i64) -> Result<Value> {
    let map = input
        .as_object()
        .filter(|m| {
            m.keys()
                .all(|k| ["id", "goal", "references", "decisions", "tasks"].contains(&k.as_str()))
        })
        .ok_or("a new plan is {id, goal, references?, decisions?, tasks?}")?;
    let at = timestamp::format(now);
    let mut plan = json!({
        "formatVersion": FIRST_VERSION,
        "id": identity(map.get("id"), "plan")?,
        "goal": line(map.get("goal"), "plan goal")?,
        "status": "open",
        "references": [], "decisions": [], "tasks": [],
        "revision": 1, "created": at, "updated": at,
    });
    let changes = Map::from_iter(
        ["references", "decisions", "tasks"]
            .iter()
            .filter_map(|k| map.get(*k).map(|v| (k.to_string(), v.clone()))),
    );
    if !changes.is_empty() {
        apply(&mut plan, &Value::Object(changes), None, false, now)?;
        plan["revision"] = json!(1);
    }
    Ok(plan)
}

/// Whether a task waits for nothing: pending, with attempts left and every dependency
/// completed and integrated.
pub fn ready(plan: &Value, task: &Value) -> bool {
    task["state"] == "pending"
        && attempts_left(task)
        && task["dependsOn"]
            .as_array()
            .into_iter()
            .flatten()
            .all(|d| find(plan, d.as_str().unwrap_or_default()).is_some_and(done))
}

/// Whether every task is completed and integrated, or abandoned, so the plan may be
/// completed.
pub fn completable(plan: &Value) -> bool {
    tasks(plan)
        .iter()
        .all(|t| done(t) || t["state"] == "abandoned")
}

/// Claims `task` has started and the most it allows.
pub(super) fn attempts(task: &Value) -> (i64, i64) {
    let count = |key: &str, default: i64| safe_integer(&task[key]).unwrap_or(default);
    (count("attempts", 0), count("maxAttempts", ATTEMPTS))
}

/// Whether `task` may start once more.
fn attempts_left(task: &Value) -> bool {
    let (made, most) = attempts(task);
    made < most
}

fn touch(plan: &mut Value, now: i64) {
    let revision = plan["revision"].as_i64().unwrap_or(0);
    plan["revision"] = json!(revision + 1);
    plan["updated"] = json!(timestamp::format(now));
    plan["formatVersion"] = json!(format_version(plan));
}

/// A task no longer held keeps no record of what its claim saw.
fn release(task: &mut Value) {
    task["owner"] = Value::Null;
    if let Some(task) = task.as_object_mut() {
        task.remove("scopeBase");
    }
}

/// A task set back for new work keeps no worktrees; returns those it had, for removal.
fn unassign(task: &mut Value) -> Option<Value> {
    let task = task.as_object_mut()?;
    task.remove("integration");
    task.remove("work")
}

fn owned_elsewhere(task: &Value, caller: Option<&str>) -> bool {
    task["owner"]["session"]
        .as_str()
        .is_some_and(|owner| Some(owner) != caller)
}

/// Apply authored changes `{goal?, status?, references?, decisions?, tasks?}`. Tasks are
/// added or edited by id; a decision with a known id replaces its text. A task another
/// session owns is changed only with `take_over`. `caller` is the session directory id.
pub fn apply(
    plan: &mut Value,
    changes: &Value,
    caller: Option<&str>,
    take_over: bool,
    now: i64,
) -> Result {
    let changes = changes
        .as_object()
        .filter(|m| !m.is_empty() && m.keys().all(|k| CHANGE_FIELDS.contains(&k.as_str())))
        .ok_or("plan changes are {goal?, status?, references?, decisions?, tasks?}")?;
    let at = timestamp::format(now);
    if let Some(goal) = changes.get("goal") {
        plan["goal"] = line(Some(goal), "plan goal")?;
    }
    if let Some(references) = changes.get("references") {
        plan["references"] = self::references(Some(references))?;
    }
    for item in changes
        .get("decisions")
        .map(|d| d.as_array().ok_or("plan decisions must be a list"))
        .transpose()?
        .into_iter()
        .flatten()
    {
        let mut decided = decision(item)?;
        decided["at"] = json!(at);
        let decisions = plan["decisions"].as_array_mut().expect("validated");
        match decisions.iter_mut().find(|d| d["id"] == decided["id"]) {
            Some(existing) => *existing = decided,
            None => decisions.push(decided),
        }
    }
    for item in changes
        .get("tasks")
        .map(|t| t.as_array().ok_or("plan tasks must be a list"))
        .transpose()?
        .into_iter()
        .flatten()
    {
        let fields = item
            .as_object()
            .filter(|m| m.keys().all(|k| AUTHORED.contains(&k.as_str())))
            .ok_or("an authored task is {id, goal, state?, dependsOn?, files?, repositories?, acceptance?, nextStep?, parent?, scope?, contract?, returns?}")?;
        let id = identity(fields.get("id"), "task")?;
        if let Some(state) = fields.get("state")
            && !state.as_str().is_some_and(|s| EDITABLE_STATES.contains(&s))
        {
            return Err(format!(
                "a plan update sets a task {}; other states follow from task work",
                EDITABLE_STATES.join(", ")
            ));
        }
        let merged = match find(plan, &id) {
            None => {
                if tasks(plan).len() == TASKS {
                    return Err(format!("a plan holds at most {TASKS} tasks"));
                }
                Value::Object(fields.clone())
            }
            Some(existing) => {
                if owned_elsewhere(existing, caller) && !take_over {
                    return Err(format!(
                        "task {id} is owned by another session; pass --take-over to change it"
                    ));
                }
                if fields.get("goal").is_some_and(|g| *g != existing["goal"])
                    && existing["state"] != "pending"
                {
                    return Err(format!(
                        "task {id} has started; its goal is fixed, add a new task instead"
                    ));
                }
                let mut merged = existing.clone();
                for (key, value) in fields {
                    merged[key] = value.clone();
                }
                if fields.contains_key("state") {
                    release(&mut merged);
                    unassign(&mut merged);
                }
                merged
            }
        };
        let mut checked = task(&merged, &at)?;
        checked["updated"] = json!(at);
        match find_mut(plan, &id) {
            Some(existing) => *existing = checked,
            None => plan["tasks"]
                .as_array_mut()
                .expect("validated")
                .push(checked),
        }
    }
    if let Some(status) = changes.get("status") {
        match status.as_str() {
            Some("completed") if !completable(plan) => {
                return Err(
                    "a plan is completed only when every task is completed or abandoned, and worktree tasks integrated".into(),
                );
            }
            Some(status) if PLAN_STATUSES.contains(&status) => plan["status"] = json!(status),
            _ => return Err("plan status must be open, completed or abandoned".into()),
        }
    }
    touch(plan, now);
    validate(plan)
}

/// What claiming a task changed.
#[derive(Debug, PartialEq, Eq)]
pub enum Claim {
    /// This session already owns it.
    Held,
    /// A ready task now runs in this session.
    Started,
    /// Another session owned it; its directory id.
    TakenOver(String),
}

/// Claim `task` for the session directory `owner`: a ready task starts running; one this
/// session holds is kept; one another session holds moves only with `take_over`. A task
/// that owns paths records `base`, the source its claim saw (see [`baseline`]), when it
/// starts; a take-over keeps the base of the first claim.
pub fn claim(
    plan: &mut Value,
    task: &str,
    owner: &str,
    take_over: bool,
    base: Option<Value>,
    now: i64,
) -> Result<Claim> {
    if plan["status"] != "open" {
        return Err(format!(
            "plan {} is {}; reopen it to work on its tasks",
            plan["id"], plan["status"]
        ));
    }
    let found =
        find(plan, task).ok_or_else(|| format!("plan {} has no task {task}", plan["id"]))?;
    let state = found["state"].as_str().unwrap_or_default();
    let claimed = match found["owner"]["session"].as_str() {
        Some(current) if current == owner => Claim::Held,
        Some(current) if take_over => Claim::TakenOver(current.to_string()),
        Some(_) => {
            return Err(format!(
                "task {task} is {state} in another session; pass --take-over to continue it here (that session may still be working)"
            ));
        }
        None if ready(plan, found) => Claim::Started,
        None if state == "pending" && !attempts_left(found) => {
            let (made, most) = attempts(found);
            return Err(format!(
                "task {task} attempt budget exhausted ({made} of {most}); raise maxAttempts with plan-update or abandon it"
            ));
        }
        None if state == "pending" => {
            let waiting: Vec<&str> = found["dependsOn"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
                .filter(|d| find(plan, d).is_some_and(|t| !done(t)))
                .collect();
            return Err(format!(
                "task {task} is not ready: it waits for {} (completed and integrated)",
                waiting.join(", ")
            ));
        }
        None => {
            return Err(format!(
                "task {task} is {state}; set it pending with plan-update to work on it again"
            ));
        }
    };
    if claimed == Claim::Held {
        return Ok(claimed);
    }
    if claimed == Claim::Started {
        unowned(plan, found)?;
    }
    let at = timestamp::format(now);
    let entry = find_mut(plan, task).expect("found");
    if claimed == Claim::Started {
        entry["state"] = json!("running");
        entry["attempts"] = json!(attempts(entry).0 + 1);
        if let Some(base) = base.filter(|_| entry.get("scope").is_some()) {
            entry["scopeBase"] = base;
        }
    }
    entry["owner"] = json!({"session": owner, "since": at});
    entry["updated"] = json!(at);
    touch(plan, now);
    Ok(claimed)
}

/// Return `task` to `pending` for any session to claim. Only its owner `caller` releases
/// it, or another session with `take_over`; a parent keeps its claim while a child is held.
/// Returns the session directory that held it.
pub fn release_task(
    plan: &mut Value,
    task: &str,
    caller: &str,
    take_over: bool,
    note: Option<&str>,
    now: i64,
) -> Result<String> {
    let found =
        find(plan, task).ok_or_else(|| format!("plan {} has no task {task}", plan["id"]))?;
    let Some(holder) = found["owner"]["session"].as_str().map(str::to_string) else {
        return Err(format!(
            "task {task} is {}; only a held task is released",
            found["state"].as_str().unwrap_or_default()
        ));
    };
    // The session that assigned a worker may take its task back.
    let assigned = found["owner"]["parent"] == caller;
    if holder != caller && !assigned && !take_over {
        return Err(format!(
            "task {task} is held by another session; pass --take-over to release it (that session may still be working)"
        ));
    }
    let held: Vec<&Value> = tasks(plan)
        .iter()
        .filter(|t| t.get("parent") == Some(&found["id"]) && !t["owner"].is_null())
        .collect();
    if !held.is_empty() {
        return Err(format!(
            "task {task} has held child tasks: {}; release them first",
            ids(&held)
        ));
    }
    let at = timestamp::format(now);
    let entry = find_mut(plan, task).expect("found");
    entry["state"] = json!("pending");
    release(entry);
    unassign(entry);
    if let Some(note) = note {
        entry["nextStep"] = line(Some(&json!(note)), "release reason")?;
    }
    entry["updated"] = json!(at);
    touch(plan, now);
    validate(plan)?;
    Ok(holder)
}

/// Tasks held for workers that `parent` assigned and that still run.
pub(super) fn workers_of<'a>(plan: &'a Value, parent: &str) -> Vec<&'a Value> {
    tasks(plan)
        .iter()
        .filter(|t| t["owner"]["parent"] == parent)
        .filter(|t| OWNED.contains(&t["state"].as_str().unwrap_or_default()))
        .collect()
}

/// One worker assignment: who assigns, who works, at most how many workers run at once,
/// the claim's source (see [`baseline`]) and the isolated worktrees (see [`work`]), if any.
pub struct Assignment<'a> {
    pub parent: &'a str,
    pub worker: &'a str,
    pub limit: i64,
    pub base: Option<Value>,
    pub work: Option<Value>,
}

/// Claim the ready task `task` for a worker session that the session `parent` delegates
/// to. The task must own paths, the parent must have a worker slot left, and no running
/// task that may work at the same time may own intersecting paths. Never takes over.
pub fn assign(plan: &mut Value, task: &str, assignment: Assignment, now: i64) -> Result {
    let Assignment {
        parent,
        worker,
        limit,
        base,
        work,
    } = assignment;
    if parent == worker {
        return Err("a worker works under its own session".into());
    }
    let found =
        find(plan, task).ok_or_else(|| format!("plan {} has no task {task}", plan["id"]))?;
    if !found["scope"]["paths"].is_array() {
        return Err(format!(
            "task {task} owns no paths; a worker task needs scope.paths (set them with plan-update)"
        ));
    }
    if !found["owner"].is_null() {
        return Err(format!(
            "task {task} is {} in a session already; only a ready task is assigned to a worker",
            found["state"].as_str().unwrap_or_default()
        ));
    }
    let running = workers_of(plan, parent).len() as i64;
    if limit <= 0 {
        return Err(
            "delegation is disabled (efficiency.delegation.maxWorkers is 0); work the task in this session".into(),
        );
    }
    if running >= limit {
        return Err(format!(
            "{running} of {limit} workers already run for this session; integrate or release one first"
        ));
    }
    // The ordinary claim decides readiness, attempts and path ownership.
    let claimed = claim(plan, task, worker, false, base, now)?;
    debug_assert_eq!(claimed, Claim::Started);
    let entry = find_mut(plan, task).expect("claimed");
    entry["owner"]["parent"] = json!(parent);
    if let Some(work) = work {
        entry["work"] = self::work(&work, task)?;
    }
    // The claim counted the revision; the worker fields may need the newer format.
    plan["formatVersion"] = json!(format_version(plan));
    validate(plan)
}

/// The worktree record of `task` for integration by `caller`, the session that assigned
/// it: the task is completed with its branches' heads recorded and not integrated yet.
pub fn integrable<'a>(plan: &'a Value, task: &str, caller: &str) -> Result<&'a Value> {
    let found =
        find(plan, task).ok_or_else(|| format!("plan {} has no task {task}", plan["id"]))?;
    let work = found
        .get("work")
        .ok_or_else(|| format!("task {task} has no worktrees to integrate"))?;
    if work["owner"] != caller {
        return Err(format!(
            "task {task} is integrated by the session that assigned it"
        ));
    }
    if found["integration"]["state"] == "integrated" {
        return Err(format!("task {task} is integrated already"));
    }
    let state = found["state"].as_str().unwrap_or_default();
    if state != "completed" {
        return Err(format!(
            "task {task} is {state}; it is integrated once completed (task-step --complete in its worktree)"
        ));
    }
    Ok(work)
}

fn integration_entry(state: &str, paths: &[String], reason: Option<&str>, now: i64) -> Value {
    let mut entry = json!({"state": state, "at": timestamp::format(now)});
    if !paths.is_empty() {
        entry["paths"] = json!(
            paths
                .iter()
                .take(CONFLICT_PATHS)
                .map(|p| clip(p))
                .collect::<Vec<_>>()
        );
    }
    if let Some(reason) = reason {
        entry["reason"] = json!(clip(reason));
    }
    entry
}

/// Record the merge commits of `task`'s repositories (`(path, commit)`); once every one is
/// merged the task is integrated, which readies its dependents.
pub fn merged(
    plan: &mut Value,
    task: &str,
    commits: &[(String, String)],
    now: i64,
) -> Result<bool> {
    let entry = find_mut(plan, task)
        .filter(|t| t.get("work").is_some())
        .ok_or_else(|| format!("task {task} has no worktrees to integrate"))?;
    let mut all = true;
    for repository in entry["work"]["repositories"]
        .as_array_mut()
        .into_iter()
        .flatten()
    {
        if let Some((_, commit)) = commits
            .iter()
            .find(|(path, _)| repository["path"] == path.as_str())
        {
            repository["merged"] = json!(commit);
        }
        all &= repository.get("merged").is_some();
    }
    if all {
        entry["integration"] = integration_entry("integrated", &[], None, now);
    }
    entry["updated"] = json!(timestamp::format(now));
    touch(plan, now);
    validate(plan)?;
    Ok(all)
}

/// A merge of `task` would conflict on `paths`: the task runs again in the integrating
/// session `caller`, which resolves the conflicts in its worktrees and completes it again.
/// Each round counts as an attempt; once none is left the task is escalated instead and
/// stays completed. Returns whether it was escalated.
pub fn conflicted(
    plan: &mut Value,
    task: &str,
    caller: &str,
    paths: &[String],
    now: i64,
) -> Result<bool> {
    integrable(plan, task, caller)?;
    let at = timestamp::format(now);
    let entry = find_mut(plan, task).expect("integrable");
    let escalated = !attempts_left(entry);
    if escalated {
        entry["integration"] = integration_entry(
            "escalated",
            paths,
            Some("merge conflicts remain and the attempt budget is spent"),
            now,
        );
    } else {
        entry["state"] = json!("running");
        entry["attempts"] = json!(attempts(entry).0 + 1);
        entry["owner"] = json!({"session": caller, "since": at});
        if let Some(entry) = entry.as_object_mut() {
            entry.remove("verified");
        }
        entry["integration"] = integration_entry("conflict", paths, None, now);
    }
    entry["updated"] = json!(at);
    touch(plan, now);
    validate(plan)?;
    Ok(escalated)
}

/// Hand `task`'s integration to the user: kept as it is and shown as needing the user
/// until it is integrated or discarded.
pub fn escalate(plan: &mut Value, task: &str, caller: &str, reason: &str, now: i64) -> Result {
    let found =
        find(plan, task).ok_or_else(|| format!("plan {} has no task {task}", plan["id"]))?;
    if found["work"]["owner"] != caller {
        return Err(format!(
            "task {task} has no worktrees this session integrates"
        ));
    }
    if found["integration"]["state"] == "integrated" {
        return Err(format!("task {task} is integrated already"));
    }
    let paths: Vec<String> = found["integration"]["paths"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(str::to_string)
        .collect();
    let reason = line(Some(&json!(reason)), "escalation reason")?;
    let entry = find_mut(plan, task).expect("found");
    entry["integration"] = integration_entry("escalated", &paths, reason.as_str(), now);
    entry["updated"] = json!(timestamp::format(now));
    touch(plan, now);
    validate(plan)
}

/// Drop `task`'s worktree result: it returns to pending for another attempt (or to be
/// abandoned) and its worktrees are returned for removal.
pub fn discard(plan: &mut Value, task: &str, caller: &str, now: i64) -> Result<Value> {
    let found =
        find(plan, task).ok_or_else(|| format!("plan {} has no task {task}", plan["id"]))?;
    if found["work"]["owner"] != caller {
        return Err(format!(
            "task {task} has no worktrees this session integrates"
        ));
    }
    if found["integration"]["state"] == "integrated" {
        return Err(format!("task {task} is integrated already"));
    }
    let entry = find_mut(plan, task).expect("found");
    entry["state"] = json!("pending");
    release(entry);
    if let Some(entry) = entry.as_object_mut() {
        entry.remove("verified");
    }
    let work = unassign(entry).expect("worktrees");
    entry["updated"] = json!(timestamp::format(now));
    touch(plan, now);
    validate(plan)?;
    Ok(work)
}

/// Refuse task work from a session that does not own `task`.
pub fn owned(plan: &Value, task: &str, owner: &str) -> Result {
    let found =
        find(plan, task).ok_or_else(|| format!("plan {} has no task {task}", plan["id"]))?;
    if found["owner"]["session"] == owner {
        Ok(())
    } else {
        Err(format!(
            "task {task} of plan {} is not held by this session; continue it with task-start --plan {} --task {task} [--take-over]",
            plan["id"], plan["id"]
        ))
    }
}

/// One recorded task step as the plan mirrors it.
pub struct Progress<'a> {
    pub stage: &'a str,
    pub outcome: Option<&'a str>,
    pub complete: bool,
    /// The session's attempt budget is spent, so a failed check fails the task.
    pub exhausted: bool,
    /// What verified a completion: `{revision, contract, at}`.
    pub verified: Option<Value>,
    /// For a task in worktrees, each repository path's branch head the completion verified.
    pub heads: &'a [(String, String)],
    pub result: Option<&'a str>,
    pub next_step: Option<&'a str>,
    pub evidence: Option<&'a str>,
}

/// The task state a recorded step leaves: completion, then blockers and resolutions, then
/// the verify stage. A failed check keeps the task running under its attempt budget and
/// fails it once that budget is spent.
pub fn state_after(progress: &Progress) -> &'static str {
    match progress.outcome {
        _ if progress.complete => "completed",
        Some(outcome) if BLOCKERS.contains(&outcome) => "blocked",
        Some("quality-failure") if progress.exhausted => "failed",
        Some("quality-failure" | "prerequisite-resolved") => "running",
        _ if progress.stage == "verify" => "verifying",
        _ => "running",
    }
}

/// Mirror one step of the owning session onto its task.
pub fn progress(plan: &mut Value, task: &str, owner: &str, step: &Progress, now: i64) -> Result {
    owned(plan, task, owner)?;
    let at = timestamp::format(now);
    let state = state_after(step);
    let entry = find_mut(plan, task).expect("owned");
    entry["state"] = json!(state);
    if !OWNED.contains(&state) {
        release(entry);
    }
    for (key, value) in [("result", step.result), ("nextStep", step.next_step)] {
        if let Some(value) = value {
            entry[key] = json!(value);
        }
    }
    if let Some(verified) = step.verified.as_ref().filter(|_| state == "completed") {
        entry["verified"] = verified.clone();
    }
    if state == "completed" {
        for repository in entry
            .get_mut("work")
            .and_then(|w| w.get_mut("repositories"))
            .and_then(Value::as_array_mut)
            .into_iter()
            .flatten()
        {
            let head = step
                .heads
                .iter()
                .find(|(path, _)| repository["path"] == path.as_str())
                .ok_or_else(|| {
                    format!("task {task} completed without the head of its worktree branch")
                })?;
            repository["head"] = json!(head.1);
        }
    }
    if let Some(evidence) = step.evidence {
        let items = entry["evidence"].as_array_mut().expect("validated");
        if !items.iter().any(|e| e == evidence) {
            items.push(json!(evidence));
            let excess = items.len().saturating_sub(ITEMS);
            items.drain(..excess);
        }
    }
    entry["updated"] = json!(at);
    touch(plan, now);
    validate(plan)
}
