//! Work plans of the worktree under `.craft/state/plans/<id>/`: created, changed and shown on
//! request, claimed by `task-start --plan` and advanced by `task-step`. One lock serializes
//! every plan write and the rebuildable index; `plan.md` is regenerated beside each record.
use crate::failure::Failure;
use crate::session::read_record;
use crate::telemetry::now_ms;
use craft_engine::plan::{self, Claim, Observed, Progress};
use craft_engine::values::safe_integer;
use craft_platform::repository::snapshot;
use craft_platform::state::{self, Lock, sweep};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};

const RECORD: &str = "plan.json";
const VIEW: &str = "plan.md";
const INDEX: &str = "index.json";
const INDEX_BYTES: usize = 262_144;
const VIEW_BYTES: usize = 262_144;
/// Plan folders read when the index is rebuilt.
pub const SCANNED: usize = 256;
/// Attempts to take the plans lock, which writers hold for one read-modify-write.
const LOCK_ATTEMPTS: u32 = 20;

/// The plans lock, held for one change of one plan and the index.
pub struct Writer {
    folder: PathBuf,
    _lock: Lock,
}

/// Take the plans lock; writers hold it briefly, so a busy lock is retried for a moment.
pub fn writer(root: &Path) -> Result<Writer, Failure> {
    let folder = state::plans(root, None, true)?;
    for attempt in 1..=LOCK_ATTEMPTS {
        if let Some(lock) = state::lock(&folder.join(INDEX))? {
            return Ok(Writer {
                folder,
                _lock: lock,
            });
        }
        if attempt < LOCK_ATTEMPTS {
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
    }
    Err("plan writer is busy".into())
}

fn record(folder: &Path, id: &str) -> PathBuf {
    folder.join(id).join(RECORD)
}

fn load(file: &Path) -> Result<Option<Value>, Failure> {
    let saved = read_record(file, plan::RECORD_BYTES)?;
    if saved.is_null() {
        return Ok(None);
    }
    plan::validate(&saved)?;
    Ok(Some(saved))
}

/// The saved plan `id`, if there is one.
pub fn read(root: &Path, id: &str) -> Result<Option<Value>, Failure> {
    if !plan::slug(id) {
        return Err("invalid plan id".into());
    }
    load(&record(&state::plans(root, None, false)?, id))
}

/// Every readable plan of the worktree, at most [`SCANNED`].
fn scan(folder: &Path, root: &Path) -> Result<Vec<Value>, Failure> {
    let (names, _) = sweep::plans(root, SCANNED)?;
    Ok(names
        .iter()
        .filter_map(|name| load(&record(folder, name)).ok().flatten())
        .collect())
}

fn valid_index(index: &Value) -> bool {
    index["formatVersion"] == 1 && index["plans"].is_array()
}

/// Summaries of every plan: the index when valid, else rebuilt from the records.
pub fn summaries(root: &Path) -> Result<Vec<Value>, Failure> {
    let folder = state::plans(root, None, false)?;
    let index = read_record(&folder.join(INDEX), INDEX_BYTES).unwrap_or(Value::Null);
    // Trusted only while it names exactly the plan folders present: a writer interrupted
    // between a record and the index leaves it behind.
    let (mut names, _) = sweep::plans(root, SCANNED)?;
    let mut indexed: Vec<&str> = index["plans"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|entry| entry["id"].as_str())
        .collect();
    names.sort();
    indexed.sort_unstable();
    if valid_index(&index) && indexed == names {
        return Ok(index["plans"].as_array().cloned().unwrap_or_default());
    }
    Ok(scan(&folder, root)?.iter().map(plan::summary).collect())
}

impl Writer {
    fn read(&self, id: &str) -> Result<Option<Value>, Failure> {
        load(&record(&self.folder, id))
    }

    /// Write the record, its Markdown view and its index entry.
    fn save(&self, root: &Path, saved: &Value) -> Result<(), Failure> {
        let id = saved["id"].as_str().expect("validated plan id");
        let folder = state::plans(root, Some(id), true)?;
        state::write(&folder.join(RECORD), saved, plan::RECORD_BYTES, true)?;
        state::write_view(&folder.join(VIEW), &plan::markdown(saved), VIEW_BYTES)?;
        let file = self.folder.join(INDEX);
        let index = read_record(&file, INDEX_BYTES).unwrap_or(Value::Null);
        let mut entries: Vec<Value> = if valid_index(&index) {
            index["plans"].as_array().cloned().unwrap_or_default()
        } else {
            scan(&self.folder, root)?
                .iter()
                .map(plan::summary)
                .collect()
        };
        entries.retain(|entry| entry["id"] != id);
        entries.push(plan::summary(saved));
        self.index(entries)
    }

    fn index(&self, mut entries: Vec<Value>) -> Result<(), Failure> {
        entries.sort_by(|a, b| a["id"].as_str().cmp(&b["id"].as_str()));
        let file = self.folder.join(INDEX);
        // An unreadable index is rebuilt, never authority, so it may be replaced.
        if read_record(&file, INDEX_BYTES).is_err() {
            std::fs::remove_file(&file).map_err(|e| e.to_string())?;
        }
        let index = json!({"formatVersion": 1, "plans": entries});
        Ok(state::write(&file, &index, INDEX_BYTES, false)?)
    }
}

/// The directory id this worktree's state keeps `session` under.
pub fn owner(root: &Path, session: &str) -> Result<String, Failure> {
    Ok(state::location(root, Some(session))?.session_id)
}

fn shown(root: &Path, saved: &Value) -> Result<Value, Failure> {
    let id = saved["id"].as_str().unwrap_or_default();
    let mut view = plan::view(saved);
    view["view"] = json!(state::plans(root, Some(id), false)?.join(VIEW));
    Ok(view)
}

/// `plan-create`: a new plan; an existing id is refused.
pub fn create(root: &Path, input: &Value) -> Result<Value, Failure> {
    let created = plan::create(input, now_ms())?;
    let id = created["id"].as_str().expect("validated plan id");
    let writer = writer(root)?;
    if writer.read(id)?.is_some() || state::plans(root, Some(id), false).is_ok_and(|f| f.exists()) {
        return Err(format!("plan {id} already exists; change it with plan-update").into());
    }
    writer.save(root, &created)?;
    shown(root, &created)
}

/// `plan-update`: apply authored changes when the plan is still at `expected` revision.
pub fn update(
    root: &Path,
    id: &str,
    expected: &str,
    changes: &Value,
    session: Option<&str>,
    take_over: bool,
) -> Result<Value, Failure> {
    let expected = expected
        .parse::<i64>()
        .ok()
        .filter(|r| *r >= 1)
        .ok_or("--expected-revision must be the plan revision last read")?;
    let caller = session.map(|s| owner(root, s)).transpose()?;
    let writer = writer(root)?;
    let mut saved = writer.read(id)?.ok_or_else(|| format!("no plan {id}"))?;
    if safe_integer(&saved["revision"]) != Some(expected) {
        return Err(format!(
            "plan {id} changed (revision {} now, {expected} expected); read it again before changing it",
            saved["revision"]
        )
        .into());
    }
    plan::apply(&mut saved, changes, caller.as_deref(), take_over, now_ms())?;
    writer.save(root, &saved)?;
    shown(root, &saved)
}

/// `plan`: one plan, the plans linking a reference, or every plan's summary.
pub fn show(root: &Path, id: Option<&str>, reference: Option<&str>) -> Result<Value, Failure> {
    if let Some(id) = id {
        let saved = read(root, id)?.ok_or_else(|| format!("no plan {id}"))?;
        return shown(root, &saved);
    }
    let mut summaries = summaries(root)?;
    if let Some(reference) = reference {
        let wanted = plan::reference(reference)?;
        summaries.retain(|s| {
            s["references"]
                .as_array()
                .is_some_and(|r| r.contains(&wanted))
        });
    }
    Ok(json!({"plans": summaries}))
}

/// The task `task` of plan `id` as authored: what `task-start` takes from it.
pub fn task(root: &Path, id: &str, task: &str) -> Result<Value, Failure> {
    let saved = read(root, id)?.ok_or_else(|| format!("no plan {id}"))?;
    saved["tasks"]
        .as_array()
        .and_then(|tasks| tasks.iter().find(|t| t["id"] == task))
        .cloned()
        .ok_or_else(|| format!("plan {id} has no task {task}").into())
}

/// The task's source as the scope check reads it, compared with `base` when given.
fn observed(
    root: &Path,
    repositories: Option<&Value>,
    base: Option<&Value>,
) -> Result<Vec<Observed>, Failure> {
    let head = |path: &str| {
        base?["repositories"]
            .as_array()?
            .iter()
            .find(|r| r["path"] == path)?["head"]
            .as_str()
            .map(str::to_string)
    };
    Ok(snapshot::task_changes(root, repositories, &head)?
        .into_iter()
        .map(|(path, changes)| Observed {
            path,
            head: changes.head,
            committed: changes.committed,
            uncommitted: changes.uncommitted,
            complete: changes.complete,
        })
        .collect())
}

/// Claim `task` of plan `id` for `session`; `repositories` is the task's source, which a
/// task owning paths records as it starts. Returns the session directory that held it
/// before when it was taken over.
pub fn claim(
    root: &Path,
    id: &str,
    task: &str,
    session: &str,
    take_over: bool,
    repositories: Option<&Value>,
) -> Result<Option<String>, Failure> {
    let owner = owner(root, session)?;
    // Read the source before the lock; only a task that starts here records it.
    let scoped = read(root, id)?
        .and_then(|p| {
            p["tasks"]
                .as_array()?
                .iter()
                .find(|t| t["id"] == task)
                .cloned()
        })
        .is_some_and(|t| t.get("scope").is_some() && t["owner"].is_null());
    let base = if scoped {
        Some(plan::baseline(&observed(root, repositories, None)?))
    } else {
        None
    };
    let writer = writer(root)?;
    let mut saved = writer.read(id)?.ok_or_else(|| format!("no plan {id}"))?;
    let claimed = plan::claim(&mut saved, task, &owner, take_over, base, now_ms())?;
    if claimed != Claim::Held {
        writer.save(root, &saved)?;
    }
    Ok(match claimed {
        Claim::TakenOver(previous) => Some(previous),
        _ => None,
    })
}

/// Holders of `saved`'s tasks whose session directory no longer exists.
fn missing_holders(root: &Path, saved: &Value) -> Vec<String> {
    saved["tasks"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|t| t["owner"]["session"].as_str())
        .filter(|holder| matches!(sweep::session(root, holder), Ok(None)))
        .map(str::to_string)
        .collect()
}

/// `plan-next`: the coordinator's view of plan `id` for `session`; reads only.
pub fn queue(root: &Path, id: &str, session: Option<&str>) -> Result<Value, Failure> {
    let saved = read(root, id)?.ok_or_else(|| format!("no plan {id}"))?;
    let own = session
        .map(|s| owner(root, s))
        .transpose()?
        .unwrap_or_default();
    let missing = missing_holders(root, &saved);
    let missing: Vec<&str> = missing.iter().map(String::as_str).collect();
    Ok(plan::queue(&saved, &own, &missing, now_ms()))
}

/// The task `task-start --next` claims for `session`: the first ready one in queue order.
pub fn next(root: &Path, id: &str, session: &str) -> Result<String, Failure> {
    let shown = queue(root, id, Some(session))?;
    if let Some(task) = shown["next"].as_str() {
        return Ok(task.to_string());
    }
    let count = |group: &str| shown[group].as_array().map_or(0, Vec::len);
    Err(format!(
        "plan {id} has no ready task ({} held here, {} elsewhere, {} waiting, {} to retry, {} out of attempts); see plan-next --plan {id}",
        count("held"),
        count("elsewhere"),
        count("waiting"),
        count("retry"),
        count("exhausted")
    )
    .into())
}

/// `plan-release`: return `task` to pending and unbind `session` from it. Another
/// session's task is released only with `take_over`.
pub fn release(
    root: &Path,
    id: &str,
    task: &str,
    session: &str,
    take_over: bool,
    reason: Option<&str>,
) -> Result<Value, Failure> {
    let caller = owner(root, session)?;
    // The binding lock first, as task-start takes it before the plans lock.
    let file = crate::session::binding_file(root, session, true)?;
    let Some(_binding) = state::lock(&file)? else {
        return Err("active task writer is busy".into());
    };
    let writer = writer(root)?;
    let mut saved = writer.read(id)?.ok_or_else(|| format!("no plan {id}"))?;
    let holder = plan::release_task(&mut saved, task, &caller, take_over, reason, now_ms())?;
    writer.save(root, &saved)?;
    let bound = crate::session::active_binding(root, session)?
        .is_some_and(|b| b["task"] == task && b["plan"] == id);
    if bound {
        std::fs::remove_file(&file)?;
    }
    Ok(json!({
        "plan": id, "task": task, "state": "pending", "releasedFrom": holder,
        "unbound": bound, "revision": saved["revision"],
    }))
}

/// Refuse a step for a plan task this session does not hold.
pub fn held(root: &Path, id: &str, task: &str, session: &str) -> Result<(), Failure> {
    let saved = read(root, id)?.ok_or_else(|| format!("no plan {id}"))?;
    Ok(plan::owned(&saved, task, &owner(root, session)?)?)
}

/// Refuse completing `task` before its children or with files changed outside the paths
/// it owns; returns the scope report.
pub fn completable(
    root: &Path,
    id: &str,
    task: &str,
    repositories: Option<&Value>,
) -> Result<Value, Failure> {
    let saved = read(root, id)?.ok_or_else(|| format!("no plan {id}"))?;
    let found = saved["tasks"]
        .as_array()
        .and_then(|tasks| tasks.iter().find(|t| t["id"] == task))
        .ok_or_else(|| format!("plan {id} has no task {task}"))?;
    let changed = match found.get("scopeBase") {
        Some(base) if found["scope"]["paths"].is_array() => {
            plan::changed(base, &observed(root, repositories, Some(base))?)
        }
        _ => None,
    };
    Ok(plan::completion(&saved, task, changed.as_deref())?)
}

/// What the parent's holder last checkpointed for it: a reference, never its content.
fn parent_checkpoint(root: &Path, parent: &Value) -> Value {
    let Some(holder) = parent["owner"]["session"].as_str() else {
        return Value::Null;
    };
    let saved = sweep::session(root, holder)
        .ok()
        .flatten()
        .and_then(|d| {
            read_record(
                &d.path.join("checkpoint.json"),
                crate::checkpoint::CHECKPOINT_BYTES,
            )
            .ok()
        })
        .filter(|c| craft_engine::checkpoint::validate(c).is_ok() && c["taskId"] == parent["id"]);
    match saved {
        Some(saved) => json!({
            "session": holder, "head": saved["head"], "updated": saved["updated"],
            "nextStep": saved["nextStep"],
        }),
        None => json!({"session": holder}),
    }
}

/// `plan-envelope`: the small context for one task, or with `brief` the worker brief.
pub fn envelope(root: &Path, id: &str, task: &str, brief: bool) -> Result<Value, Failure> {
    let saved = read(root, id)?.ok_or_else(|| format!("no plan {id}"))?;
    let parent = saved["tasks"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|t| t["id"] == task)
        .and_then(|t| t.get("parent"))
        .and_then(|p| saved["tasks"].as_array()?.iter().find(|t| t["id"] == *p));
    let checkpoint = parent.map_or(Value::Null, |p| parent_checkpoint(root, p));
    let shown = plan::envelope(&saved, task, checkpoint)?;
    if brief {
        return Ok(plan::brief(&shown)?);
    }
    Ok(shown)
}

/// Mirror one recorded step onto the plan task; returns its new state.
pub fn advance(
    root: &Path,
    id: &str,
    task: &str,
    session: &str,
    step: &Progress,
) -> Result<Value, Failure> {
    let owner = owner(root, session)?;
    let writer = writer(root)?;
    let mut saved = writer.read(id)?.ok_or_else(|| format!("no plan {id}"))?;
    plan::progress(&mut saved, task, &owner, step, now_ms())?;
    writer.save(root, &saved)?;
    Ok(
        json!({"plan": id, "task": task, "state": plan::state_after(step), "revision": saved["revision"]}),
    )
}

/// Open plans a starting session is shown, newest first; reads at most [`plan::LISTED`]
/// records beside the index.
pub fn listing(root: &Path, session: &str) -> Result<Vec<Value>, Failure> {
    let mut open: Vec<Value> = summaries(root)?
        .into_iter()
        .filter(|s| s["status"] == "open")
        .collect();
    if open.is_empty() {
        return Ok(open);
    }
    let instant = |s: &Value| {
        s["updated"]
            .as_str()
            .and_then(craft_engine::timestamp::parse)
    };
    open.sort_by_key(|s| std::cmp::Reverse(instant(s)));
    let plans: Vec<Value> = open
        .iter()
        .take(plan::LISTED)
        .filter_map(|s| read(root, s["id"].as_str()?).ok().flatten())
        .collect();
    Ok(plan::listing(plans, &owner(root, session)?, now_ms()))
}

/// Session directories that hold a task of an open plan, so cleanup keeps what a take-over
/// copies; and every plan with whether `state-clean` may remove it.
pub struct Retained {
    pub owners: Vec<String>,
    pub plans: Vec<Value>,
}

const PLAN_RETENTION_MS: i64 = craft_engine::retention::SESSION_RETENTION_DAYS * 86_400_000;

/// Plans as `state-clean` sees them: completed or abandoned plans idle for the retention
/// period are removable; open plans are only reported. With `apply`, removable ones go.
pub fn retained(root: &Path, apply: bool) -> Result<Retained, Failure> {
    let folder = state::plans(root, None, false)?;
    let (names, _) = sweep::plans(root, SCANNED)?;
    let mut retained = Retained {
        owners: Vec::new(),
        plans: Vec::new(),
    };
    if names.is_empty() {
        return Ok(retained);
    }
    let now = now_ms();
    let writer = if apply { Some(writer(root)?) } else { None };
    let mut kept = Vec::new();
    for name in &names {
        let mut item = json!({"plan": name});
        let saved = match load(&record(&folder, name)) {
            Ok(Some(saved)) => saved,
            Ok(None) => {
                item["action"] = json!("keep");
                item["reason"] = json!("holds no plan record");
                retained.plans.push(item);
                continue;
            }
            Err(failure) => {
                item["action"] = json!("keep");
                item["reason"] = json!(failure.message);
                retained.plans.push(item);
                continue;
            }
        };
        let idle = saved["updated"]
            .as_str()
            .and_then(craft_engine::timestamp::parse)
            .map_or(0, |u| (now - u).max(0));
        item["status"] = saved["status"].clone();
        item["idleDays"] = json!(idle / 86_400_000);
        if saved["status"] == "open" {
            for task in saved["tasks"].as_array().into_iter().flatten() {
                if let Some(owner) = task["owner"]["session"].as_str() {
                    retained.owners.push(owner.to_string());
                }
            }
            item["action"] = json!("keep");
            item["reason"] = json!("open");
            kept.push(plan::summary(&saved));
        } else if idle < PLAN_RETENTION_MS {
            item["action"] = json!("keep");
            item["reason"] = json!("changed within the retention period");
            kept.push(plan::summary(&saved));
        } else if writer.is_some() {
            match sweep::remove_plan(&folder.join(name), &[RECORD, VIEW]) {
                Ok(()) => item["action"] = json!("removed"),
                Err(error) => {
                    item["action"] = json!("keep");
                    item["reason"] = json!(error.to_string());
                    kept.push(plan::summary(&saved));
                }
            }
        } else {
            item["action"] = json!("remove");
        }
        retained.plans.push(item);
    }
    if let Some(writer) = &writer {
        writer.index(kept)?;
    }
    Ok(retained)
}
