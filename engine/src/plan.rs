//! Work plans: one goal of a work item (a feature, an issue, a backlog item) and its ordered
//! tasks with fixed states and dependencies, worked on across sessions. `ready` is derived
//! from dependencies and never stored, so completing a task propagates without a write.
use crate::contracts::relative_file;
use crate::values::{safe_integer, sha256, text};
use crate::{repository, timestamp};
use serde_json::{Map, Value, json};

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

/// A project-relative path pattern: segments, `*` within a segment, `**` across them.
fn pattern(value: &Value) -> Result<String> {
    let text = relative_file(value)?;
    let canonical = value.as_str() == Some(text.as_str())
        && text
            .split('/')
            .all(|part| part == "**" || !part.contains("**"));
    if !canonical {
        return Err(format!("scope path must be written canonically: {value}"));
    }
    Ok(text)
}

/// The paths a task owns and the worktree it works in.
fn scope(value: &Value, id: &str) -> Result<Value> {
    let map = value
        .as_object()
        .filter(|m| {
            m.keys()
                .all(|k| ["paths", "worktree"].contains(&k.as_str()))
        })
        .ok_or_else(|| format!("task {id} scope is {{paths, worktree?}}"))?;
    let paths = match map.get("paths") {
        Some(Value::Array(items)) if (1..=SCOPE_PATHS).contains(&items.len()) => {
            items.iter().map(pattern).collect::<Result<Vec<_>>>()?
        }
        _ => {
            return Err(format!(
                "task {id} scope paths must be 1..{SCOPE_PATHS} project-relative patterns"
            ));
        }
    };
    let mut scope = json!({"paths": paths});
    if let Some(worktree) = map.get("worktree").filter(|w| !w.is_null()) {
        scope["worktree"] = json!(relative_file(worktree)?);
    }
    Ok(scope)
}

/// One recorded uncommitted file: `<path digest>:<content digest>`, 16 hex each.
fn base_entry(value: &Value) -> bool {
    value.as_str().is_some_and(|s| {
        s.split_once(':').is_some_and(|(path, content)| {
            crate::values::lower_hex(path, 16)
                && (content == "missing" || crate::values::lower_hex(content, 16))
        })
    })
}

