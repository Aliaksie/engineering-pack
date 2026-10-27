//! Work plans: one goal of a work item (a feature, an issue, a backlog item) and its ordered
//! tasks with fixed states and dependencies, worked on across sessions. `ready` is derived
//! from dependencies and never stored, so completing a task propagates without a write.
use crate::contracts::relative_file;
use crate::values::{safe_integer, text};
use crate::{repository, timestamp};
use serde_json::{Map, Value, json};

type Result<T = ()> = std::result::Result<T, String>;

pub const FORMAT_VERSION: i64 = 1;
pub const RECORD_BYTES: usize = 131_072;
pub const TASKS: usize = 64;
const DEPENDENCIES: usize = 16;
const DECISIONS: usize = 32;
const REFERENCES: usize = 16;
const ITEMS: usize = 16;
const FILES: usize = 32;
/// Open plans listed when a session starts, newest first.
pub const LISTED: usize = 3;
const LISTED_TASKS: usize = 8;

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
];
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
];
const CHANGE_FIELDS: &[&str] = &["goal", "status", "references", "decisions", "tasks"];
const BLOCKERS: &[&str] = &["permission-denied", "tool-unavailable", "blocked"];

/// A plan, task or decision identity: 1..64 of `[a-z0-9._-]`, not starting with `.`.
pub fn slug(value: &str) -> bool {
    (1..=64).contains(&value.len())
        && !value.starts_with('.')
        && value
            .bytes()
            .all(|b| matches!(b, b'a'..=b'z' | b'0'..=b'9' | b'.' | b'_' | b'-'))
}

fn identity(value: Option<&Value>, what: &str) -> Result<String> {
    value
        .and_then(Value::as_str)
        .filter(|id| slug(id))
        .map(str::to_string)
        .ok_or_else(|| {
            format!("{what} id must be 1..64 of a-z, 0-9, '.', '_' or '-', not starting with '.'")
        })
}

fn line(value: Option<&Value>, what: &str) -> Result<Value> {
    match value {
        Some(value) if text(value) => Ok(value.clone()),
        _ => Err(format!(
            "{what} must be 1..512 characters without control characters"
        )),
    }
}

fn texts(value: Option<&Value>, bound: usize, what: &str) -> Result<Value> {
    match value {
        None | Some(Value::Null) => Ok(json!([])),
        Some(Value::Array(items)) if items.len() <= bound && items.iter().all(text) => {
            Ok(Value::Array(items.clone()))
        }
        _ => Err(format!(
            "{what} must be at most {bound} texts of 1..512 characters"
        )),
    }
}

fn optional_line(value: Option<&Value>, what: &str) -> Result<Value> {
    match value {
        None | Some(Value::Null) => Ok(Value::Null),
        value => line(value, what),
    }
}

fn instant(value: &Value) -> Option<i64> {
    value.as_str().and_then(timestamp::parse)
}

fn references(value: Option<&Value>) -> Result<Value> {
    let items = match value {
        None | Some(Value::Null) => return Ok(json!([])),
        Some(Value::Array(items)) if items.len() <= REFERENCES => items,
        _ => return Err(format!("plan references must be at most {REFERENCES}")),
    };
    let mut seen = Vec::new();
    for item in items {
        let valid = item.as_object().is_some_and(|m| {
            m.len() == 2
                && m.get("kind")
                    .and_then(Value::as_str)
                    .is_some_and(|k| REFERENCE_KINDS.contains(&k))
                && m.get("id").is_some_and(text)
        });
        if !valid {
            return Err("a plan reference is {kind: tracker|document|link, id}".into());
        }
        if seen.contains(&item) {
            return Err("duplicate plan reference".into());
        }
        seen.push(item);
    }
    Ok(Value::Array(items.clone()))
}

/// `kind:id` as a command line names a reference.
pub fn reference(text: &str) -> Result<Value> {
    let (kind, id) = text.split_once(':').unwrap_or_default();
    let reference = json!({"kind": kind, "id": id});
    references(Some(&json!([reference])))?;
    Ok(reference)
}

fn decision(value: &Value) -> Result<Value> {
    let map = value
        .as_object()
        .filter(|m| m.keys().all(|k| ["id", "text", "at"].contains(&k.as_str())))
        .ok_or("a plan decision is {id, text}")?;
    let id = identity(map.get("id"), "decision")?;
    let text = line(map.get("text"), "decision text")?;
    let mut decision = json!({"id": id, "text": text});
    if let Some(at) = map.get("at") {
        instant(at).ok_or("invalid decision time")?;
        decision["at"] = at.clone();
    }
    Ok(decision)
}

