//! Context delivery at host lifecycle boundaries: the work a lifecycle fact asks for, the
//! context a hook delivers and when an identical routing hint stays quiet. Lifecycle facts
//! come from the application; host event names and wire formats never enter this policy.
use crate::routing::catalog::SOURCE;
use crate::values::lower_hex;
use serde_json::{Value, json};

/// Host-neutral facts about one hook event.
#[derive(Default)]
pub struct Facts<'a> {
    pub session: bool,
    /// The host can show this event's context to the model; routing hints need it.
    pub context_supported: bool,
    /// An edit at the one tool hook that delivers its context: before the tool runs when the
    /// host shows context there, otherwise after it.
    pub edit: bool,
    /// The intent the session's requests established, from its intent record.
    pub established: Option<&'a str>,
    /// The edited file was already routed under the established `implement` intent.
    pub file_routed: bool,
    /// The submitted prompt, for prompt events only.
    pub prompt: Option<&'a str>,
    pub started: bool,
    pub context_changing: bool,
    pub context_restored: bool,
    pub turn_finished: bool,
}

#[derive(Debug, PartialEq)]
pub struct Plan {
    pub intent: Option<&'static str>,
    /// What a prompt asks to produce when it decides the workflow: `spec-audit`, `adr`.
    pub artifact: Option<&'static str>,
    /// The intent this event establishes for the session: a prompt's request, or the
    /// `implement` an edit routes when no request established one.
    pub establish: Option<&'static str>,
    pub invalidate_hint: bool,
    pub services: bool,
    pub save_checkpoint: bool,
    pub recover_checkpoint: bool,
    pub read_checkpoint: bool,
    /// Offer unfinished tasks of earlier sessions to a session without one.
    pub offer_handoff: bool,
}

/// What a lifecycle fact asks for under the effective `settings`. Checkpoints are saved at
/// turn completion and before context changes, never after each tool call.
///
/// A prompt's request establishes the session's intent even where the host cannot show its
/// hint. An edit is a side effect of that intent: it routes `implement` only when no request
/// established another intent, and only for a file not yet routed under it.
pub fn plan(facts: &Facts, settings: &Value) -> Plan {
    let request = facts.prompt.and_then(prompt::request);
    let edit = facts.edit
        && settings["routing"]["editHints"] == true
        && match facts.established {
            None => true,
            Some(intent) => intent == "implement" && !facts.file_routed,
        };
    let (intent, artifact) = if !facts.context_supported {
        (None, None)
    } else if edit {
        (Some("implement"), None)
    } else {
        request.as_ref().map_or((None, None), |request| {
            (Some(request.intent), request.artifact)
        })
    };
    let establish = match (&request, intent) {
        _ if !facts.session => None,
        (Some(request), _) => Some(request.intent),
        (None, Some("implement")) if edit => Some("implement"),
        _ => None,
    };
    let checkpoint = facts.session && settings["checkpoint"]["enabled"] == true;
    let services = settings["services"]
        .as_object()
        .is_some_and(|s| !s.is_empty());
    Plan {
        intent,
        artifact,
        establish,
        invalidate_hint: facts.session
            && settings["routing"]["dedupeHints"] == true
            && (facts.started || facts.context_changing || facts.context_restored),
        services: facts.started && services,
        save_checkpoint: checkpoint && (facts.turn_finished || facts.context_changing),
        recover_checkpoint: checkpoint && (facts.started || facts.context_restored),
        read_checkpoint: checkpoint && facts.context_changing,
        offer_handoff: checkpoint && facts.started,
    }
}

/// The hook decision: routing first, then a saved checkpoint, configured services, or
/// the continuation reminder before context changes.
pub fn decision(
    routing: Option<&Value>,
    checkpoint: Option<&Value>,
    services: Option<&Value>,
    preserve_continuation: bool,
) -> Value {
    let saved = checkpoint.filter(|c| !matches!(c["saved"], Value::Null | Value::Bool(false)));
    let context = if let Some(routing) = routing {
        json!({"kind": "routing", "data": routing})
    } else if let Some(checkpoint) = saved {
        json!({"kind": "checkpoint", "data": checkpoint})
    } else if let Some(services) = services {
        json!({"kind": "services", "data": services["services"]})
    } else if preserve_continuation {
        json!({"kind": "preserve"})
    } else {
        return json!({"decision": "allow"});
    };
    json!({"decision": "allow", "context": context})
}

/// A hook that could not be evaluated: shell policy fails closed, other hooks continue
/// and say why their context is unavailable.
pub fn failure(enforcement: bool, message: &str) -> Value {
    if enforcement {
        json!({"decision": "deny", "reason": format!("Policy could not be evaluated: {message}")})
    } else {
        json!({"decision": "allow", "context": {"kind": "unavailable", "data": message}})
    }
}

/// How long an emitted routing hint suppresses an identical one.
pub const HINT_TTL_MS: i64 = 15 * 60 * 1000;

