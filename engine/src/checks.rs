//! The project's declared checks as Craft runs them: which of `modules[].checks` and
//! `standards[].run` a change falls under, which of those the requested stage runs and which
//! it leaves to a later one, and what one finished command counts as. A check that did not
//! run is never reported as passed.
use crate::routing::Scope;
use crate::session::test_runs;
use crate::standards;
use serde_json::{Value, json};

pub mod gates;

type Result<T = ()> = std::result::Result<T, String>;

/// When a check runs, earliest first. A run at one stage includes the earlier ones.
pub const STAGES: [&str; 4] = ["step", "commit", "pr", "ci"];
/// The stage of a check that names none, and of a run that names none.
pub const DEFAULT_STAGE: &str = "commit";
pub const DEFAULT_TIMEOUT_SECONDS: u64 = 600;
pub const MAX_TIMEOUT_SECONDS: u64 = 3600;
/// Bytes of a command's output kept in memory, from its end.
pub const OUTPUT_BYTES: usize = 65_536;
/// Bytes of that output shown for a check that did not pass.
const SHOWN_BYTES: usize = 4_000;
/// Runs kept in a session's history.
const HISTORY: usize = 20;
/// An argument that stands for the changed files the check covers.
pub const CHANGED_FILES: &str = "{changedFiles}";
/// Most bytes the files of one `{changedFiles}` argument may take on a command line.
const ARGUMENT_BYTES: usize = 65_536;

/// Whether `craft check` runs declared commands: unless `routing.projectChecks` is false.
pub fn enabled(settings: &Value) -> bool {
    settings["routing"]["projectChecks"] != false
}

fn rank(stage: &str) -> Option<usize> {
    STAGES.iter().position(|known| *known == stage)
}

/// Validate the optional `id`, `stage` and `timeoutSeconds` of one declared command.
pub fn validate(check: &serde_json::Map<String, Value>, what: &str) -> Result<()> {
    if let Some(stage) = check.get("stage")
        && stage.as_str().and_then(rank).is_none()
    {
        return Err(format!("{what} stage must be step, commit, pr or ci"));
    }
    if let Some(timeout) = check.get("timeoutSeconds")
        && !timeout
            .as_u64()
            .is_some_and(|seconds| (1..=MAX_TIMEOUT_SECONDS).contains(&seconds))
    {
        return Err(format!(
            "{what} timeoutSeconds must be a whole number from 1 to {MAX_TIMEOUT_SECONDS}"
        ));
    }
    if check
        .get("args")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .any(|arg| arg != CHANGED_FILES && arg.contains(CHANGED_FILES))
    {
        return Err(format!(
            "{what} args may hold {CHANGED_FILES} only as a whole argument"
        ));
    }
    if let Some(id) = check.get("id")
        && !id.as_str().is_some_and(|id| {
            !id.is_empty()
                && id.len() <= 64
                && id
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
        })
    {
        return Err(format!(
            "{what} id takes lowercase letters, digits and hyphens"
        ));
    }
    Ok(())
}

fn planned(kind: &str, owner: &Value, id: String, cwd: &Value, command: &Value) -> Value {
    json!({
        "id": id,
        "kind": kind,
        kind: owner,
        "command": command["command"],
        "args": command.get("args").cloned().unwrap_or_else(|| json!([])),
        "cwd": cwd,
        "stage": command["stage"].as_str().unwrap_or(DEFAULT_STAGE),
        "timeoutSeconds": command["timeoutSeconds"].as_u64().unwrap_or(DEFAULT_TIMEOUT_SECONDS),
    })
}

