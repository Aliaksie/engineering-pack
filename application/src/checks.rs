//! `craft check`: the project's declared checks for the changed files, run by Craft at the
//! stage asked for. Commands start without a shell, each within its time bound, only from
//! this explicit command and never from a hook. The result binds to the source digest of the
//! tree the commands ran on.
use crate::failure::Failure;
use crate::session::read_record;
use craft_engine::{checks, routing, session::test_runs};
use craft_platform::{process, repository::snapshot, state};
use serde_json::{Value, json};
use std::{path::Path, time::Duration};

const HISTORY_BYTES: usize = 262_144;
const OFF: &str = "routing.projectChecks is false: Craft runs no declared check here";
const NO_CHANGES: &str = "no changed file: nothing is in scope; name files with --file or run every declared check with --all";
const NOTHING: &str = "no check is declared for the changed files";

pub struct Request<'a> {
    pub stage: &'a str,
    /// Project-relative files that select the checks; empty means the uncommitted changes.
    pub files: &'a [Value],
    pub all: bool,
    pub session: Option<&'a str>,
}

/// Run one planned check in its directory under the project root.
fn execute(root: &Path, check: &Value) -> Value {
    let cwd = check["cwd"].as_str().unwrap_or(".");
    let dir = if cwd == "." {
        root.to_path_buf()
    } else {
        root.join(cwd)
    };
    if !dir.is_dir() {
        return json!({"started": false, "durationMs": 0,
            "error": format!("working directory {cwd} does not exist")});
    }
    let args: Vec<String> = check["args"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|arg| arg.as_str().map(str::to_string))
        .collect();
    let ran = process::run_bounded(
        check["command"].as_str().unwrap_or_default(),
        &args,
        &dir,
        Duration::from_secs(
            check["timeoutSeconds"]
                .as_u64()
                .unwrap_or(checks::DEFAULT_TIMEOUT_SECONDS),
        ),
        checks::OUTPUT_BYTES,
    );
    json!({
        "started": ran.started,
        "exitCode": ran.exit,
        "timedOut": ran.timed_out,
        "durationMs": u64::try_from(ran.elapsed.as_millis()).unwrap_or(u64::MAX),
        "output": String::from_utf8_lossy(&ran.output),
        "outputTruncated": ran.truncated,
        "error": ran.error,
    })
}

/// The command line of a check as one text, for the observed test runs.
fn command_line(check: &Value) -> String {
    std::iter::once(check["command"].as_str().unwrap_or_default())
        .chain(
            check["args"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(Value::as_str),
        )
        .collect::<Vec<_>>()
        .join(" ")
}

pub fn run(root: &Path, settings: &Value, request: &Request) -> Result<Value, Failure> {
    let at = craft_engine::timestamp::format(crate::telemetry::now_ms());
    let empty = |note: &str| -> Result<Value, Failure> {
        let plan = json!({"stage": request.stage, "run": [], "left": []});
        let mut report = checks::report(&plan, Vec::new(), &Value::Null, &Value::Null, &at);
        report["note"] = json!(note);
        Ok(report)
    };
    if request.all && !request.files.is_empty() {
        return Err("--all takes no --file".into());
    }
    // An unknown stage is refused before anything else is said.
    checks::plan(
        settings,
        &routing::scope(settings, &[])?,
        request.stage,
        false,
    )?;
    if !checks::enabled(settings) {
        return empty(OFF);
    }
    let changed: Vec<Value> = if request.all || !request.files.is_empty() {
        request.files.to_vec()
    } else {
        snapshot::changes(root, None)
            .uncommitted
            .into_iter()
            .map(|(path, _)| json!(path))
            .collect()
    };
    if changed.is_empty() && !request.all {
        return empty(NO_CHANGES);
    }
    let scope = routing::scope(settings, &changed)?;
    let plan = checks::plan(settings, &scope, request.stage, request.all)?;
    let planned = plan["run"].as_array().map_or(&[][..], Vec::as_slice);
    if planned.is_empty() && plan["left"].as_array().is_none_or(Vec::is_empty) {
        return empty(NOTHING);
    }
    let repositories = match request.session {
        Some(session) => crate::session::active_binding(root, session)?
            .and_then(|binding| binding.get("repositories").cloned()),
        None => None,
    };
    let before = crate::test_runs::source(root, repositories.as_ref())?;
    let mut results = Vec::new();
    for check in planned {
        let outcome = execute(root, check);
        let verdict = checks::verdict(check, &outcome);
        let line = command_line(check);
        // A test command Craft ran itself is an observed run, as one the host's shell ran.
        if let Some(session) = request.session
            && outcome["started"] == true
            && outcome["timedOut"] != true
            && craft_engine::delivery::test_runs(settings)
            && test_runs::is_test_command(&line)
        {
            let observed = test_runs::classify(
                outcome["exitCode"].as_i64(),
                outcome["output"].as_str().unwrap_or_default(),
            );
            crate::test_runs::record(
                root,
                session,
                "craft",
                &line,
                &observed,
                repositories.as_ref(),
            )?;
        }
        results.push(verdict);
    }
    let after = crate::test_runs::source(root, repositories.as_ref())?;
    let mut report = checks::report(&plan, results, &before, &after, &at);
    report["recorded"] = json!(false);
    if let Some(session) = request.session {
        craft_engine::session::identity(&json!(session), &json!("check-runs"))?;
        let target = state::file(root, "check-runs.json", true, Some(session))?;
        if let Some(_lock) = state::lock(&target)? {
            let next = checks::recorded(&read_record(&target, HISTORY_BYTES)?, &report);
            state::write(&target, &next, HISTORY_BYTES, false)?;
            report["recorded"] = json!(true);
        }
    }
    Ok(report)
}