fn files(value: Option<&Value>) -> Result<Value> {
    let items = match value {
        None | Some(Value::Null) => return Ok(json!([])),
        Some(Value::Array(items)) if items.len() <= FILES => items,
        _ => {
            return Err(format!(
                "task files must be at most {FILES} project-relative paths"
            ));
        }
    };
    Ok(json!(
        items
            .iter()
            .map(relative_file)
            .collect::<Result<Vec<_>>>()?
    ))
}

/// One task as authored: defaults filled, work fields validated where present.
fn task(value: &Value, now: &str) -> Result<Value> {
    let map = value
        .as_object()
        .filter(|m| m.keys().all(|k| TASK_FIELDS.contains(&k.as_str())))
        .ok_or("invalid plan task")?;
    let id = identity(map.get("id"), "task")?;
    let state = map.get("state").cloned().unwrap_or(json!("pending"));
    if !state.as_str().is_some_and(|s| STATES.contains(&s)) {
        return Err(format!(
            "task {id} state must be one of {}",
            STATES.join(", ")
        ));
    }
    let depends = match map.get("dependsOn") {
        None | Some(Value::Null) => vec![],
        Some(Value::Array(items)) if items.len() <= DEPENDENCIES => items
            .iter()
            .map(|d| identity(Some(d), "dependency"))
            .collect::<Result<Vec<_>>>()?,
        _ => {
            return Err(format!(
                "task {id} has more than {DEPENDENCIES} dependencies"
            ));
        }
    };
    let owner = match map.get("owner") {
        None | Some(Value::Null) => Value::Null,
        Some(owner) => {
            let valid = owner.as_object().is_some_and(|o| {
                o.len() == 2
                    && o.get("session")
                        .and_then(Value::as_str)
                        .is_some_and(|s| crate::values::lower_hex(s, 24))
                    && o.get("since").and_then(instant).is_some()
            });
            if !valid {
                return Err(format!("task {id} owner is invalid"));
            }
            owner.clone()
        }
    };
    let mut task = json!({
        "id": id,
        "goal": line(map.get("goal"), "task goal")?,
        "state": state,
        "dependsOn": depends,
        "files": files(map.get("files"))?,
        "acceptance": texts(map.get("acceptance"), ITEMS, "task acceptance")?,
        "owner": owner,
        "result": optional_line(map.get("result"), "task result")?,
        "nextStep": optional_line(map.get("nextStep"), "task next step")?,
        "evidence": texts(map.get("evidence"), ITEMS, "task evidence")?,
        "updated": map.get("updated").cloned().unwrap_or(json!(now)),
    });
    if let Some(repositories) = map.get("repositories").filter(|r| !r.is_null()) {
        task["repositories"] = json!(repository::scopes(repositories)?);
    }
    instant(&task["updated"]).ok_or("invalid task time")?;
    Ok(task)
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

/// Every dependency names a task of the plan, and none depends on itself through others.
fn dependencies(plan: &Value) -> Result {
    let tasks = tasks(plan);
    let index = |id: &Value| tasks.iter().position(|t| t["id"] == *id);
    for task in tasks {
        for dependency in task["dependsOn"].as_array().into_iter().flatten() {
            if index(dependency).is_none() {
                return Err(format!(
                    "task {} depends on unknown task {dependency}",
                    task["id"]
                ));
            }
        }
    }
    // 0 unvisited, 1 on the current path, 2 done.
    fn visit(at: usize, tasks: &[Value], marks: &mut [u8]) -> Result {
        match marks[at] {
            1 => return Err(format!("dependency cycle through task {}", tasks[at]["id"])),
            2 => return Ok(()),
            _ => {}
        }
        marks[at] = 1;
        for dependency in tasks[at]["dependsOn"].as_array().into_iter().flatten() {
            let next = tasks
                .iter()
                .position(|t| t["id"] == *dependency)
                .expect("known");
            visit(next, tasks, marks)?;
        }
        marks[at] = 2;
        Ok(())
    }
    let mut marks = vec![0; tasks.len()];
    (0..tasks.len()).try_for_each(|at| visit(at, tasks, &mut marks))
}

/// A saved plan as every reader checks it.
pub fn validate(plan: &Value) -> Result {
    let map = plan
        .as_object()
        .filter(|m| m.keys().all(|k| PLAN_FIELDS.contains(&k.as_str())))
        .ok_or("invalid plan")?;
    if map.get("formatVersion").and_then(safe_integer) != Some(FORMAT_VERSION) {
        return Err("unsupported plan version; stop older writers".into());
    }
    identity(map.get("id"), "plan")?;
    line(map.get("goal"), "plan goal")?;
    if !map
        .get("status")
        .and_then(Value::as_str)
        .is_some_and(|s| PLAN_STATUSES.contains(&s))
    {
        return Err("invalid plan status".into());
    }
    references(map.get("references"))?;
    let decisions = map
        .get("decisions")
        .and_then(Value::as_array)
        .ok_or("invalid plan decisions")?;
    if decisions.len() > DECISIONS {
        return Err(format!("a plan holds at most {DECISIONS} decisions"));
    }
    let mut ids = Vec::new();
    for item in decisions {
        if decision(item)? != *item || item.get("at").is_none() {
            return Err("invalid plan decision".into());
        }
        if ids.contains(&&item["id"]) {
            return Err("duplicate decision id".into());
        }
        ids.push(&item["id"]);
    }
    let items = map
        .get("tasks")
        .and_then(Value::as_array)
        .ok_or("invalid plan tasks")?;
    if items.len() > TASKS {
        return Err(format!("a plan holds at most {TASKS} tasks"));
    }
    let mut ids = Vec::new();
    for item in items {
        if task(item, "")? != *item {
            return Err(format!("invalid plan task {}", item["id"]));
        }
        let owned = OWNED.contains(&item["state"].as_str().unwrap_or_default());
        if owned != !item["owner"].is_null() {
            return Err(format!(
                "task {} owner does not match its state",
                item["id"]
            ));
        }
        if ids.contains(&&item["id"]) {
            return Err(format!("duplicate task id {}", item["id"]));
        }
        ids.push(&item["id"]);
    }
    dependencies(plan)?;
    let revision = map.get("revision").and_then(safe_integer);
    let times = ["created", "updated"]
        .iter()
        .all(|k| map.get(*k).and_then(instant).is_some());
    if !revision.is_some_and(|r| r >= 1) || !times {
        return Err("invalid plan revision or time".into());
    }
    if plan.to_string().len() > RECORD_BYTES {
        return Err("plan exceeds its size limit".into());
    }
    Ok(())
}

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
        "formatVersion": FORMAT_VERSION,
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

/// Whether a task waits for nothing: pending, with every dependency completed.
pub fn ready(plan: &Value, task: &Value) -> bool {
    task["state"] == "pending"
        && task["dependsOn"].as_array().into_iter().flatten().all(|d| {
            find(plan, d.as_str().unwrap_or_default()).is_some_and(|t| t["state"] == "completed")
        })
}

/// Whether every task is completed or abandoned, so the plan may be completed.
pub fn completable(plan: &Value) -> bool {
    tasks(plan)
        .iter()
        .all(|t| t["state"] == "completed" || t["state"] == "abandoned")
}

fn touch(plan: &mut Value, now: i64) {
    let revision = plan["revision"].as_i64().unwrap_or(0);
    plan["revision"] = json!(revision + 1);
    plan["updated"] = json!(timestamp::format(now));
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
            .ok_or("an authored task is {id, goal, state?, dependsOn?, files?, repositories?, acceptance?, nextStep?}")?;
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
                    merged["owner"] = Value::Null;
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
                    "a plan is completed only when every task is completed or abandoned".into(),
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
/// session holds is kept; one another session holds moves only with `take_over`.
pub fn claim(
    plan: &mut Value,
    task: &str,
    owner: &str,
    take_over: bool,
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
        None if state == "pending" => {
            let waiting: Vec<&str> = found["dependsOn"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
                .filter(|d| find(plan, d).is_some_and(|t| t["state"] != "completed"))
                .collect();
            return Err(format!(
                "task {task} is not ready: it waits for {}",
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
    let at = timestamp::format(now);
    let entry = find_mut(plan, task).expect("found");
    if claimed == Claim::Started {
        entry["state"] = json!("running");
    }
    entry["owner"] = json!({"session": owner, "since": at});
    entry["updated"] = json!(at);
    touch(plan, now);
    Ok(claimed)
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
    pub result: Option<&'a str>,
    pub next_step: Option<&'a str>,
    pub evidence: Option<&'a str>,
}

/// The task state a recorded step leaves: completion, then blockers and resolutions, then
/// the verify stage. A failed check keeps the task running under its attempt budget.
pub fn state_after(progress: &Progress) -> &'static str {
    match progress.outcome {
        _ if progress.complete => "completed",
        Some(outcome) if BLOCKERS.contains(&outcome) => "blocked",
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
        entry["owner"] = Value::Null;
    }
    for (key, value) in [("result", step.result), ("nextStep", step.next_step)] {
        if let Some(value) = value {
            entry[key] = json!(value);
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

/// The plan as shown: each task with its derived readiness, and task counts by state.
pub fn view(plan: &Value) -> Value {
    let mut shown = plan.clone();
    for item in shown["tasks"].as_array_mut().into_iter().flatten() {
        item["ready"] = json!(ready(plan, item));
    }
    shown["counts"] = counts(plan);
    shown["completable"] = json!(completable(plan));
    shown
}

fn counts(plan: &Value) -> Value {
    let mut counts = Map::new();
    for item in tasks(plan) {
        let state = item["state"].as_str().unwrap_or_default();
        let count = counts.get(state).and_then(Value::as_i64).unwrap_or(0);
        counts.insert(state.into(), json!(count + 1));
    }
    let ready = tasks(plan).iter().filter(|t| ready(plan, t)).count();
    counts.insert("ready".into(), json!(ready));
    Value::Object(counts)
}

/// The plans index entry: what a listing or a reference lookup needs without the plan.
pub fn summary(plan: &Value) -> Value {
    json!({
        "id": plan["id"], "goal": plan["goal"], "status": plan["status"],
        "references": plan["references"], "counts": counts(plan),
        "revision": plan["revision"], "updated": plan["updated"],
    })
}

/// Open plans as a starting session sees them, newest first: ready tasks and held tasks
/// with how long since their last step. `own` is this session's directory id.
pub fn listing(mut plans: Vec<Value>, own: &str, now: i64) -> Vec<Value> {
    plans.retain(|p| p["status"] == "open");
    plans.sort_by(|a, b| {
        instant(&b["updated"])
            .cmp(&instant(&a["updated"]))
            .then_with(|| a["id"].as_str().cmp(&b["id"].as_str()))
    });
    plans
        .iter()
        .take(LISTED)
        .map(|plan| {
            let ready: Vec<&Value> = tasks(plan).iter().filter(|t| ready(plan, t)).map(|t| &t["id"]).take(LISTED_TASKS).collect();
            let held: Vec<Value> = tasks(plan)
                .iter()
                .filter(|t| !t["owner"].is_null())
                .take(LISTED_TASKS)
                .map(|t| {
                    let idle = instant(&t["updated"]).map_or(0, |u| (now - u).max(0) / 60_000);
                    json!({"task": t["id"], "state": t["state"], "here": t["owner"]["session"] == own, "idleMinutes": idle})
                })
                .collect();
            json!({"plan": plan["id"], "goal": plan["goal"], "ready": ready, "held": held})
        })
        .collect()
}

fn cell(value: &Value) -> String {
    match value.as_str() {
        Some(text) if !text.is_empty() => text.replace('|', "\\|"),
        _ => "—".into(),
    }
}

fn joined(value: &Value) -> String {
    let items: Vec<String> = value.as_array().into_iter().flatten().map(cell).collect();
    if items.is_empty() {
        "—".into()
    } else {
        items.join(", ")
    }
}

/// The generated reading view of a plan; never read back.
pub fn markdown(plan: &Value) -> String {
    let mut out = vec![
        format!("# {}: {}", cell(&plan["id"]), cell(&plan["goal"])),
        String::new(),
        "<!-- Generated from plan.json by craft; edits here are not read back. -->".into(),
        String::new(),
        format!(
            "Status: {} · revision {} · updated {}",
            cell(&plan["status"]),
            plan["revision"],
            cell(&plan["updated"])
        ),
    ];
    let references: Vec<String> = plan["references"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|r| format!("{}:{}", cell(&r["kind"]), cell(&r["id"])))
        .collect();
    if !references.is_empty() {
        out.push(format!("References: {}", references.join(", ")));
    }
    out.extend(["".into(), "## Tasks".into(), String::new()]);
    out.push("| Task | State | Depends on | Owner | Next |".into());
    out.push("|---|---|---|---|---|".into());
    for item in tasks(plan) {
        let state = if ready(plan, item) {
            "ready".to_string()
        } else {
            cell(&item["state"])
        };
        let owner = item["owner"]["session"]
            .as_str()
            .map_or("—".into(), |s| s[..8].to_string());
        out.push(format!(
            "| {} | {state} | {} | {owner} | {} |",
            cell(&item["id"]),
            joined(&item["dependsOn"]),
            cell(&item["nextStep"])
        ));
    }
    for item in tasks(plan) {
        out.extend([
            String::new(),
            format!("### {} — {}", cell(&item["id"]), cell(&item["goal"])),
        ]);
        if item["result"].is_string() {
            out.push(format!("Result: {}", cell(&item["result"])));
        }
        for (label, key) in [
            ("Files", "files"),
            ("Repositories", "repositories"),
            ("Evidence", "evidence"),
        ] {
            if item[key].as_array().is_some_and(|a| !a.is_empty()) {
                out.push(format!("{label}: {}", joined(&item[key])));
            }
        }
        if let Some(criteria) = item["acceptance"].as_array().filter(|a| !a.is_empty()) {
            out.push("Acceptance:".into());
            out.extend(criteria.iter().map(|c| format!("- {}", cell(c))));
        }
    }
    if let Some(decisions) = plan["decisions"].as_array().filter(|d| !d.is_empty()) {
        out.extend(["".into(), "## Decisions".into(), String::new()]);
        out.push("| # | Decision | At |".into());
        out.push("|---|---|---|".into());
        for item in decisions {
            out.push(format!(
                "| {} | {} | {} |",
                cell(&item["id"]),
                cell(&item["text"]),
                cell(&item["at"])
            ));
        }
    }
    out.push(String::new());
    out.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: i64 = 1_800_000_000_000;
    const ONE: &str = "aaaaaaaaaaaaaaaaaaaaaaaa";
    const TWO: &str = "bbbbbbbbbbbbbbbbbbbbbbbb";

    fn sample() -> Value {
        create(
            &json!({"id": "b2", "goal": "Managed sessions", "references": [{"kind": "tracker", "id": "PROJ-1"}],
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
            claim(&mut plan, "b", ONE, false, NOW)
                .unwrap_err()
                .contains("waits for a")
        );
        assert_eq!(
            claim(&mut plan, "a", ONE, false, NOW).unwrap(),
            Claim::Started
        );
        assert_eq!(plan["revision"], 2);
        assert_eq!(claim(&mut plan, "a", ONE, false, NOW).unwrap(), Claim::Held);
        assert_eq!(plan["revision"], 2);
        assert!(
            claim(&mut plan, "a", TWO, false, NOW)
                .unwrap_err()
                .contains("--take-over")
        );
        assert_eq!(
            claim(&mut plan, "a", TWO, true, NOW).unwrap(),
            Claim::TakenOver(ONE.into())
        );
        assert_eq!(plan["tasks"][0]["owner"]["session"], TWO);
        assert!(claim(&mut plan, "z", ONE, false, NOW).is_err());
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
        claim(&mut plan, "a", ONE, false, NOW).unwrap();
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
        claim(&mut plan, "a", ONE, false, NOW).unwrap();
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
        assert!(claim(&mut plan, "a", ONE, false, NOW).is_err());
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
        claim(&mut first, "a", ONE, false, NOW - 120_000).unwrap();
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
            json!([{"task": "a", "state": "running", "here": true, "idleMinutes": 2}])
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
        assert!(text.starts_with("# b2: Managed sessions"));
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
        let planned = json!({"formatVersion": 2, "task": "a", "plan": "b2"});
        assert!(binding(&planned, None).unwrap().is_some());
        for invalid in [
            json!({"formatVersion": 1, "task": "a", "plan": "b2"}),
            json!({"formatVersion": 2, "task": "a"}),
            json!({"formatVersion": 2, "task": "a", "plan": "B2"}),
        ] {
            assert!(binding(&invalid, None).is_err(), "{invalid}");
        }
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
}
