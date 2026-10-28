//! Bounded native observations owned by one host session in one worktree. Only an explicit
//! host integration or the native status line supplies data; no private logs are scraped.
use crate::{failure::Failure, telemetry::now_ms};
use craft_engine::{session, timestamp};
use craft_platform::state;
use serde_json::{Value, json};
use std::path::Path;
const BYTES: usize = 16_384;

pub(crate) fn read(root: &Path, session: &str) -> Result<Value, Failure> {
    session::identity(&json!(session), &json!("host-context"))?;
    let file = state::file(root, "host-context.json", false, Some(session))?;
    let value = state::read(&file, BYTES)?.unwrap_or(Value::Null);
    if !value.is_null() && (value["formatVersion"] != 1 || value["sessionId"] != session) {
        return Err("saved host context has an invalid identity or version".into());
    }
    Ok(value)
}

pub(crate) fn record(
    root: &Path,
    host: &str,
    session: &str,
    payload: &Value,
    at: &str,
) -> Result<Value, Failure> {
    session::identity(&json!(session), &json!("host-context"))?;
    let received = timestamp::parse(at).ok_or("invalid native observation receive timestamp")?;
    let now = now_ms();
    if received > now || now - received > 120_000 {
        return Err("native observation receive timestamp is stale or in the future".into());
    }
    let event = session::host_context::decode(host, payload, session, at)?;
    record_event(root, session, &event)
}

pub(crate) fn record_event(root: &Path, session: &str, event: &Value) -> Result<Value, Failure> {
    session::identity(&json!(session), &json!("host-context"))?;
    let file = state::file(root, "host-context.json", true, Some(session))?;
    let Some(_lock) = state::lock(&file)? else {
        return Err("host context writer is busy".into());
    };
    let previous = read(root, session)?;
    let merged = session::host_context::merge(&previous, event)?;
    state::write(&file, &merged, BYTES, false)?;
    Ok(merged)
}

/// Latest facts stay timestamped; the engine's ordinary freshness/session checks govern
/// whether occupancy is usable. A compaction event removes the old usage immediately.
pub(crate) fn supplement(input: &mut serde_json::Map<String, Value>, saved: &Value) {
    if saved.is_null() {
        return;
    }
    if input
        .get("host")
        .and_then(|h| h.get("name"))
        .is_some_and(|h| *h != saved["host"])
    {
        return;
    }
    let host = input.entry("host").or_insert_with(|| json!({}));
    if let Some(host) = host.as_object_mut() {
        host.entry("name").or_insert_with(|| saved["host"].clone());
        host.entry("reliableCompaction").or_insert_with(|| {
            json!(
                saved["host"]
                    .as_str()
                    .and_then(crate::statusline::capabilities)
                    .is_some_and(|host| host["commands"].get("compact").is_some())
            )
        });
        if let Some(model) = saved.get("model") {
            host.entry("model").or_insert_with(|| model.clone());
        }
    }
    if let Some(usage) = saved.get("usage").filter(|u| !u.is_null()) {
        input.entry("usage").or_insert_with(|| usage.clone());
    }
}
