//! The persisted plan format: fields, versions 1 to 4, defaults and validation.
use super::{
    BASE_FILES, CONFLICT_PATHS, COORDINATED_FIELDS, COORDINATED_VERSION, DECISIONS, DEPENDENCIES,
    FILES, FIRST_VERSION, FORMAT_VERSION, INTEGRATION_STATES, ITEMS, MAX_ATTEMPTS, OWNED,
    PARALLEL_FIELDS, PLAN_FIELDS, PLAN_STATUSES, RECORD_BYTES, REFERENCE_KINDS, REFERENCES, Result,
    SCOPE_PATHS, SCOPED_FIELDS, SCOPED_VERSION, STATES, TASK_FIELDS, TASKS, WORK_REPOSITORIES,
    tasks, waits_for,
};
use crate::contracts::relative_file;
use crate::values::{safe_integer, sha256, text};
use crate::{repository, timestamp};
use serde_json::{Value, json};

/// A plan, task or decision identity: 1..64 of `[a-z0-9._-]`, not starting with `.`.
pub fn slug(value: &str) -> bool {
    (1..=64).contains(&value.len())
        && !value.starts_with('.')
        && value
            .bytes()
            .all(|b| matches!(b, b'a'..=b'z' | b'0'..=b'9' | b'.' | b'_' | b'-'))
}

pub(super) fn identity(value: Option<&Value>, what: &str) -> Result<String> {
    value
        .and_then(Value::as_str)
        .filter(|id| slug(id))
        .map(str::to_string)
        .ok_or_else(|| {
            format!("{what} id must be 1..64 of a-z, 0-9, '.', '_' or '-', not starting with '.'")
        })
}

pub(super) fn line(value: Option<&Value>, what: &str) -> Result<Value> {
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

pub(super) fn instant(value: &Value) -> Option<i64> {
    value.as_str().and_then(timestamp::parse)
}

pub(super) fn references(value: Option<&Value>) -> Result<Value> {
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

pub(super) fn decision(value: &Value) -> Result<Value> {
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
pub(super) fn task(value: &Value, now: &str) -> Result<Value> {
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
pub(super) fn work(value: &Value, id: &str) -> Result<Value> {
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

/// The version a plan is written at: the first one holding every field its tasks use, so
/// older readers keep reading plans that do not need a newer format.
pub(super) fn format_version(plan: &Value) -> i64 {
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
