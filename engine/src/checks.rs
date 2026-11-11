//! The project's declared checks as Craft runs them: which of `modules[].checks` and
//! `standards[].run` a change falls under, which of those the requested stage runs and which
//! it leaves to a later one, and what one finished command counts as. A check that did not
//! run is never reported as passed.
use crate::routing::Scope;
use crate::session::test_runs;
use crate::standards;
use serde_json::{Value, json};

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

/// The report of one run: every executed check with its verdict, every check left to a later
/// stage, and one overall `result`: `failed` when any executed check did not pass, `passed`
/// when all did, `nothing-run` when none was executed. `before` and `after` are the source
/// digests of the tree around the run; results bind to `before`, and a tree the commands
/// changed says so.
pub fn report(plan: &Value, results: Vec<Value>, before: &Value, after: &Value, at: &str) -> Value {
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
        "summary": {"run": results.len(), "passed": passed, "notPassed": results.len() - passed, "left": left.len()},
        "checks": results,
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
        "source": report["source"], "treeChanged": report["treeChanged"],
        "checks": entries("checks"), "left": entries("left")});
    let mut runs: Vec<Value> = history["runs"].as_array().cloned().unwrap_or_default();
    runs.push(run);
    let skip = runs.len().saturating_sub(HISTORY);
    json!({"formatVersion": 1, "runs": runs.split_off(skip)})
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
    for entry in report["left"].as_array().into_iter().flatten() {
        lines.push(line(entry));
    }
    if let Some(note) = report["note"].as_str() {
        lines.push(note.to_string());
    }
    let summary = &report["summary"];
    lines.push(format!(
        "checks at {}: {} — {} run, {} passed, {} not passed, {} left to a later stage{}",
        report["stage"].as_str().unwrap_or("?"),
        report["result"].as_str().unwrap_or("?"),
        summary["run"],
        summary["passed"],
        summary["notPassed"],
        summary["left"],
        if report["treeChanged"] == true {
            "; the commands changed the tree, so the results describe the tree before them"
        } else {
            ""
        }
    ));
    lines.join("\n") + "\n"
}

#[cfg(test)]
mod tests;
