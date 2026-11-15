//! `craft check`: the project's declared checks for the changed files, run by Craft at the
//! stage asked for. The changed files are read from each checkout the project lies in, named
//! from the project root: its own folder below the top of a repository, or each member
//! repository of a workspace root. Commands start without a shell, each within its time bound, only from
//! this explicit command and never from a hook. The result binds to the source digest of the
//! tree the commands ran on; an agent's commit waits for a passing run on its files.
use crate::failure::Failure;
use crate::session::read_record;
use craft_engine::{checks, routing, session::test_runs};
use craft_platform::{process, state};
use serde_json::{Value, json};
use std::{path::Path, time::Duration};

const HISTORY_BYTES: usize = 262_144;
const OFF: &str = "routing.projectChecks is false: Craft runs no declared check here";
const NO_CHANGES: &str = "no changed file: nothing is in scope; name files with --file or run every declared check with --all";
const NO_CHECKOUT: &str =
    "no Git checkout here: name files with --file or run every declared check with --all";
/// How much one Git listing of changed files may print.
const LISTING_BYTES: usize = 1_048_576;
const NOTHING: &str = "no check is declared for the changed files";

pub struct Request<'a> {
    pub stage: &'a str,
    /// Project-relative files that select the checks; empty means the uncommitted changes.
    pub files: &'a [Value],
    pub all: bool,
    /// A revision whose merge base with `HEAD` adds the files committed since then.
    pub base: Option<&'a str>,
    pub session: Option<&'a str>,
}