/// The declared checks for `scope` (every declared one with `all`), split by `stage`:
/// `{stage, run, left}`. `run` holds the checks of that stage and earlier, in declaration
/// order, modules first; `left` those of later stages. Two checks never share an id.
pub fn plan(settings: &Value, scope: &Scope, stage: &str, all: bool) -> Result<Value> {
    let at = rank(stage).ok_or("stage must be step, commit, pr or ci")?;
    let modules: Vec<Value> = if all {
        crate::routing::declared_modules(settings)
    } else {
        scope.modules.clone()
    };
    let mut found: Vec<Value> = Vec::new();
    for module in &modules {
        let name = module["name"].as_str().unwrap_or_default();
        let checks = module["checks"].as_array().map_or(&[][..], Vec::as_slice);
        for (index, check) in checks.iter().enumerate() {
            let id = check["id"]
                .as_str()
                .map_or_else(|| format!("{name}-{}", index + 1), str::to_string);
            found.push(planned(
                "module",
                &module["name"],
                id,
                &module["path"],
                check,
            ));
        }
    }
    for standard in standards::commands(settings, scope, all) {
        let run = &standard["run"];
        let id = standard["id"].as_str().unwrap_or_default().to_string();
        let cwd = run.get("cwd").cloned().unwrap_or_else(|| json!("."));
        found.push(planned("standard", &standard["id"], id, &cwd, run));
    }
    let mut seen: Vec<&str> = Vec::new();
    for check in &found {
        let id = check["id"].as_str().unwrap_or_default();
        if seen.contains(&id) {
            return Err(format!(
                "two declared checks share the id {id}; give one an `id`"
            ));
        }
        seen.push(id);
    }
    let (run, left): (Vec<Value>, Vec<Value>) = found
        .into_iter()
        .partition(|check| check["stage"].as_str().and_then(rank) <= Some(at));
    Ok(json!({"stage": stage, "run": run, "left": left}))
}

/// Whether a planned check passes the changed files through a `{changedFiles}` argument.
pub fn takes_files(check: &Value) -> bool {
    check["args"]
        .as_array()
        .is_some_and(|args| args.iter().any(|arg| arg == CHANGED_FILES))
}

/// The files of `changed` a planned check covers, named from its `cwd`: for a module check
/// those the module owns, for a standard's command those in the standard's scope. A file
/// outside the check's directory is not passed to it.
pub fn files_for(settings: &Value, check: &Value, changed: &[Value]) -> Result<Vec<String>> {
    let cwd = check["cwd"].as_str().unwrap_or(".").trim_end_matches('/');
    let mut files = Vec::new();
    for file in changed {
        let scope = crate::routing::scope(settings, std::slice::from_ref(file))?;
        let covered = match check["kind"].as_str() {
            Some("module") => scope
                .modules
                .iter()
                .any(|module| module["name"] == check["module"]),
            _ => standards::commands(settings, &scope, false)
                .iter()
                .any(|standard| standard["id"] == check["standard"]),
        };
        let Some(path) = scope.files.first().filter(|_| covered) else {
            continue;
        };
        let named = if cwd == "." || cwd.is_empty() {
            Some(path.as_str())
        } else {
            path.strip_prefix(cwd)
                .and_then(|rest| rest.strip_prefix('/'))
        };
        files.extend(named.filter(|name| !name.is_empty()).map(str::to_owned));
    }
    Ok(files)
}

