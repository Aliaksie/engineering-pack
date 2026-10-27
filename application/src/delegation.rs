//! Plan tasks delegated to workers: assigned to a worker session, optionally in isolated
//! Git worktrees below `.craft/worktrees/<plan>/<task>-<attempt>/`, and integrated back into
//! the coordinator checkout. A clean merge is made here without a model; conflicts go back
//! to the integrating session and, when they cannot be resolved, to the user.
use crate::failure::Failure;
use crate::plan::{owner, read, writer};
use crate::telemetry::now_ms;
use craft_engine::plan::{self, Assignment, Observed};
use craft_engine::values::safe_integer;
use craft_platform::repository::{snapshot, worktree};
use craft_platform::state;
use serde_json::{Value, json};
use std::path::Path;

/// Where worker worktrees live, relative to the root; the checkout must ignore it.
const WORKTREES: &str = ".craft/worktrees";
/// Workers one session runs at once unless `efficiency.delegation.maxWorkers` says otherwise.
const WORKERS: i64 = 3;

/// The session a worker works under: its own, derived from the assigning session.
pub fn worker_session(session: &str, id: &str, task: &str) -> String {
    format!("{session}/{id}/{task}")
}

/// The branch a worker's worktree is on for the task's `attempt`; a discarded attempt keeps
/// its branch, so the next one needs its own.
fn branch(id: &str, task: &str, attempt: i64) -> String {
    format!("craft/{id}/{task}-{attempt}")
}

/// The worktree folder for repository `path` (empty for the root's own) in the task's
/// `attempt`, relative to the root.
fn folder(id: &str, task: &str, attempt: i64, path: &str) -> String {
    let base = format!("{WORKTREES}/{id}/{task}-{attempt}");
    if path.is_empty() {
        base
    } else {
        format!("{base}/{path}")
    }
}

fn find<'a>(saved: &'a Value, task: &str) -> Result<&'a Value, Failure> {
    saved["tasks"]
        .as_array()
        .and_then(|tasks| tasks.iter().find(|t| t["id"] == task))
        .ok_or_else(|| format!("plan {} has no task {task}", saved["id"]).into())
}

fn entries(work: &Value) -> impl Iterator<Item = &Value> {
    work["repositories"].as_array().into_iter().flatten()
}

fn text(value: &Value) -> &str {
    value.as_str().unwrap_or_default()
}

/// The repository scopes a session working in `work` binds: its worktree folders.
pub fn scopes(work: &Value) -> Value {
    json!(
        entries(work)
            .map(|r| r["worktree"].clone())
            .collect::<Vec<_>>()
    )
}

/// The task's worktrees as the scope check reads them, each under its repository's path.
pub fn observed(root: &Path, work: &Value, base: Option<&Value>) -> Vec<Observed> {
    entries(work)
        .map(|r| {
            let path = text(&r["path"]).to_string();
            let since = base
                .and_then(|b| {
                    b["repositories"]
                        .as_array()?
                        .iter()
                        .find(|e| e["path"] == path.as_str())
                })
                .and_then(|e| e["head"].as_str());
            let changes = snapshot::changes(&root.join(text(&r["worktree"])), since);
            Observed {
                path,
                head: changes.head,
                committed: changes.committed,
                uncommitted: changes.uncommitted,
                complete: changes.complete,
            }
        })
        .collect()
}

/// Commit what the task's worktrees hold on their branches; returns each repository path's
/// branch head, which its completion verifies.
pub fn commit(root: &Path, work: &Value, message: &str) -> Result<Vec<(String, String)>, Failure> {
    entries(work)
        .map(|r| {
            let head = worktree::commit(&root.join(text(&r["worktree"])), message)?;
            Ok((text(&r["path"]).to_string(), head))
        })
        .collect()
}