/// The source a claim saw: each repository's revision and its already changed files.
fn scope_base(value: &Value, id: &str) -> Result<Value> {
    let invalid = || format!("task {id} scope base is invalid");
    let map = value
        .as_object()
        .filter(|m| m.len() == 2 && m.get("complete").is_some_and(Value::is_boolean))
        .ok_or_else(invalid)?;
    let repositories = map
        .get("repositories")
        .and_then(Value::as_array)
        .filter(|r| (1..=8).contains(&r.len()))
        .ok_or_else(invalid)?;
    let mut files = 0;
    for entry in repositories {
        let valid = entry.as_object().is_some_and(|e| {
            e.len() == 3
                && e.get("path").is_some_and(Value::is_string)
                && e.get("head").is_some_and(|h| {
                    h.is_null()
                        || h.as_str().is_some_and(|h| {
                            crate::values::lower_hex(h, h.len()) && matches!(h.len(), 40 | 64)
                        })
                })
                && e.get("files")
                    .and_then(Value::as_array)
                    .is_some_and(|f| f.iter().all(base_entry))
        });
        if !valid {
            return Err(invalid());
        }
        files += entry["files"].as_array().map_or(0, Vec::len);
    }
    if files > BASE_FILES {
        return Err(invalid());
    }
    Ok(value.clone())
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
            let session = |v: Option<&Value>| {
                v.and_then(Value::as_str)
                    .is_some_and(|s| crate::values::lower_hex(s, 24))
            };
            let valid = owner.as_object().is_some_and(|o| {
                o.keys()
                    .all(|k| ["session", "since", "parent"].contains(&k.as_str()))
                    && session(o.get("session"))
                    && o.get("since").and_then(instant).is_some()
                    && o.get("parent")
                        .is_none_or(|p| session(Some(p)) && Some(p) != o.get("session"))
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
    let id = task["id"].as_str().unwrap_or_default().to_string();
    let given = |key: &str| map.get(key).filter(|v| !v.is_null());
    if let Some(parent) = given("parent") {
        let parent = identity(Some(parent), "parent task")?;
        if parent == id {
            return Err(format!("task {id} cannot be its own parent"));
        }
        task["parent"] = json!(parent);
    }
    if let Some(owned) = given("scope") {
        task["scope"] = scope(owned, &id)?;
    }
    if let Some(contract) = given("contract") {
        task["contract"] = json!(relative_file(contract)?);
    }
    if let Some(returns) = given("returns") {
        task["returns"] = line(Some(returns), "task result shape")?;
    }
    if let Some(base) = given("scopeBase") {
        task["scopeBase"] = scope_base(base, &id)?;
    }
    for (key, most) in [
        ("attempts", TASKS as i64 * MAX_ATTEMPTS),
        ("maxAttempts", MAX_ATTEMPTS),
    ] {
        if let Some(count) = given(key) {
            if !safe_integer(count).is_some_and(|c| (1..=most).contains(&c)) {
                return Err(format!("task {id} {key} must be 1..{most}"));
            }
            task[key] = count.clone();
        }
    }
    if let Some(verified) = given("verified") {
        task["verified"] = self::verified(verified, &id)?;
    }
    if let Some(work) = given("work") {
        task["work"] = self::work(work, &id)?;
    }
    if let Some(integration) = given("integration") {
        task["integration"] = self::integration(integration, &id)?;
    }
    instant(&task["updated"]).ok_or("invalid task time")?;
    Ok(task)
}

fn revision(value: Option<&Value>) -> bool {
    value
        .and_then(Value::as_str)
        .is_some_and(|h| crate::values::lower_hex(h, h.len()) && matches!(h.len(), 40 | 64))
}

/// The isolated worktrees a worker works in: per repository of the task (`path`, empty for
/// the root's own repository) its worktree below the root, branch and base revision, the
/// branch head its completion was verified at and, once merged, the merge commit.
fn work(value: &Value, id: &str) -> Result<Value> {
    let invalid = || format!("task {id} worktree record is invalid");
    let map = value
        .as_object()
        .filter(|m| m.len() == 2)
        .ok_or_else(invalid)?;
    let owner = map.get("owner").and_then(Value::as_str);
    if !owner.is_some_and(|o| crate::values::lower_hex(o, 24)) {
        return Err(invalid());
    }
    let repositories = map
        .get("repositories")
        .and_then(Value::as_array)
        .filter(|r| (1..=WORK_REPOSITORIES).contains(&r.len()))
        .ok_or_else(invalid)?;
    for entry in repositories {
        let valid = entry.as_object().is_some_and(|e| {
            e.keys().all(|k| {
                ["path", "worktree", "branch", "base", "head", "merged"].contains(&k.as_str())
            }) && e.get("path").and_then(Value::as_str).is_some_and(|p| {
                p.is_empty() || repository::scopes(&json!([p])).is_ok_and(|s| s == [p])
            }) && e.get("worktree").is_some_and(|w| relative_file(w).is_ok())
                && e.get("branch").is_some_and(text)
                && revision(e.get("base"))
                && ["head", "merged"]
                    .iter()
                    .all(|k| e.get(*k).is_none_or(|v| revision(Some(v))))
        });
        if !valid {
            return Err(invalid());
        }
    }
    Ok(value.clone())
}

/// How a worktree task's branches reached the coordinator: `conflict` while its holder
/// resolves conflicts, `integrated` once merged, `escalated` when it needs the user.
fn integration(value: &Value, id: &str) -> Result<Value> {
    let valid = value.as_object().is_some_and(|v| {
        v.keys()
            .all(|k| ["state", "at", "paths", "reason"].contains(&k.as_str()))
            && v.get("state")
                .and_then(Value::as_str)
                .is_some_and(|s| INTEGRATION_STATES.contains(&s))
            && v.get("at").and_then(instant).is_some()
            && v.get("paths").is_none_or(|p| {
                p.as_array()
                    .is_some_and(|p| p.len() <= CONFLICT_PATHS && p.iter().all(text))
            })
            && v.get("reason").is_none_or(text)
    });
    if !valid {
        return Err(format!("task {id} integration record is invalid"));
    }
    Ok(value.clone())
}

/// What verified a completion: the source revision, the engineering contract digest and when.
fn verified(value: &Value, id: &str) -> Result<Value> {
    let valid = value.as_object().is_some_and(|v| {
        v.len() == 3
            && v.get("revision").is_some_and(|r| r.is_null() || text(r))
            && v.get("contract").is_some_and(|c| c.is_null() || sha256(c))
            && v.get("at").and_then(instant).is_some()
    });
    if !valid {
        return Err(format!("task {id} verification record is invalid"));
    }
    Ok(value.clone())
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

/// Every dependency and parent names a task of the plan, parents are not children
/// themselves, and no task waits for itself through others.
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
        if let Some(parent) = task.get("parent") {
            let found = index(parent)
                .map(|at| &tasks[at])
                .ok_or_else(|| format!("task {} has unknown parent task {parent}", task["id"]))?;
            if found.get("parent").is_some() {
                return Err(format!(
                    "task {} is a child of {parent}, which is a child itself; plans nest one level",
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
        for dependency in waits_for(tasks, &tasks[at]) {
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
    let version = map.get("formatVersion").and_then(safe_integer);
    if !version.is_some_and(|v| (FIRST_VERSION..=FORMAT_VERSION).contains(&v)) {
        return Err("unsupported plan version; stop older writers".into());
    }
    if version != Some(format_version(plan)) {
        return Err("plan version does not match the fields it uses".into());
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
        if item.get("scopeBase").is_some() && item["owner"].is_null() {
            return Err(format!(
                "task {} records a scope base but no owner",
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

/// Whether a completed task's work has reached the coordinator: always, unless it worked
/// in isolated worktrees that are not integrated yet.
fn done(task: &Value) -> bool {
    task["state"] == "completed"
        && (task.get("work").is_none() || task["integration"]["state"] == "integrated")
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

/// The version a plan is written at: the first one holding every field its tasks use, so
/// older readers keep reading plans that do not need a newer format.
fn format_version(plan: &Value) -> i64 {
    let uses = |fields: &[&str]| {
        tasks(plan)
            .iter()
            .any(|t| fields.iter().any(|f| t.get(*f).is_some()))
    };
    let worker = tasks(plan)
        .iter()
        .any(|t| t["owner"].get("parent").is_some());
    if worker || uses(PARALLEL_FIELDS) {
        FORMAT_VERSION
    } else if uses(COORDINATED_FIELDS) {
        COORDINATED_VERSION
    } else if uses(SCOPED_FIELDS) {
        SCOPED_VERSION
    } else {
        FIRST_VERSION
    }
}

/// Claims `task` has started and the most it allows.
fn attempts(task: &Value) -> (i64, i64) {
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

/// Whether neither task can work at the same time as the other: one waits for the other or
/// is its parent.
fn related(plan: &Value, a: &Value, b: &Value) -> bool {
    a.get("parent") == Some(&b["id"])
        || b.get("parent") == Some(&a["id"])
        || reaches(plan, a, b, &mut vec![])
        || reaches(plan, b, a, &mut vec![])
}

/// Owned paths of `a` and `b` that may intersect, compared by their literal leading
/// segments.
fn shared(a: &Value, b: &Value) -> Vec<String> {
    let paths = |t: &Value| -> Vec<String> {
        t["scope"]["paths"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .map(str::to_string)
            .collect()
    };
    paths(a)
        .iter()
        .flat_map(|p| paths(b).into_iter().map(move |q| (p.clone(), q)))
        .filter(|(p, q)| {
            let (p, q) = (prefix(p), prefix(q));
            let n = p.len().min(q.len());
            p[..n] == q[..n]
        })
        .map(|(p, q)| if p == q { p } else { format!("{p} ~ {q}") })
        .collect()
}

/// Refuse starting `task` while a running task that may work at the same time owns
/// intersecting paths, in this checkout or another worktree.
fn unowned(plan: &Value, task: &Value) -> Result {
    let clashes: Vec<String> = clashing(plan, task)
        .into_iter()
        .map(|(t, paths)| format!("{} ({})", plain(&t["id"]), paths.join(", ")))
        .collect();
    if clashes.is_empty() {
        return Ok(());
    }
    Err(format!(
        "task {} owns paths that running tasks also own: {}; wait for them or narrow the scopes with plan-update",
        plain(&task["id"]),
        clashes.join("; ")
    ))
}

/// The running tasks unrelated to `task` that own paths intersecting its own, with those paths.
fn clashing<'a>(plan: &'a Value, task: &Value) -> Vec<(&'a Value, Vec<String>)> {
    tasks(plan)
        .iter()
        .filter(|t| t["id"] != task["id"])
        .filter(|t| OWNED.contains(&t["state"].as_str().unwrap_or_default()))
        .filter(|t| !related(plan, task, t))
        .filter_map(|t| {
            let paths = shared(task, t);
            (!paths.is_empty()).then_some((t, paths))
        })
        .collect()
}

/// Tasks held for workers that `parent` assigned and that still run.
fn workers_of<'a>(plan: &'a Value, parent: &str) -> Vec<&'a Value> {
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

/// What one repository of a task's source shows: its revision, the files committed since
/// a base revision (`None` when that cannot be told) and the uncommitted files with their
/// content digest. `path` is the repository scope, empty for the worktree itself.
pub struct Observed {
    pub path: String,
    pub head: Value,
    pub committed: Option<Vec<String>>,
    pub uncommitted: Vec<(String, String)>,
    pub complete: bool,
}

fn base_file(path: &str, digest: &str) -> String {
    let content = if digest == "missing" {
        digest
    } else {
        &digest[..16.min(digest.len())]
    };
    format!("{}:{content}", &crate::hash(path.as_bytes())[..16])
}

/// The scope base a claim records: each repository's revision and its already changed
/// files, so completion counts only what changed afterwards. Incomplete when the source
/// could not be fully read or too many files were already changed.
pub fn baseline(observed: &[Observed]) -> Value {
    let total: usize = observed.iter().map(|o| o.uncommitted.len()).sum();
    let complete = observed.iter().all(|o| o.complete) && total <= BASE_FILES;
    let repositories: Vec<Value> = observed
        .iter()
        .map(|o| {
            let files: Vec<String> = if complete {
                o.uncommitted.iter().map(|(p, d)| base_file(p, d)).collect()
            } else {
                vec![]
            };
            json!({"path": o.path, "head": o.head, "files": files})
        })
        .collect();
    json!({"repositories": repositories, "complete": complete})
}

/// Files changed since `base`, as project-relative paths; `None` when that cannot be told.
pub fn changed(base: &Value, observed: &[Observed]) -> Option<Vec<String>> {
    if base["complete"] != true {
        return None;
    }
    let mut changed = Vec::new();
    for current in observed {
        let saved = base["repositories"]
            .as_array()?
            .iter()
            .find(|r| r["path"] == current.path.as_str())?;
        let before: Vec<&str> = saved["files"]
            .as_array()?
            .iter()
            .filter_map(Value::as_str)
            .collect();
        if !current.complete {
            return None;
        }
        let committed = current.committed.as_ref()?;
        let uncommitted = current
            .uncommitted
            .iter()
            .filter(|(p, d)| !before.contains(&base_file(p, d).as_str()))
            .map(|(p, _)| p);
        for file in committed.iter().chain(uncommitted) {
            changed.push(if current.path.is_empty() {
                file.clone()
            } else {
                format!("{}/{file}", current.path)
            });
        }
    }
    changed.sort();
    changed.dedup();
    Some(changed)
}

fn segment(pattern: &[u8], name: &[u8]) -> bool {
    match pattern.split_first() {
        None => name.is_empty(),
        Some((b'*', rest)) => (0..=name.len()).any(|at| segment(rest, &name[at..])),
        Some((byte, rest)) => name.first() == Some(byte) && segment(rest, &name[1..]),
    }
}

fn walk(pattern: &[&str], path: &[&str]) -> bool {
    match pattern.split_first() {
        // A pattern that ends early names a directory: everything below it is covered.
        None => true,
        Some((&"**", rest)) => (0..=path.len()).any(|at| walk(rest, &path[at..])),
        Some((part, rest)) => path.split_first().is_some_and(|(name, below)| {
            segment(part.as_bytes(), name.as_bytes()) && walk(rest, below)
        }),
    }
}

/// Whether the scope pattern covers the project-relative `path`: the file itself or, for a
/// pattern naming a directory, anything below it.
pub fn covers(pattern: &str, path: &str) -> bool {
    let pattern: Vec<&str> = pattern.split('/').collect();
    let path: Vec<&str> = path.split('/').collect();
    walk(&pattern, &path)
}

fn ids(items: &[&Value]) -> String {
    items
        .iter()
        .filter_map(|t| t["id"].as_str())
        .collect::<Vec<_>>()
        .join(", ")
}

/// What completing `task` needs beyond its acceptance: its children done and no file
/// changed since its claim outside the paths it or its children own. `changed` is `None`
/// when the changed files cannot be told; the scope is then reported unchecked. Returns
/// the scope report.
pub fn completion(plan: &Value, task: &str, changed: Option<&[String]>) -> Result<Value> {
    let found =
        find(plan, task).ok_or_else(|| format!("plan {} has no task {task}", plan["id"]))?;
    let open: Vec<&Value> = tasks(plan)
        .iter()
        .filter(|t| t.get("parent") == Some(&found["id"]))
        .filter(|t| t["state"] != "completed" && t["state"] != "abandoned")
        .collect();
    if !open.is_empty() {
        return Err(format!(
            "task {task} has child tasks that are not completed or abandoned: {}",
            ids(&open)
        ));
    }
    let pending: Vec<&Value> = tasks(plan)
        .iter()
        .filter(|t| t.get("parent") == Some(&found["id"]))
        .filter(|t| t["state"] == "completed" && !done(t))
        .collect();
    if !pending.is_empty() {
        return Err(format!(
            "task {task} has child tasks whose worktrees are not integrated: {}; run plan-integrate for them",
            ids(&pending)
        ));
    }
    if !found["scope"]["paths"].is_array() {
        return Ok(Value::Null);
    }
    // A parent's work includes what its children change within their own paths.
    let paths: Vec<&str> = tasks(plan)
        .iter()
        .filter(|t| t["id"] == found["id"] || t.get("parent") == Some(&found["id"]))
        .flat_map(|t| t["scope"]["paths"].as_array().into_iter().flatten())
        .filter_map(Value::as_str)
        .collect();
    let Some(changed) = changed else {
        let reason = if found.get("scopeBase").is_none() {
            "the claim recorded no source to compare with"
        } else {
            "the changed files cannot be told completely"
        };
        return Ok(json!({"checked": false, "reason": reason}));
    };
    let outside: Vec<&String> = changed
        .iter()
        .filter(|file| !paths.iter().any(|p| covers(p, file)))
        .collect();
    if !outside.is_empty() {
        let shown: Vec<&str> = outside.iter().take(16).map(|f| f.as_str()).collect();
        return Err(format!(
            "scope exceeded: task {task} changed files outside its paths: {}{}; widen its scope with plan-update or undo those changes",
            shown.join(", "),
            if outside.len() > shown.len() {
                ", …"
            } else {
                ""
            }
        ));
    }
    Ok(json!({"checked": true, "changed": changed.len()}))
}

/// The literal leading segments of a pattern, before any wildcard.
fn prefix(pattern: &str) -> Vec<&str> {
    pattern
        .split('/')
        .take_while(|p| !p.contains('*'))
        .collect()
}

fn reaches(plan: &Value, from: &Value, to: &Value, seen: &mut Vec<Value>) -> bool {
    waits_for(tasks(plan), from).any(|next| {
        if *next == to["id"] {
            return true;
        }
        if seen.contains(next) {
            return false;
        }
        seen.push(next.clone());
        find(plan, next.as_str().unwrap_or_default()).is_some_and(|t| reaches(plan, t, to, seen))
    })
}

/// Pairs of unfinished tasks that may work at the same time (neither waits for the other
/// and neither is the other's parent) with owned paths that may intersect. Patterns are
/// compared by their literal leading segments, so an overlap may be reported where the
/// wildcards would keep them apart.
pub fn overlaps(plan: &Value) -> Vec<Value> {
    let open: Vec<&Value> = tasks(plan)
        .iter()
        .filter(|t| t["state"] != "completed" && t["state"] != "abandoned")
        .filter(|t| t["scope"]["paths"].is_array())
        .collect();
    let mut found = Vec::new();
    for (at, a) in open.iter().enumerate() {
        for b in &open[at + 1..] {
            if related(plan, a, b) {
                continue;
            }
            let shared = shared(a, b);
            if !shared.is_empty() {
                found.push(json!({"tasks": [a["id"], b["id"]], "paths": shared}));
            }
        }
    }
    found
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

fn summary_of(task: &Value, keys: &[&str]) -> Value {
    let mut out = Map::new();
    for key in keys {
        if let Some(value) = task.get(*key).filter(|v| !v.is_null()) {
            out.insert(key.to_string(), value.clone());
        }
    }
    Value::Object(out)
}

/// The small context a session or worker needs for one task: the task, what its
/// dependencies returned, its parent with the parent's last checkpoint (`checkpoint`, a
/// reference without content, or null), its children, the latest decisions and the plan's
/// references. Never conversation, other tasks' work or secrets. Bounded; the oldest
/// decisions and dependency evidence go first when it would not fit.
pub fn envelope(plan: &Value, task: &str, checkpoint: Value) -> Result<Value> {
    let found =
        find(plan, task).ok_or_else(|| format!("plan {} has no task {task}", plan["id"]))?;
    let mut own = summary_of(
        found,
        &[
            "id",
            "goal",
            "state",
            "acceptance",
            "files",
            "repositories",
            "scope",
            "contract",
            "returns",
            "nextStep",
            "work",
            "integration",
        ],
    );
    own["ready"] = json!(ready(plan, found));
    let dependencies: Vec<Value> = found["dependsOn"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|d| find(plan, d.as_str()?))
        .map(|d| {
            let mut entry = summary_of(d, &["id", "goal", "state", "result"]);
            let evidence = d["evidence"].as_array().map_or(&[][..], Vec::as_slice);
            entry["evidence"] = json!(evidence[evidence.len().saturating_sub(ENVELOPE_EVIDENCE)..]);
            entry
        })
        .collect();
    let parent = match found.get("parent").and_then(|p| find(plan, p.as_str()?)) {
        Some(parent) => {
            let mut entry = summary_of(parent, &["id", "goal", "state", "nextStep"]);
            entry["checkpoint"] = checkpoint;
            entry
        }
        None => Value::Null,
    };
    let children: Vec<Value> = tasks(plan)
        .iter()
        .filter(|t| t.get("parent") == Some(&found["id"]))
        .map(|t| summary_of(t, &["id", "goal", "state", "result"]))
        .collect();
    let decisions = plan["decisions"].as_array().map_or(&[][..], Vec::as_slice);
    let mut shown = json!({
        "formatVersion": 1,
        "plan": {"id": plan["id"], "goal": plan["goal"], "revision": plan["revision"]},
        "task": own,
        "dependencies": dependencies,
        "parent": parent,
        "children": children,
        "decisions": decisions[decisions.len().saturating_sub(ENVELOPE_DECISIONS)..],
        "references": plan["references"],
    });
    let size = |v: &Value| v.to_string().len();
    while size(&shown) > ENVELOPE_BYTES
        && shown["decisions"].as_array().is_some_and(|d| !d.is_empty())
    {
        shown["decisions"].as_array_mut().expect("list").remove(0);
    }
    if size(&shown) > ENVELOPE_BYTES {
        for entry in shown["dependencies"].as_array_mut().into_iter().flatten() {
            entry["evidence"] = json!([]);
        }
    }
    if size(&shown) > ENVELOPE_BYTES {
        return Err(format!(
            "the envelope of task {task} exceeds {ENVELOPE_BYTES} bytes"
        ));
    }
    Ok(shown)
}

fn plain(value: &Value) -> &str {
    value.as_str().unwrap_or_default()
}

/// An envelope as the worker `brief`: sources are the plan references and what the
/// dependencies returned, restrictions the owned paths and worktree.
pub fn brief(envelope: &Value) -> Result<Value> {
    let task = &envelope["task"];
    let acceptance = task["acceptance"].as_array().cloned().unwrap_or_default();
    if acceptance.is_empty() {
        return Err(format!(
            "task {} has no acceptance criteria; a worker brief needs them",
            task["id"]
        ));
    }
    let mut sources: Vec<Value> = envelope["references"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|r| json!(clip(&format!("{}:{}", plain(&r["kind"]), plain(&r["id"])))))
        .collect();
    for dependency in envelope["dependencies"].as_array().into_iter().flatten() {
        if let Some(result) = dependency["result"].as_str() {
            sources.push(json!(clip(&format!(
                "{} returned: {result}",
                plain(&dependency["id"])
            ))));
        }
    }
    let mut restrictions: Vec<Value> = task["scope"]["paths"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(|p| json!(clip(&format!("Change only files matching {p}"))))
        .collect();
    if let Some(worktree) = task["scope"]["worktree"].as_str() {
        restrictions.push(json!(clip(&format!("Work in the worktree {worktree}"))));
    }
    for repository in task["work"]["repositories"]
        .as_array()
        .into_iter()
        .flatten()
    {
        restrictions.push(json!(clip(&format!(
            "Work only in the worktree {} (branch {}); leave other checkouts unchanged",
            plain(&repository["worktree"]),
            plain(&repository["branch"])
        ))));
    }
    if sources.len() > ITEMS || restrictions.len() > ITEMS {
        return Err(format!(
            "task {} has more than {ITEMS} sources or owned paths for a worker brief",
            task["id"]
        ));
    }
    let returns = task["returns"]
        .as_str()
        .unwrap_or("What changed and how it was verified");
    Ok(json!({
        "goal": task["goal"],
        "sources": sources,
        "acceptance": acceptance,
        "restrictions": restrictions,
        "returnShape": returns,
    }))
}

/// The plan as shown: each task with its derived readiness, and task counts by state.
pub fn view(plan: &Value) -> Value {
    let mut shown = plan.clone();
    for item in shown["tasks"].as_array_mut().into_iter().flatten() {
        item["ready"] = json!(ready(plan, item));
    }
    shown["counts"] = counts(plan);
    shown["completable"] = json!(completable(plan));
    let overlaps = overlaps(plan);
    if !overlaps.is_empty() {
        shown["overlaps"] = json!(overlaps);
    }
    shown
}

/// Tasks in reading order: each parent followed by its children.
fn ordered(plan: &Value) -> Vec<&Value> {
    let mut out = Vec::new();
    for item in tasks(plan).iter().filter(|t| t.get("parent").is_none()) {
        out.push(item);
        out.extend(
            tasks(plan)
                .iter()
                .filter(|t| t.get("parent") == Some(&item["id"])),
        );
    }
    out
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
                    let idle = idle_minutes(t, now);
                    json!({"task": t["id"], "state": t["state"], "here": t["owner"]["session"] == own, "idleMinutes": idle, "stale": idle >= STALE_MINUTES})
                })
                .collect();
            let mut entry = json!({"plan": plan["id"], "goal": plan["goal"], "ready": ready, "held": held});
            let escalated: Vec<&Value> = tasks(plan)
                .iter()
                .filter(|t| t["integration"]["state"] == "escalated")
                .map(|t| &t["id"])
                .take(LISTED_TASKS)
                .collect();
            if !escalated.is_empty() {
                entry["needsUser"] = json!(escalated);
            }
            entry
        })
        .collect()
}

/// Minutes since the claim of a held task or its last step, whichever is later.
fn idle_minutes(task: &Value, now: i64) -> i64 {
    let last = [&task["owner"]["since"], &task["updated"]]
        .into_iter()
        .filter_map(instant)
        .max()
        .unwrap_or(now);
    (now - last).max(0) / 60_000
}

/// The coordinator's view of a plan for the session directory `own`, each group in reading
/// order: ready tasks (children of a parent `own` holds first), tasks held here, tasks held
/// elsewhere (`stale` after [`STALE_MINUTES`] without a step, `orphaned` when `missing`
/// names their holder's directory as gone), tasks waiting with what they wait on (`stuck`
/// behind a failed or abandoned one), failed tasks with attempts left, tasks out of
/// attempts, and parents whose children are all done. Nothing is claimed or released.
pub fn queue(plan: &Value, own: &str, missing: &[&str], now: i64) -> Value {
    let order = ordered(plan);
    let here = |t: &Value| t["owner"]["session"] == own;
    let mut ready: Vec<&Value> = order
        .iter()
        .copied()
        .filter(|t| self::ready(plan, t) && clashing(plan, t).is_empty())
        .collect();
    // Stable: the children of a parent held here keep their order ahead of the rest.
    ready.sort_by_key(|t| {
        !t.get("parent")
            .and_then(|p| find(plan, p.as_str()?))
            .is_some_and(here)
    });
    let mut out = json!({
        "plan": plan["id"], "revision": plan["revision"], "status": plan["status"],
        "next": ready.first().map_or(Value::Null, |t| t["id"].clone()),
        "ready": ready.iter().map(|t| &t["id"]).collect::<Vec<_>>(),
        "held": [], "elsewhere": [], "waiting": [], "retry": [], "exhausted": [], "parents": [],
        "integrate": [], "needsUser": [], "workers": workers_of(plan, own).len(),
    });
    let push = |out: &mut Value, group: &str, item: Value| {
        out[group].as_array_mut().expect("group").push(item);
    };
    for task in &order {
        let id = &task["id"];
        let state = task["state"].as_str().unwrap_or_default();
        let (made, most) = attempts(task);
        let done = |t: &Value| self::done(t) || t["state"] == "abandoned";
        let children: Vec<&Value> = tasks(plan)
            .iter()
            .filter(|t| t.get("parent") == Some(id))
            .collect();
        if !children.is_empty() && children.iter().all(|c| done(c)) && !done(task) {
            push(&mut out, "parents", json!({"task": id, "state": state}));
        }
        if task.get("work").is_some() {
            let integration = &task["integration"];
            if integration["state"] == "escalated" {
                push(
                    &mut out,
                    "needsUser",
                    json!({"task": id, "paths": integration["paths"], "reason": integration["reason"]}),
                );
            } else if state == "completed" && integration.is_null() {
                push(&mut out, "integrate", json!({"task": id}));
            }
        }
        if let Some(holder) = task["owner"]["session"].as_str() {
            if holder == own {
                push(&mut out, "held", json!({"task": id, "state": state}));
            } else {
                let idle = idle_minutes(task, now);
                let mut entry = json!({
                    "task": id, "state": state, "owner": holder, "idleMinutes": idle,
                    "stale": idle >= STALE_MINUTES, "orphaned": missing.contains(&holder),
                });
                if task["owner"]["parent"] == own {
                    entry["worker"] = json!(true);
                }
                push(&mut out, "elsewhere", entry);
            }
            continue;
        }
        let counted = json!({"task": id, "state": state, "attempts": made, "maxAttempts": most});
        match state {
            "pending" | "failed" if made >= most => push(&mut out, "exhausted", counted),
            "failed" => push(&mut out, "retry", counted),
            "pending" if !self::ready(plan, task) => {
                let on: Vec<Value> = task["dependsOn"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(|d| find(plan, d.as_str()?))
                    .filter(|d| !self::done(d))
                    .map(|d| json!({"id": d["id"], "state": d["state"]}))
                    .collect();
                let stuck = on
                    .iter()
                    .any(|d| d["state"] == "failed" || d["state"] == "abandoned");
                push(
                    &mut out,
                    "waiting",
                    json!({"task": id, "on": on, "stuck": stuck}),
                );
            }
            // Ready, but a running task owns some of its paths.
            "pending" if !clashing(plan, task).is_empty() => {
                let on: Vec<Value> = clashing(plan, task)
                    .into_iter()
                    .map(|(t, paths)| json!({"id": t["id"], "state": t["state"], "paths": paths}))
                    .collect();
                push(
                    &mut out,
                    "waiting",
                    json!({"task": id, "on": on, "stuck": false}),
                );
            }
            _ => {}
        }
    }
    out
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
    for item in ordered(plan) {
        let state = if ready(plan, item) {
            "ready".to_string()
        } else {
            cell(&item["state"])
        };
        let owner = item["owner"]["session"]
            .as_str()
            .map_or("—".into(), |s| s[..8].to_string());
        let indent = if item.get("parent").is_some() {
            "↳ "
        } else {
            ""
        };
        out.push(format!(
            "| {indent}{} | {state} | {} | {owner} | {} |",
            cell(&item["id"]),
            joined(&item["dependsOn"]),
            cell(&item["nextStep"])
        ));
    }
    for item in ordered(plan) {
        out.extend([
            String::new(),
            format!("### {} — {}", cell(&item["id"]), cell(&item["goal"])),
        ]);
        for (label, key) in [
            ("Part of", "parent"),
            ("Returns", "returns"),
            ("Result", "result"),
            ("Contract", "contract"),
        ] {
            if item[key].is_string() {
                out.push(format!("{label}: {}", cell(&item[key])));
            }
        }
        if item.get("attempts").is_some() {
            let (made, most) = attempts(item);
            out.push(format!("Attempts: {made} of {most}"));
        }
        if let Some(verified) = item.get("verified") {
            out.push(format!(
                "Verified: {} at {}",
                cell(&verified["revision"]),
                cell(&verified["at"])
            ));
        }
        if item["scope"]["paths"].is_array() {
            out.push(format!("Owns: {}", joined(&item["scope"]["paths"])));
        }
        if item["scope"]["worktree"].is_string() {
            out.push(format!("Worktree: {}", cell(&item["scope"]["worktree"])));
        }
        for repository in item["work"]["repositories"]
            .as_array()
            .into_iter()
            .flatten()
        {
            out.push(format!(
                "Worktree: {} on {}",
                cell(&repository["worktree"]),
                cell(&repository["branch"])
            ));
        }
        if item.get("work").is_some() {
            let integration = &item["integration"];
            let state = integration["state"].as_str().unwrap_or("pending");
            let paths = if integration["paths"].is_array() {
                format!(" on {}", joined(&integration["paths"]))
            } else {
                String::new()
            };
            out.push(format!("Integration: {state}{paths}"));
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
    let overlaps = overlaps(plan);
    if !overlaps.is_empty() {
        out.extend(["".into(), "## Overlapping scopes".into(), String::new()]);
        for item in &overlaps {
            out.push(format!(
                "- {} may work at the same time on {}",
                joined(&item["tasks"]).replace(", ", " and "),
                joined(&item["paths"])
            ));
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

    fn nested() -> Value {
        create(
            &json!({"id": "s5", "goal": "Child contracts", "tasks": [
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
            &json!({"id": "s7", "goal": "Parallel", "tasks": [
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
        json!({"owner": ONE, "repositories": [{"path": "", "worktree": format!(".craft/worktrees/s7/{task}"),
            "branch": format!("craft/s7/{task}"), "base": BASE}]})
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
        assert_eq!(work["repositories"][0]["branch"], "craft/s7/a");
        let a = find(&plan, "a").unwrap();
        assert!(
            a["state"] == "pending" && a.get("work").is_none() && a.get("integration").is_none()
        );
        validate(&plan).unwrap();
    }

    #[test]
    fn an_integrator_escalates_by_hand_and_parents_wait_for_integration() {
        let mut plan = create(
            &json!({"id": "s7", "goal": "Parallel", "tasks": [
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
