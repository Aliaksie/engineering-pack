//! Test runs the hooks observe: one history per session, each run with the verdict its
//! runner's summary gave and the source digest of the tree it ran on. A run counts for the
//! code only while that digest is the current one.
use crate::failure::Failure;
use crate::session::read_record;
use craft_engine::session::{self, test_runs};
use craft_platform::{repository::snapshot, state};
use serde_json::Value;
use std::path::{Path, PathBuf};

const RUNS_BYTES: usize = 65_536;

fn file(root: &Path, session: &str, create: bool) -> Result<PathBuf, Failure> {
    session::identity(&serde_json::json!(session), &serde_json::json!("test-runs"))?;
    Ok(state::file(root, "test-runs.json", create, Some(session))?)
}

/// The source digest of the tree as it is now (`null` when it cannot be fingerprinted), over
/// the repositories a task names, as acceptance evidence binds to it.
pub fn source(root: &Path, repositories: Option<&Value>) -> Result<Value, Failure> {
    let repository = snapshot::workspace(root, repositories, false)?;
    Ok(session::source(&repository))
}

/// Record one observed run; returns it. A busy history keeps its state.
pub fn record(
    root: &Path,
    session: &str,
    host: &str,
    command: &str,
    verdict: &Value,
    repositories: Option<&Value>,
) -> Result<Value, Failure> {
    let source = source(root, repositories)?;
    let at = craft_engine::timestamp::format(crate::telemetry::now_ms());
    let run = test_runs::observation(verdict, command, host, &at, &source);
    let target = file(root, session, true)?;
    let Some(_lock) = state::lock(&target)? else {
        return Ok(run);
    };
    let next = test_runs::observed(&read_record(&target, RUNS_BYTES)?, run.clone());
    state::write(&target, &next, RUNS_BYTES, false)?;
    Ok(run)
}

/// The latest run in `session` on the tree as it is now, if any.
pub fn current(
    root: &Path,
    session: &str,
    repositories: Option<&Value>,
) -> Result<Option<Value>, Failure> {
    let history = read_record(&file(root, session, false)?, RUNS_BYTES)?;
    if history.is_null() {
        return Ok(None);
    }
    let source = source(root, repositories)?;
    Ok(test_runs::current(&history, &source).cloned())
}

/// Ledger records that ask for the observed run get it; a ledger asking for one needs
/// format 2, and a tree with no observed run refuses the request.
pub fn fill_evidence(
    root: &Path,
    session: &str,
    repositories: Option<&Value>,
    ledger: &mut Value,
) -> Result<(), Failure> {
    let asks = |record: &Value| record["tests"] == "observed";
    let Some(records) = ledger["records"].as_array_mut() else {
        return Ok(());
    };
    if !records.iter().any(asks) {
        return Ok(());
    }
    let run = current(root, session, repositories)?.ok_or(
        "no test run observed on the current tree in this session; run the tests through the host's shell tool first",
    )?;
    for record in records.iter_mut().filter(|record| asks(record)) {
        *record = test_runs::evidence(record, &run)?;
    }
    if ledger["formatVersion"] != 2 {
        return Err(
            "observed test evidence needs an acceptance ledger with formatVersion 2".into(),
        );
    }
    Ok(())
}

/// The test status of the current tree in `session`: `{status, …}` by the latest run on it,
/// or `None` when the session observed no test run at all.
pub fn status(
    root: &Path,
    session: &str,
    repositories: Option<&Value>,
) -> Result<Option<Value>, Failure> {
    let history = read_record(&file(root, session, false)?, RUNS_BYTES)?;
    if history.is_null() {
        return Ok(None);
    }
    Ok(Some(test_runs::status(
        &history,
        &source(root, repositories)?,
    )))
}