/// Remove the worktrees of `work` that hold nothing unmerged or uncommitted, then the
/// folders left empty below `.craft/worktrees`; what is kept is reported.
fn remove(root: &Path, work: &Value) -> Vec<String> {
    let mut kept = Vec::new();
    let top = root.join(WORKTREES);
    for r in entries(work) {
        let place = root.join(text(&r["worktree"]));
        kept.extend(worktree::remove(
            &root.join(text(&r["path"])),
            &place,
            text(&r["branch"]),
        ));
        for folder in place.ancestors().skip(1) {
            if !folder.starts_with(&top) || std::fs::remove_dir(folder).is_err() {
                break;
            }
        }
    }
    kept
}

/// Remove `work` left behind by a task set back for new work.
pub fn discarded(root: &Path, work: &Value) -> Value {
    let kept = remove(root, work);
    json!({"removed": kept.is_empty(), "kept": kept})
}

/// One request to delegate a plan task.
pub struct Assign<'a> {
    pub root: &'a Path,
    pub plan: &'a str,
    pub task: &'a str,
    pub session: &'a str,
    /// Work in the coordinator checkout instead of isolated worktrees.
    pub shared: bool,
    pub settings: &'a Value,
}

/// The repositories a worktree task isolates: those it names, else the root's own when the
/// root is a repository's top folder; `None` when there is nothing to isolate.
fn isolated(root: &Path, task: &Value) -> Result<Option<Vec<String>>, Failure> {
    if let Some(listed) = task["repositories"].as_array() {
        return Ok(Some(listed.iter().map(|r| text(r).to_string()).collect()));
    }
    let Some(top) = worktree::top(root)? else {
        return Ok(None);
    };
    if state::canonical_root(Path::new(&top))? != root {
        return Err("the root is a folder inside a repository; name the task's repositories or assign it with --shared".into());
    }
    Ok(Some(vec![String::new()]))
}

