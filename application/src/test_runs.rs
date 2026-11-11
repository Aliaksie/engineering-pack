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
    record_at(root, session, host, command, verdict, &source)
}

/// Record one observed run against the source digest the caller captured, for a command
/// that may itself have changed the tree it ran on.
pub fn record_at(
    root: &Path,
    session: &str,
    host: &str,
    command: &str,
    verdict: &Value,
    source: &Value,
) -> Result<Value, Failure> {
    let at = craft_engine::timestamp::format(crate::telemetry::now_ms());
    let run = test_runs::observation(verdict, command, host, &at, source);
    let target = file(root, session, true)?;
    let Some(_lock) = state::lock(&target)? else {
        return Ok(run);
    };
    let next = test_runs::observed(&read_record(&target, RUNS_BYTES)?, run.clone());
    state::write(&target, &next, RUNS_BYTES, false)?;
    Ok(run)
}

/// Ledger records that ask for the observed run (`"tests": "observed"`) get it. Both that
/// request and a record carrying observed counts need format 2; observed counts must be ones
/// Craft gave, from this session's runs or the ledger already `saved`; and a tree with no
/// observed run refuses the request.
pub fn fill_evidence(
    root: &Path,
    session: &str,
    repositories: Option<&Value>,
    saved: &Value,
    ledger: &mut Value,
) -> Result<(), Failure> {
    let asks = |record: &Value| record["tests"] == "observed";
    let claims = |record: &Value| record["tests"]["source"] == "observed";
    let Some(records) = ledger["records"].as_array() else {
        return Ok(());
    };
    if !records.iter().any(|record| asks(record) || claims(record)) {
        return Ok(());
    }
    if ledger["formatVersion"] != 2 {
        return Err(
            "observed test evidence needs an acceptance ledger with formatVersion 2".into(),
        );
    }
    let history = read_record(&file(root, session, false)?, RUNS_BYTES)?;
    if let Some(record) = records
        .iter()
        .find(|record| !test_runs::vouched(record, &history, saved))
    {
        return Err(format!(
            "acceptance evidence {} claims observed tests Craft did not observe; ask for them with \"tests\": \"observed\" or declare them with source declared",
            record["id"]
        )
        .into());
    }
    if !records.iter().any(asks) {
        return Ok(());
    }
    let run = test_runs::current(&history, &source(root, repositories)?)
        .cloned()
        .ok_or(
            "no test run observed on the current tree in this session; run the tests through the host's shell tool first",
        )?;
    for record in ledger["records"]
        .as_array_mut()
        .into_iter()
        .flatten()
        .filter(|record| asks(record))
    {
        *record = test_runs::evidence(record, &run)?;
    }
    Ok(())
}

/// The test status of the current tree in `session`: `{status, …}` by the run that decides
/// it, or `None` when the session observed no test run at all.
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