/// A routing hint record `{formatVersion: 1, key, emittedAt}`; `Null` is no record.
pub fn validate_hint(state: &Value) -> Result<(), String> {
    if state.is_null() {
        return Ok(());
    }
    let key = match state.get("key") {
        Some(Value::Null) => true,
        Some(Value::String(key)) => lower_hex(key, 64),
        _ => false,
    };
    if state["formatVersion"] != 1 || !key || !state["emittedAt"].is_number() {
        return Err("invalid routing hint state".into());
    }
    Ok(())
}

/// The identity of a routing hint: the routing rules, the invalidation `generation`, the
/// effective settings and the structured context the host would receive.
pub fn hint_key(generation: Option<&str>, settings: &Value, context: &Value) -> String {
    let rules = [env!("CARGO_PKG_VERSION"), &crate::hash(SOURCE.as_bytes())];
    crate::digest(
        &json!({"rules": rules, "generation": generation, "settings": settings, "context": context}),
    )
}

/// Whether `previous` was emitted for the same `key` within the reuse window.
pub fn hint_valid(previous: &Value, key: &str, now: f64) -> bool {
    let Some(emitted) = previous["emittedAt"].as_f64() else {
        return false;
    };
    previous["key"] == key && now >= emitted && now - emitted < HINT_TTL_MS as f64
}

/// How many routed files an intent record keeps; an older file routes again.
pub const INTENT_FILES: usize = 32;

/// How many planning artifacts an intent record keeps; the oldest is dropped first.
pub const PLANNING_ARTIFACTS: usize = 8;

fn planning_entry(entry: &Value) -> bool {
    entry["path"].as_str().is_some_and(|p| !p.is_empty())
        && entry["contentHash"]
            .as_str()
            .is_some_and(|h| lower_hex(h, 64))
        && entry["revision"].as_u64().is_some_and(|r| r >= 1)
        && entry["intent"] == "plan"
}

/// A session intent record `{formatVersion: 1, intent, files, planning?}`; `Null` is no
/// record. `planning` lists the artifacts written while the session planned:
/// `{path, contentHash, revision, intent: "plan"}`.
pub fn validate_intent(state: &Value) -> Result<(), String> {
    if state.is_null() {
        return Ok(());
    }
    let files = state["files"]
        .as_array()
        .is_some_and(|files| files.len() <= INTENT_FILES && files.iter().all(Value::is_string));
    let planning = match state.get("planning") {
        None => true,
        Some(Value::Array(entries)) => {
            entries.len() <= PLANNING_ARTIFACTS && entries.iter().all(planning_entry)
        }
        Some(_) => false,
    };
    if state["formatVersion"] != 1 || !state["intent"].is_string() || !files || !planning {
        return Err("invalid session intent state".into());
    }
    Ok(())
}

/// Whether `file` was routed under the `implement` intent `record` holds.
pub fn file_routed(record: &Value, file: &str) -> bool {
    record["intent"] == "implement"
        && record["files"]
            .as_array()
            .is_some_and(|files| files.iter().any(|f| f == file))
}

/// The intent record after an event establishes `intent`: a request starts it afresh, an
/// edit adds its routed `file`. `None` when the record does not change.
pub fn established(record: &Value, intent: &str, file: Option<&str>) -> Option<Value> {
    let mut files: Vec<Value> = match file {
        Some(_) if record["intent"] == intent => {
            record["files"].as_array().cloned().unwrap_or_default()
        }
        Some(_) => vec![],
        None if record["intent"] == intent
            && record["files"].as_array().is_some_and(Vec::is_empty) =>
        {
            return None;
        }
        None => vec![],
    };
    if let Some(file) = file {
        if files.iter().any(|f| f == file) {
            return None;
        }
        files.push(json!(file));
        let excess = files.len().saturating_sub(INTENT_FILES);
        files.drain(..excess);
    }
    let mut next = json!({"formatVersion": 1, "intent": intent, "files": files});
    if let Some(planning) = record.get("planning") {
        next["planning"] = planning.clone();
    }
    Some(next)
}

/// The intent record after `path` was written with content `hash` while the session plans:
/// the artifact becomes planning evidence, its revision rising with each new content. `None`
/// outside a planning session or when the content is unchanged. The content is never read.
pub fn planning_recorded(record: &Value, path: &str, hash: &str) -> Option<Value> {
    if record["intent"] != "plan" {
        return None;
    }
    let mut entries = record["planning"].as_array().cloned().unwrap_or_default();
    match entries.iter_mut().find(|e| e["path"] == path) {
        Some(entry) if entry["contentHash"] == hash => return None,
        Some(entry) => {
            entry["contentHash"] = json!(hash);
            entry["revision"] = json!(entry["revision"].as_u64().unwrap_or(0) + 1);
        }
        None => {
            entries
                .push(json!({"path": path, "contentHash": hash, "revision": 1, "intent": "plan"}));
            let excess = entries.len().saturating_sub(PLANNING_ARTIFACTS);
            entries.drain(..excess);
        }
    }
    let mut next = record.clone();
    next["planning"] = json!(entries);
    Some(next)
}

mod prompt;
#[cfg(test)]
mod tests;
