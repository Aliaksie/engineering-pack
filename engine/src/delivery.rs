//! Context delivery at host lifecycle boundaries: the work a lifecycle fact asks for, the
//! context a hook delivers and when an identical routing hint stays quiet. Lifecycle facts
//! come from the application; host event names and wire formats never enter this policy.
use crate::routing::catalog::SOURCE;
use crate::values::{js_space, lower_hex};
use serde_json::{Value, json};

/// Host-neutral facts about one hook event.
#[derive(Default)]
pub struct Facts<'a> {
    pub session: bool,
    pub edit: bool,
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
    pub invalidate_hint: bool,
    pub services: bool,
    pub save_checkpoint: bool,
    pub recover_checkpoint: bool,
    pub read_checkpoint: bool,
    /// Offer unfinished tasks of earlier sessions to a session without one.
    pub offer_handoff: bool,
}

const PROMPT_INTENTS: [(&[&str], &str); 4] = [
    (&["review", "/review"], "review"),
    (&["plan", "/plan"], "plan"),
    (&["verify", "/verify"], "verify"),
    (&["find", "locate", "where"], "locate"),
];

/// Whether `prompt`, after leading white space, starts with `word` (ASCII case-insensitive)
/// followed by a word boundary.
fn leads_with(prompt: &str, word: &str) -> bool {
    let text = prompt.trim_start_matches(js_space);
    text.get(..word.len())
        .is_some_and(|head| head.eq_ignore_ascii_case(word))
        && !text[word.len()..]
            .chars()
            .next()
            .is_some_and(|c| c.is_ascii_alphanumeric() || c == '_')
}

fn prompt_intent(prompt: &str) -> Option<&'static str> {
    PROMPT_INTENTS
        .iter()
        .find(|(words, _)| words.iter().any(|word| leads_with(prompt, word)))
        .map(|(_, intent)| *intent)
}

/// What a lifecycle fact asks for under the effective `settings`. Checkpoints are saved at
/// turn completion and before context changes, never after each tool call.
pub fn plan(facts: &Facts, settings: &Value) -> Plan {
    let intent = if facts.edit && settings["routing"]["editHints"] == true {
        Some("implement")
    } else {
        facts.prompt.and_then(prompt_intent)
    };
    let checkpoint = facts.session && settings["checkpoint"]["enabled"] == true;
    let services = settings["services"]
        .as_object()
        .is_some_and(|s| !s.is_empty());
    Plan {
        intent,
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

#[cfg(test)]
mod tests;