/// A planned check's arguments with each `{changedFiles}` replaced by `files`; `Err` when the
/// files would make the command line longer than Craft passes.
pub fn arguments(check: &Value, files: &[String]) -> Result<Vec<String>> {
    let bytes: usize = files.iter().map(|file| file.len() + 1).sum();
    if takes_files(check) && bytes > ARGUMENT_BYTES {
        return Err(format!(
            "{} changed files take {bytes} bytes, more than the {ARGUMENT_BYTES} one {CHANGED_FILES} argument passes; name fewer with --file",
            files.len()
        ));
    }
    let mut arguments = Vec::new();
    for arg in check["args"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
    {
        if arg == CHANGED_FILES {
            arguments.extend(files.iter().cloned());
        } else {
            arguments.push(arg.to_owned());
        }
    }
    Ok(arguments)
}

/// A check that takes the changed files and covers none of them: not run, never passed.
pub fn skipped(check: &Value) -> Value {
    let mut entry = check.clone();
    entry["result"] = json!("skipped");
    entry["reason"] = json!("no-changed-file");
    entry
}

/// The last `limit` bytes of `text`, from a character boundary.
fn tail(text: &str, limit: usize) -> &str {
    let mut start = text.len().saturating_sub(limit);
    while !text.is_char_boundary(start) {
        start += 1;
    }
    &text[start..]
}

/// What one finished command counts as. `outcome` is what the platform observed:
/// `{started, exitCode, timedOut, durationMs, output, error}`. Passed only when the command
/// started, ended in time with exit 0 and, where its output carries a test runner's summary,
/// that summary executed a test.
pub fn verdict(check: &Value, outcome: &Value) -> Value {
    let output = outcome["output"].as_str().unwrap_or_default();
    let exit = outcome["exitCode"].as_i64();
    let mut tests = None;
    let (result, reason) = if outcome["started"] != true {
        ("not-started", "command-unavailable")
    } else if outcome["timedOut"] == true {
        ("timed-out", "time-bound")
    } else if exit != Some(0) {
        ("failed", "exit-code")
    } else {
        let summary = test_runs::classify(exit, output);
        let empty = summary["result"] == "failed";
        if !summary["runner"].is_null() {
            tests = Some(summary.clone());
        }
        if empty {
            (
                "failed",
                match summary["reason"].as_str() {
                    Some("all-skipped") => "all-skipped",
                    _ => "zero-tests",
                },
            )
        } else {
            ("passed", "exit-code")
        }
    };
    let mut entry = check.clone();
    entry["result"] = json!(result);
    entry["reason"] = json!(reason);
    entry["exitCode"] = json!(exit);
    entry["durationMs"] = outcome["durationMs"].clone();
    if let Some(tests) = tests {
        entry["tests"] = json!({"runner": tests["runner"], "executed": tests["executed"],
            "failed": tests["failed"], "skipped": tests["skipped"]});
    }
    if result != "passed" {
        if let Some(error) = outcome["error"].as_str() {
            entry["error"] = json!(error);
        }
        let shown = tail(output.trim_end(), SHOWN_BYTES);
        if !shown.is_empty() {
            entry["output"] = json!(shown);
            entry["outputTruncated"] =
                json!(outcome["outputTruncated"] == true || shown.len() < output.trim_end().len());
        }
    }
    entry
}

/// The report of one run: every executed check with its verdict, every check skipped for want
/// of a changed file, every check left to a later stage, and one overall `result`: `failed` when any executed check did not pass, `passed`
/// when all did, `nothing-run` when none was executed. `before` and `after` are the source
/// digests of the tree around the run; results bind to `before`, and a tree the commands
/// changed says so.
pub fn report(plan: &Value, results: Vec<Value>, before: &Value, after: &Value, at: &str) -> Value {
    let (skipped, results): (Vec<Value>, Vec<Value>) = results
        .into_iter()
        .partition(|entry| entry["result"] == "skipped");
    let left: Vec<Value> = plan["left"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|check| {
            let mut entry = check.clone();
            entry["result"] = json!(format!(
                "left-to-{}",
                check["stage"].as_str().unwrap_or("?")
            ));
            entry
        })
        .collect();
    let passed = results
        .iter()
        .filter(|entry| entry["result"] == "passed")
        .count();
    let result = if results.is_empty() {
        "nothing-run"
    } else if passed == results.len() {
        "passed"
    } else {
        "failed"
    };
    json!({
        "stage": plan["stage"],
        "result": result,
        "at": at,
        "source": before,
        "treeChanged": before != after,
        "summary": {"run": results.len(), "passed": passed, "notPassed": results.len() - passed,
            "skipped": skipped.len(), "left": left.len()},
        "checks": results,
        "skipped": skipped,
        "left": left,
    })
}

/// A session's history with `report` added: the newest [`HISTORY`] runs, each without the
/// commands' output.
pub fn recorded(history: &Value, report: &Value) -> Value {
    let brief = |entry: &Value| {
        json!({"id": entry["id"], "stage": entry["stage"], "result": entry["result"],
            "reason": entry["reason"], "exitCode": entry["exitCode"], "durationMs": entry["durationMs"]})
    };
    let entries = |field: &str| -> Vec<Value> {
        report[field]
            .as_array()
            .into_iter()
            .flatten()
            .map(brief)
            .collect()
    };
    let run = json!({"at": report["at"], "stage": report["stage"], "result": report["result"],
        "source": report["source"], "content": report["content"], "treeChanged": report["treeChanged"],
        "checks": entries("checks"), "skipped": entries("skipped"), "left": entries("left")});
    let mut runs: Vec<Value> = history["runs"].as_array().cloned().unwrap_or_default();
    runs.push(run);
    let skip = runs.len().saturating_sub(HISTORY);
    json!({"formatVersion": 1, "runs": runs.split_off(skip)})
}

/// The newest run of a session's history, counted, as `craft session` shows it; `current`
/// when `source` (the tree now) is the tree it ran on, `null` when the tree cannot be told.
/// `None` before the first run.
pub fn latest(history: &Value, source: &Value) -> Option<Value> {
    let run = history["runs"].as_array()?.last()?;
    let count = |field: &str| run[field].as_array().map_or(0, Vec::len);
    let passed = run["checks"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|check| check["result"] == "passed")
        .count();
    Some(json!({
        "at": run["at"],
        "stage": run["stage"],
        "result": run["result"],
        "run": count("checks"),
        "passed": passed,
        "skipped": count("skipped"),
        "left": count("left"),
        "current": if source.is_null() { Value::Null } else { json!(run["source"] == *source) },
    }))
}

/// One line per check for a terminal, then what was left and the overall result.
pub fn text(report: &Value) -> String {
    let mut lines = Vec::new();
    let line = |entry: &Value| {
        let args: Vec<&str> = entry["args"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .collect();
        format!(
            "{} {} ({}): {} {}",
            entry["result"].as_str().unwrap_or("?"),
            entry["id"].as_str().unwrap_or("?"),
            entry["stage"].as_str().unwrap_or("?"),
            entry["command"].as_str().unwrap_or("?"),
            args.join(" ")
        )
        .trim_end()
        .to_string()
    };
    for entry in report["checks"].as_array().into_iter().flatten() {
        lines.push(line(entry));
        if let Some(output) = entry["output"].as_str() {
            lines.extend(output.lines().map(|text| format!("  {text}")));
        }
    }
    for field in ["skipped", "left"] {
        for entry in report[field].as_array().into_iter().flatten() {
            lines.push(line(entry));
        }
    }
    if let Some(note) = report["note"].as_str() {
        lines.push(note.to_string());
    }
    let summary = &report["summary"];
    lines.push(format!(
        "checks at {}: {} — {} run, {} passed, {} not passed, {}{} left to a later stage{}",
        report["stage"].as_str().unwrap_or("?"),
        report["result"].as_str().unwrap_or("?"),
        summary["run"],
        summary["passed"],
        summary["notPassed"],
        match summary["skipped"].as_u64() {
            Some(skipped) if skipped > 0 => format!("{skipped} skipped with no changed file, "),
            _ => String::new(),
        },
        summary["left"],
        if report["treeChanged"] == true {
            "; the commands changed the tree, so the results describe the tree before them"
        } else {
            ""
        }
    ));
    lines.join("\n") + "\n"
}

/// Whether an agent's `git commit` waits for the declared checks: Craft runs checks and
/// `routing.commitGate` is not false.
pub fn gates_commits(settings: &Value) -> bool {
    enabled(settings) && settings["routing"]["commitGate"] != false
}

/// The declared checks of the commit stage (and the step stage before it) that cover a file
/// of `changed`: one that takes `{changedFiles}` only when a changed file falls under it.
pub fn covering(settings: &Value, changed: &[Value]) -> Result<Vec<Value>> {
    if changed.is_empty() {
        return Ok(Vec::new());
    }
    let scope = crate::routing::scope(settings, changed)?;
    let plan = plan(settings, &scope, DEFAULT_STAGE, false)?;
    let mut found = Vec::new();
    for check in plan["run"].as_array().into_iter().flatten() {
        if !takes_files(check) || !files_for(settings, check, changed)?.is_empty() {
            found.push(check.clone());
        }
    }
    Ok(found)
}

/// The identity of the working tree's content, staged or not: each checkout's `HEAD` and the
/// current bytes of its uncommitted files, `{path, head, files: [[file, digest]], complete}`.
/// Staging alone leaves it unchanged. `null` when a checkout's changes were not all read.
pub fn content(checkouts: &Value) -> Value {
    let Some(entries) = checkouts.as_array() else {
        return Value::Null;
    };
    if entries.is_empty()
        || entries
            .iter()
            .any(|entry| entry["complete"] != true || !entry["head"].is_string())
    {
        return Value::Null;
    }
    let kept: Vec<Value> = entries
        .iter()
        .map(|entry| json!([entry["path"], entry["head"], entry["files"]]))
        .collect();
    json!(crate::digest(&json!(kept)))
}

/// The refusal of a commit while a check of `covering` has not passed on the current files
/// in a run of the session's `history` at the commit stage or later; `None` once the newest
/// such run on these files passed every one of them (a check skipped for want of a file it
/// takes counts as covered). A run is on these files when it bound to `source`, the tree
/// digest with the index, or to `whole`: the [`content`] of a working tree the commit takes
/// in full, so files staged since the run do not hold it. Nothing is run here.
pub fn commit_hold(
    covering: &[Value],
    history: &Value,
    source: &Value,
    whole: &Value,
    session: &str,
) -> Option<Value> {
    if covering.is_empty() || source.is_null() {
        return None;
    }
    let rank = |stage: &str| STAGES.iter().position(|s| *s == stage);
    let commit = rank(DEFAULT_STAGE);
    let ids: Vec<&str> = covering
        .iter()
        .filter_map(|check| check["id"].as_str())
        .collect();
    let on_these_files = history["runs"]
        .as_array()
        .into_iter()
        .flatten()
        .rev()
        .find(|run| {
            run["stage"].as_str().and_then(rank) >= commit
                && (run["source"] == *source || (!whole.is_null() && run["content"] == *whole))
        });
    let failed: Vec<&str> = match on_these_files {
        Some(run) => {
            let entries = |field: &str| -> Vec<&Value> {
                run[field].as_array().into_iter().flatten().collect()
            };
            let (checks, skipped) = (entries("checks"), entries("skipped"));
            let settled = |id: &str| {
                checks
                    .iter()
                    .any(|c| c["id"] == id && c["result"] == "passed")
                    || skipped.iter().any(|c| c["id"] == id)
            };
            let open: Vec<&str> = ids.iter().copied().filter(|id| !settled(id)).collect();
            if open.is_empty() {
                return None;
            }
            // Not passed in that run: failed there, or not part of it.
            open.into_iter()
                .filter(|id| checks.iter().any(|c| c["id"] == *id))
                .collect()
        }
        None => Vec::new(),
    };
    let command = format!("craft check --stage commit --session {session}");
    let reason = if failed.is_empty() {
        format!(
            "Craft holds this commit: the declared commit-stage check{} {} cover{} changed files and {} not passed on the current files. Run `{command}`, fix what it reports, then commit. (routing.commitGate: false turns this off.)",
            if ids.len() == 1 { "" } else { "s" },
            ids.join(", "),
            if ids.len() == 1 { "s" } else { "" },
            if ids.len() == 1 { "has" } else { "have" },
        )
    } else {
        format!(
            "Craft holds this commit: the declared commit-stage check{} {} did not pass on the current files. Fix it, run `{command}` again, then commit. (routing.commitGate: false turns this off.)",
            if failed.len() == 1 { "" } else { "s" },
            failed.join(", "),
        )
    };
    Some(json!({"decision": "deny", "rule": "commit-gate", "reason": reason, "checks": ids}))
}

#[cfg(test)]
mod tests;
