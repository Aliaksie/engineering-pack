//! What a plan shows: task envelopes and worker briefs, summaries, the session listing, the
//! coordinator queue and the generated Markdown.
use super::format::instant;
use super::lifecycle::{attempts, completable, ready, workers_of};
use super::scope::{clashing, overlaps};
use super::{
    ENVELOPE_BYTES, ENVELOPE_DECISIONS, ENVELOPE_EVIDENCE, ITEMS, LISTED, LISTED_TASKS, Result,
    STALE_MINUTES, clip, done, find, plain, tasks,
};
use serde_json::{Map, Value, json};

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
