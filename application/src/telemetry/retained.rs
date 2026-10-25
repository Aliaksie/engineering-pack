//! Reports over retained telemetry and explicit, bounded captures that keep one session's
//! report after the rolling log rotates. Captures are never evicted automatically.
use craft_engine::contracts::normalize_numbers;
use craft_engine::telemetry::report::{self, CAPTURE_MAX_BYTES, CAPTURE_MAX_ENTRIES, Scope};
use craft_engine::timestamp;
use craft_platform::{state, telemetry as store};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};

const CAPTURES: &str = "telemetry-captures.json";

/// The retained report for the project, or for one session's attributed events.
pub fn report(root: &Path, session: Option<&str>) -> Result<Value, String> {
    if session.is_some_and(|session| !super::valid_session(session)) {
        return Err("invalid telemetry session".into());
    }
    let (records, storage) = store::snapshot(root)?;
    // A report never creates a correlation key just to search retained data.
    let identity = match session {
        Some(session) => Some(store::existing_identity(root, session)?),
        None => None,
    };
    let scope = match &identity {
        Some(identity) => Scope::Session(identity.as_deref()),
        None => Scope::Project,
    };
    Ok(report::summarize(&records, &storage, scope))
}

fn captures(file: &Path) -> Result<Value, String> {
    let mut state = match state::read(file, CAPTURE_MAX_BYTES)? {
        None | Some(Value::Null) => json!({ "formatVersion": 1, "captures": [] }),
        Some(state) => state,
    };
    normalize_numbers(&mut state);
    report::validate_captures(&state)?;
    Ok(state)
}

fn find<'a>(state: &'a Value, id: &str) -> Option<&'a Value> {
    state["captures"]
        .as_array()?
        .iter()
        .find(|entry| entry["id"] == id)
}

fn display(file: &Path) -> String {
    file.display().to_string()
}

/// Retain the current report of `session`; the same report is retained once.
pub fn capture(root: &Path, session: Option<&str>) -> Result<Value, String> {
    let session = session
        .filter(|session| super::valid_session(session))
        .ok_or("telemetry capture requires a bounded --session identity")?;
    let session_id = store::existing_identity(root, session)?
        .ok_or("telemetry correlation key is unavailable")?;
    let (records, storage) = store::snapshot(root)?;
    let mut report = report::summarize(&records, &storage, Scope::Session(Some(&session_id)));
    report.as_object_mut().expect("report").remove("limits");
    if report["retainedEvents"] == 0 {
        return Err("no retained session-attributed observations to capture".into());
    }
    let id = report::capture_id(&session_id, &report);
    let file: PathBuf = state::file(root, CAPTURES, true, None)?;
    let _ticket = state::lock(&file)?.ok_or("telemetry capture writer is busy")?;
    let mut state = captures(&file)?;
    let receipt = |captured_at: &Value, unchanged: bool| {
        json!({ "file": display(&file), "capture": id, "capturedAt": captured_at,
            "unchanged": unchanged, "limits": report::capture_limits() })
    };
    if let Some(existing) = find(&state, &id) {
        return Ok(receipt(&existing["capturedAt"], true));
    }
    let entries = state["captures"]
        .as_array_mut()
        .expect("validated captures");
    if entries.len() >= CAPTURE_MAX_ENTRIES {
        return Err("telemetry capture capacity reached; preserve needed evidence and explicitly forget a capture before adding another".into());
    }
    let captured_at = json!(timestamp::format(super::now_ms()));
    entries.push(
        json!({ "id": id, "capturedAt": captured_at, "sessionId": session_id, "report": report }),
    );
    state::write(&file, &state, CAPTURE_MAX_BYTES, false)?;
    Ok(receipt(&captured_at, false))
}

fn capture_file(root: &Path, id: &str) -> Result<PathBuf, String> {
    if !report::valid_capture_id(id) {
        return Err("invalid telemetry capture id".into());
    }
    state::file(root, CAPTURES, false, None)
}

/// One retained report with the current interpretation limits.
pub fn read(root: &Path, id: &str) -> Result<Value, String> {
    let state = captures(&capture_file(root, id)?)?;
    let entry = find(&state, id).ok_or("telemetry capture not found")?;
    let mut retained = entry["report"].clone();
    retained["limits"] = report::limits();
    Ok(
        json!({ "capture": id, "capturedAt": entry["capturedAt"], "report": retained,
        "limits": report::capture_limits() }),
    )
}

/// Explicitly remove one retained report.
pub fn forget(root: &Path, id: &str) -> Result<Value, String> {
    let file = capture_file(root, id)?;
    find(&captures(&file)?, id).ok_or("telemetry capture not found")?;
    let _ticket = state::lock(&file)?.ok_or("telemetry capture writer is busy")?;
    let state = captures(&file)?;
    let remaining: Vec<_> = state["captures"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|entry| entry["id"] != id)
        .cloned()
        .collect();
    if remaining.len() == state["captures"].as_array().map_or(0, Vec::len) {
        return Err("telemetry capture not found".into());
    }
    let count = remaining.len();
    state::write(
        &file,
        &json!({ "formatVersion": 1, "captures": remaining }),
        CAPTURE_MAX_BYTES,
        false,
    )?;
    Ok(json!({ "forgotten": id, "remaining": count, "limits": report::capture_limits() }))
}
