//! What a task's source shows: the claim's baseline, files changed since it, the paths its
//! scope covers and tasks whose owned paths may overlap.
use super::{BASE_FILES, OWNED, Result, done, find, plain, tasks, waits_for};
use serde_json::{Value, json};

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
pub(super) fn unowned(plan: &Value, task: &Value) -> Result {
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
pub(super) fn clashing<'a>(plan: &'a Value, task: &Value) -> Vec<(&'a Value, Vec<String>)> {
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

pub(super) fn ids(items: &[&Value]) -> String {
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