/// Run one planned check with `args` in its directory under the project root.
fn execute(root: &Path, check: &Value, args: &[String]) -> Value {
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
    let ran = process::run_bounded(
        check["command"].as_str().unwrap_or_default(),
        args,
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

/// NUL-separated Git output of `args` run in `directory`, refused when Git fails.
fn listing(directory: &Path, args: &[&str], what: &str) -> Result<Vec<String>, Failure> {
    let output = process::git(directory, args, LISTING_BYTES)?
        .ok_or_else(|| format!("Git could not list {what} in {}", directory.display()))?;
    Ok(String::from_utf8_lossy(&output)
        .split('\0')
        .filter(|entry| !entry.is_empty())
        .map(str::to_owned)
        .collect())
}

/// One checkout's changed files named from the project root: uncommitted ones (deleted too,
/// a removal can break a build) and, with `base`, those committed since its merge base with
/// `HEAD`. Private state is never a change.
fn changed_files(
    checkout: &crate::stop_review::Checkout,
    base: Option<&str>,
) -> Result<Vec<String>, Failure> {
    let directory = &checkout.directory;
    let status = [
        "status",
        "--porcelain=v1",
        "-z",
        "--untracked-files=all",
        "--no-renames",
        "--",
        ".",
    ];
    let mut paths: Vec<String> = listing(directory, &status, "the changed files")?
        .iter()
        .filter_map(|entry| entry.get(3..).map(str::to_owned))
        .collect();
    if let Some(base) = base {
        // A ref is resolved by Git itself; `--end-of-options` keeps it from reading as one.
        let merge = process::git(
            directory,
            &["merge-base", "--end-of-options", base, "HEAD"],
            256,
        )?
        .map(|output| String::from_utf8_lossy(&output).trim().to_owned())
        .filter(|id| !id.is_empty())
        .ok_or_else(|| {
            format!(
                "--base {base} names no commit sharing history with HEAD in {}",
                directory.display()
            )
        })?;
        let range = format!("{merge}..HEAD");
        paths.extend(listing(
            directory,
            &[
                "diff",
                "--name-only",
                "--no-renames",
                "-z",
                &range,
                "--",
                ".",
            ],
            "the committed files",
        )?);
    }
    Ok(paths
        .into_iter()
        .filter_map(|path| path.strip_prefix(&*checkout.prefix).map(str::to_owned))
        .filter(|path| !path.is_empty() && !routing::private_path(path))
        .map(|path| format!("{}{path}", checkout.folder))
        .collect())
}

/// The refusal of the session's `git commit` while a declared commit-stage check covers a
/// changed file and has not passed on the current files in a `craft check` run of the
/// session; `None` to let it through. Runs no check: reads the changes, the session's check
/// history and the current tree digest.
pub(crate) fn commit_gate(
    root: &Path,
    settings: &Value,
    session: &str,
) -> Result<Option<Value>, Failure> {
    if !checks::gates_commits(settings) {
        return Ok(None);
    }
    let mut changed = Vec::new();
    for checkout in &crate::stop_review::checkouts(root) {
        changed.extend(changed_files(checkout, None)?);
    }
    changed.sort();
    changed.dedup();
    let changed: Vec<Value> = changed.into_iter().map(Value::from).collect();
    let covering = checks::covering(settings, &changed)?;
    if covering.is_empty() {
        return Ok(None);
    }
    let repositories = crate::session::active_binding(root, session)?
        .and_then(|binding| binding.get("repositories").cloned());
    let source = crate::test_runs::source(root, repositories.as_ref())?;
    Ok(checks::commit_hold(
        &covering,
        &history(root, session)?,
        &source,
        session,
    ))
}

/// The command line of a check as one text, for the observed test runs.
fn command_line(check: &Value, args: &[String]) -> String {
    std::iter::once(check["command"].as_str().unwrap_or_default())
        .chain(args.iter().map(String::as_str))
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
    if request.base.is_some() && (request.all || !request.files.is_empty()) {
        return Err("--base takes no --file or --all".into());
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
        let checkouts = crate::stop_review::checkouts(root);
        if checkouts.is_empty() {
            return empty(NO_CHECKOUT);
        }
        let mut changed = Vec::new();
        for checkout in &checkouts {
            changed.extend(changed_files(checkout, request.base)?);
        }
        changed.sort();
        changed.dedup();
        changed.into_iter().map(Value::from).collect()
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
    // The digest of the tree each command started on: a run is recorded against that tree,
    // so a command that rewrites files leaves a record that no longer matches.
    let mut started_on = before.clone();
    let mut results = Vec::new();
    let mut record_errors = Vec::new();
    for check in planned {
        let files = if checks::takes_files(check) {
            // A deleted file selects its module's checks but is no file to hand a tool.
            let dir = root.join(check["cwd"].as_str().unwrap_or("."));
            let mut files = checks::files_for(settings, check, &changed)?;
            files.retain(|file| std::fs::symlink_metadata(dir.join(file)).is_ok());
            if files.is_empty() {
                results.push(checks::skipped(check));
                continue;
            }
            Some(files)
        } else {
            None
        };
        let expanded = checks::arguments(check, files.as_deref().unwrap_or(&[]));
        let (outcome, args) = match &expanded {
            Ok(args) => (execute(root, check, args), args.clone()),
            Err(error) => (
                json!({"started": false, "durationMs": 0, "error": error}),
                Vec::new(),
            ),
        };
        let mut verdict = checks::verdict(check, &outcome);
        if expanded.is_err() {
            verdict["reason"] = json!("argument-bound");
        }
        if let Some(files) = &files {
            verdict["files"] = json!(files.len());
        }
        let line = command_line(check, &args);
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
            // The commands already ran: a history that cannot be written is reported, and
            // it neither skips the checks still planned nor discards their verdicts.
            if let Err(error) =
                crate::test_runs::record_at(root, session, "craft", &line, &observed, &started_on)
            {
                record_errors.push(json!(format!("{line}: {error}")));
            }
        }
        results.push(verdict);
        started_on = crate::test_runs::source(root, repositories.as_ref())?;
    }
    let after = started_on;
    let mut report = checks::report(&plan, results, &before, &after, &at);
    report["recorded"] = json!(false);
    if !record_errors.is_empty() {
        report["recordErrors"] = json!(record_errors);
    }
    if let Some(session) = request.session {
        match record_history(root, session, &report) {
            Ok(true) => report["recorded"] = json!(true),
            Ok(false) => {}
            Err(error) => report["recordErrors"] = json!([error.to_string()]),
        }
    }
    Ok(report)
}

/// Append the report to the session's check history; `false` when another writer holds it.
fn record_history(root: &Path, session: &str, report: &Value) -> Result<bool, Failure> {
    craft_engine::session::identity(&json!(session), &json!("check-runs"))?;
    let target = state::file(root, "check-runs.json", true, Some(session))?;
    let Some(_lock) = state::lock(&target)? else {
        return Ok(false);
    };
    let next = checks::recorded(&read_record(&target, HISTORY_BYTES)?, report);
    state::write(&target, &next, HISTORY_BYTES, false)?;
    Ok(true)
}

/// The session's check history, `Null` before its first run.
pub(crate) fn history(root: &Path, session: &str) -> Result<Value, Failure> {
    craft_engine::session::identity(&json!(session), &json!("check-runs"))?;
    let target = state::file(root, "check-runs.json", false, Some(session))?;
    read_record(&target, HISTORY_BYTES)
}

/// The session's newest check run, and whether the tree is still the one it ran on; `None`
/// before its first run, so a session that never ran a check fingerprints nothing.
pub(crate) fn last(root: &Path, session: &str) -> Result<Option<Value>, Failure> {
    let history = history(root, session)?;
    if history["runs"].as_array().is_none_or(Vec::is_empty) {
        return Ok(None);
    }
    let repositories = crate::session::active_binding(root, session)?
        .and_then(|binding| binding.get("repositories").cloned());
    let source = crate::test_runs::source(root, repositories.as_ref())?;
    Ok(checks::latest(&history, &source))
}
