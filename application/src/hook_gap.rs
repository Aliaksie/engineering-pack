//! Codex sessions in this project that ran without Craft's hooks, from Codex's own session
//! records against this project's hook telemetry. Read-only; only counts and times leave it.
use craft_platform::{host_install, telemetry};
use serde_json::{Value, json};
use std::path::Path;

/// The gap for `host` at `root` as `craft_engine::hook_gap::gap` finds it, seen from the
/// session `current` (left out), with the running bundle installed at `installed`. `None`
/// when the host keeps no session records Craft reads, telemetry is off, or none counts.
pub(crate) fn found(
    root: &Path,
    host: &str,
    current: Option<&str>,
    installed: Option<i64>,
    safety: bool,
) -> Option<Value> {
    if host != "codex" || crate::telemetry::effective(root)["local"] == false {
        return None;
    }
    let (records, _) = telemetry::snapshot(root).ok()?;
    let window = records
        .iter()
        .filter_map(|record| record["timestamp"].as_str())
        .filter_map(craft_engine::timestamp::parse)
        .min()?;
    let sessions = host_install::codex_sessions(root, window.max(installed.unwrap_or(window)));
    let mut ids: Vec<&str> = sessions.iter().map(|(id, _)| id.as_str()).collect();
    ids.extend(current);
    let identities = telemetry::existing_identities(root, &ids).ok()??;
    let current = current.and(identities.last());
    let sessions: Vec<Value> = sessions
        .iter()
        .zip(&identities)
        .map(|((_, started), identity)| json!({"identity": identity, "started": started}))
        .collect();
    craft_engine::hook_gap::gap(&json!({
        "host": host, "current": current, "sessions": sessions, "records": records,
        "installed": installed, "version": crate::telemetry::pack_version(), "safety": safety,
    }))
}