/// Create the worktrees of `task` for the attempt its assignment starts, removing those
/// created when one fails.
fn create(root: &Path, id: &str, task: &Value, paths: &[String]) -> Result<Value, Failure> {
    let task_id = text(&task["id"]);
    let attempt = safe_integer(&task["attempts"]).unwrap_or(0) + 1;
    let owned: Vec<&str> = task["scope"]["paths"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .collect();
    if worktree::top(root)?.is_some() && !worktree::ignored(root, &format!("{WORKTREES}/{id}"))? {
        return Err(format!("Git does not ignore {WORKTREES}/; add it to .gitignore (Craft never edits it) or assign with --shared").into());
    }
    // Uncommitted work in the task's paths would not reach a worktree made from HEAD.
    let mut waiting = Vec::new();
    for path in paths {
        for file in worktree::uncommitted(&root.join(path))? {
            let file = if path.is_empty() {
                file
            } else {
                format!("{path}/{file}")
            };
            if owned.iter().any(|p| plan::covers(p, &file)) {
                waiting.push(file);
            }
        }
    }
    if !waiting.is_empty() {
        waiting.truncate(16);
        return Err(format!(
            "uncommitted changes in the task's paths would not reach its worktree: {}; commit them first or assign with --shared",
            waiting.join(", ")
        )
        .into());
    }
    let name = branch(id, task_id, attempt);
    let mut made: Vec<Value> = Vec::new();
    for path in paths {
        let place = folder(id, task_id, attempt, path);
        let created = worktree::add(&root.join(path), &root.join(&place), &name);
        match created {
            Ok(base) => {
                made.push(json!({"path": path, "worktree": place, "branch": name, "base": base}))
            }
            Err(error) => {
                remove(root, &json!({"repositories": made}));
                return Err(error.into());
            }
        }
    }
    Ok(json!({"repositories": made}))
}

/// `plan-assign`: claim a ready task of plan `id` for a worker of `session`, in isolated
/// worktrees unless `shared` or there is no repository; returns what the worker needs.
pub fn assign(request: Assign) -> Result<Value, Failure> {
    let Assign {
        root,
        plan: id,
        task,
        session,
        shared,
        settings,
    } = request;
    let root = &state::canonical_root(root)?;
    let parent = owner(root, session)?;
    let worker = worker_session(session, id, task);
    let held = owner(root, &worker)?;
    let limit = settings
        .pointer("/efficiency/delegation/maxWorkers")
        .and_then(safe_integer)
        .unwrap_or(WORKERS);
    let saved = read(root, id)?.ok_or_else(|| format!("no plan {id}"))?;
    let found = find(&saved, task)?.clone();
    let assignment = |base, work| Assignment {
        parent: &parent,
        worker: &held,
        limit,
        base,
        work,
    };
    // Refuse before any worktree is made.
    plan::assign(&mut saved.clone(), task, assignment(None, None), now_ms())?;
    let paths = if shared {
        None
    } else {
        isolated(root, &found)?
    };
    let work = match &paths {
        Some(paths) => {
            let mut made = create(root, id, &found, paths)?;
            made["owner"] = json!(parent);
            Some(made)
        }
        None => None,
    };
    let base = match &work {
        Some(work) => plan::baseline(&observed(root, work, None)),
        None => plan::baseline(&crate::plan::source(root, found.get("repositories"))?),
    };
    let assigned = (|| -> Result<Value, Failure> {
        let writer = writer(root)?;
        let mut saved = writer.read(id)?.ok_or_else(|| format!("no plan {id}"))?;
        plan::assign(
            &mut saved,
            task,
            assignment(Some(base), work.clone()),
            now_ms(),
        )?;
        writer.save(root, &saved)?;
        Ok(saved)
    })();
    let saved = match assigned {
        Ok(saved) => saved,
        Err(failure) => {
            if let Some(work) = &work {
                remove(root, work);
            }
            return Err(failure);
        }
    };
    let envelope = crate::plan::envelope(root, id, task, false)?;
    let brief = plan::brief(&envelope)
        .unwrap_or_else(|reason| json!({"status": "unavailable", "reason": reason}));
    let quoted = worker.replace('\'', "'\\''");
    let start = format!(
        "craft task-start --root '{}' --session '{quoted}' --plan {id} --task {task}",
        root.display()
    );
    Ok(json!({
        "plan": id, "task": task, "revision": saved["revision"],
        "worker": {"session": worker, "start": start},
        "work": find(&saved, task)?.get("work").cloned().unwrap_or(Value::Null),
        "isolated": work.is_some(),
        "envelope": envelope, "brief": brief,
        "authorization": "Local plan claim and worktrees only; no worker was started.",
    }))
}

/// How `plan-integrate` treats the task.
pub enum Integration<'a> {
    Merge,
    Escalate(&'a str),
    Discard,
}

/// Refuse a merge the coordinator checkout of repository `path` cannot take now.
fn mergeable(dir: &Path, entry: &Value) -> Result<(), Failure> {
    let name = text(&entry["branch"]);
    if worktree::busy(dir)? {
        return Err(format!(
            "{} is in the middle of a merge or rebase; finish it first",
            dir.display()
        )
        .into());
    }
    if worktree::branch_head(dir, name)?.as_deref() != entry["head"].as_str() {
        return Err(format!("branch {name} moved since the task's completion was verified; discard the task or reset the branch").into());
    }
    let touched = worktree::branch_files(dir, name)?;
    let mut dirty: Vec<String> = worktree::uncommitted(dir)?
        .into_iter()
        .filter(|f| touched.contains(f))
        .collect();
    if !dirty.is_empty() {
        dirty.truncate(16);
        return Err(format!(
            "uncommitted changes in {} touch files the branch changes: {}; commit or stash them first",
            dir.display(),
            dirty.join(", ")
        )
        .into());
    }
    Ok(())
}

/// `plan-integrate`: merge a completed worktree task into the coordinator checkout when it
/// merges cleanly; otherwise hand the conflicts to `session`, or record what `how` says.
pub fn integrate(
    root: &Path,
    id: &str,
    task: &str,
    session: &str,
    how: Integration,
) -> Result<Value, Failure> {
    let root = &state::canonical_root(root)?;
    let caller = owner(root, session)?;
    let change =
        |f: &dyn Fn(&mut Value) -> Result<Value, Failure>| -> Result<(Value, Value), Failure> {
            let writer = writer(root)?;
            let mut saved = writer.read(id)?.ok_or_else(|| format!("no plan {id}"))?;
            let out = f(&mut saved)?;
            writer.save(root, &saved)?;
            Ok((saved, out))
        };
    match how {
        Integration::Escalate(reason) => {
            let (saved, _) = change(&|saved| {
                plan::escalate(saved, task, &caller, reason, now_ms())?;
                Ok(Value::Null)
            })?;
            return Ok(
                json!({"plan": id, "task": task, "integration": find(&saved, task)?["integration"], "revision": saved["revision"]}),
            );
        }
        Integration::Discard => {
            let (saved, work) =
                change(&|saved| Ok(plan::discard(saved, task, &caller, now_ms())?))?;
            return Ok(
                json!({"plan": id, "task": task, "state": "pending", "worktrees": discarded(root, &work), "revision": saved["revision"]}),
            );
        }
        Integration::Merge => {}
    }
    let saved = read(root, id)?.ok_or_else(|| format!("no plan {id}"))?;
    let work = plan::integrable(&saved, task, &caller)?.clone();
    let open: Vec<&Value> = entries(&work)
        .filter(|r| r.get("merged").is_none())
        .collect();
    let mut conflicts = Vec::new();
    for entry in &open {
        let path = text(&entry["path"]);
        let dir = root.join(path);
        mergeable(&dir, entry)?;
        if let Err(paths) = worktree::trial(&dir, text(&entry["branch"]))? {
            conflicts.extend(paths.into_iter().map(|p| {
                if path.is_empty() {
                    p
                } else {
                    format!("{path}/{p}")
                }
            }));
        }
    }
    if !conflicts.is_empty() {
        let (saved, escalated) = change(&|saved| {
            Ok(json!(plan::conflicted(
                saved,
                task,
                &caller,
                &conflicts,
                now_ms()
            )?))
        })?;
        let heads: Vec<Value> = open
            .iter()
            .map(|e| {
                let dir = root.join(text(&e["path"]));
                json!({"path": e["path"], "worktree": e["worktree"], "merge": worktree::head(&dir).ok().flatten()})
            })
            .collect();
        let next = if escalated == true {
            "The attempt budget is spent; the user decides: resolve and complete the task, then plan-integrate again, or plan-integrate --discard.".to_string()
        } else {
            format!(
                "craft task-start --session <id> --plan {id} --task {task}; in each worktree merge the listed commit, resolve the conflicts, run the checks, task-step --complete, then craft plan-integrate again. If they cannot be resolved: craft plan-integrate --escalate --reason \"...\"."
            )
        };
        return Ok(json!({
            "plan": id, "task": task, "integrated": false,
            "integration": find(&saved, task)?["integration"], "state": find(&saved, task)?["state"],
            "conflicts": conflicts, "coordinator": heads, "next": next, "revision": saved["revision"],
        }));
    }
    let goal = text(&find(&saved, task)?["goal"])
        .chars()
        .take(200)
        .collect::<String>();
    let mut commits = Vec::new();
    let mut failure = None;
    for entry in &open {
        let message = format!("Merge {}: {goal}", text(&entry["branch"]));
        match worktree::merge(
            &root.join(text(&entry["path"])),
            text(&entry["branch"]),
            &message,
        ) {
            Ok(commit) => commits.push((text(&entry["path"]).to_string(), commit)),
            Err(error) => {
                failure = Some(error);
                break;
            }
        }
    }
    let (saved, all) = change(&|saved| Ok(json!(plan::merged(saved, task, &commits, now_ms())?)))?;
    if let Some(error) = failure {
        return Err(format!("{error}; merged so far: {}", commits.len()).into());
    }
    let worktrees = if all == true {
        discarded(root, &work)
    } else {
        Value::Null
    };
    Ok(json!({
        "plan": id, "task": task, "integrated": all,
        "merges": commits.iter().map(|(path, commit)| json!({"path": path, "commit": commit})).collect::<Vec<_>>(),
        "worktrees": worktrees, "revision": saved["revision"],
        "authorization": "Merged locally under the approved integration rule; nothing was pushed.",
    }))
}
