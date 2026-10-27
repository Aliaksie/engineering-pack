//! Schema-1 telemetry envelope. Events carry bounded counts, closed enums and
//! pseudonymous IDs only; no input bodies.
use crate::values::lower_hex as hex;
use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};

pub mod report;

const HOSTS: &[&str] = &[
    "claude",
    "codex",
    "cursor",
    "copilot",
    "gemini",
    "agents",
    "jetbrains",
    "gemini-extension",
];
const COUNTS: &[&str] = &[
    "scratchBytes",
    "evidenceBytes",
    "workspacesCleaned",
    "guidanceFiles",
    "guidanceBytes",
    "estimatedTokens",
    "evidenceCount",
    "batchSize",
    "cacheHits",
    "cacheMisses",
    "recordedAttempts",
    "selectedSkills",
    "checkpoints",
    "unavailableCapabilities",
    "droppedEvents",
    "exportedEvents",
    "rejectedEvents",
    "statusCode",
];
const FLAGS: &[&str] = &[
    "reused",
    "occupancyKnown",
    "available",
    "complete",
    "contextSupported",
];
const ENUMS: &[(&str, &[&str])] = &[
    (
        "workspaceAction",
        &[
            "open", "write", "promote", "release", "complete", "maintain", "touch",
        ],
    ),
    (
        "package",
        &[
            "craft-delivery",
            "craft-engineering",
            "craft-review",
            "craft-stories",
        ],
    ),
    (
        "inputStatus",
        &["complete", "invalid", "oversized", "read-failure"],
    ),
    (
        "area",
        &[
            "cli",
            "host",
            "project",
            "routing",
            "guidance",
            "discovery",
            "cache",
            "context",
            "session",
            "model",
            "attempt",
            "checkpoint",
            "review",
            "integration",
            "state",
            "telemetry",
        ],
    ),
    (
        "command",
        &[
            "workspace-open",
            "workspace-write",
            "workspace-promote",
            "workspace-release",
            "workspace-clean",
            "state-clean",
            "worker-status",
            "task-feature",
            "task-features",
            "telemetry-capture",
            "telemetry-forget",
            "telemetry-report",
            "task-offer",
            "task-decision",
            "task-action-check",
            "settings",
            "task-start",
            "task-step",
            "route",
            "context-report",
            "session",
            "session-advice",
            "session-attempt",
            "model-advice",
            "lookup",
            "locate",
            "prepare-services",
            "operation",
            "checkpoint-save",
            "checkpoint-read",
            "acceptance-view",
            "project-evidence",
            "unknown",
        ],
    ),
    (
        "operation",
        &[
            "git.status",
            "git.diff",
            "git.commit",
            "git.push",
            "change.read",
            "change.diff",
            "change.publish",
            "task.read",
            "task.search",
            "task.transition",
            "task.comment",
        ],
    ),
    (
        "phase",
        &[
            "prepared",
            "validated",
            "publication-planned",
            "followup-checked",
        ],
    ),
    (
        "intent",
        &[
            "implement",
            "review",
            "plan",
            "verify",
            "inspect",
            "deliver",
            "ship",
            "fix",
            "test",
            "architecture",
            "investigate",
            "locate",
            "status",
            "run-check",
            "configure-service",
            "maintain-context",
        ],
    ),
    (
        "decision",
        &[
            "continue-task",
            "allow",
            "deny",
            "continue",
            "compact",
            "checkpoint",
            "new",
            "start-new-session",
            "escalate",
            "defer",
            "reuse",
            "fresh",
            "unknown",
            "unavailable",
            "checkpoint-and-reset",
            "continue-and-reassess",
            "checkpoint-then-compact",
            "save-handoff",
            "retry-differently",
            "run-check",
            "design-verification",
            "review",
            "summarize-evidence",
            "stop-blocked",
            "return-control",
            "escalate-model",
            "recommend",
            "stop",
            "completed",
            "inspect-failure",
            "resolve-blocker",
            "continue-bounded-step",
        ],
    ),
    (
        "pressure",
        &[
            "unknown",
            "low",
            "normal",
            "moderate",
            "high",
            "critical",
            "elevated",
            "saturated",
            "healthy",
        ],
    ),
    ("capabilityClass", &["economical", "standard", "strong"]),
    ("identityScope", &["session", "invocation"]),
    (
        "reason",
        &[
            "observed",
            "unavailable",
            "stale",
            "busy",
            "invalid",
            "disabled",
            "budget-exhausted",
            "deferred",
            "recorded",
            "missing",
            "incomplete",
        ],
    ),
    (
        "lifecycle",
        &[
            "session-start",
            "before-prompt",
            "before-tool",
            "after-tool",
            "before-compact",
            "after-compact",
            "turn-end",
        ],
    ),
];
pub const EVENT_NAMES: &[&str] = &[
    "workspace.observed",
    "worker.dispatch",
    "cli.command",
    "host.hook",
    "host.session",
    "project.loaded",
    "input.loaded",
    "search.completed",
    "cache.lookup",
    "cache.invalidated",
    "state.read",
    "state.updated",
    "checkpoint.read",
    "review.command",
    "review.publication-planned",
    "integration.prepared",
    "integration.planned",
    "integration.observed",
    "telemetry.export",
    "diagnostic",
    "session.observed",
    "routing.completed",
    "guidance.selected",
    "discovery.completed",
    "cache.observed",
    "context.observed",
    "session.advice",
    "model.advice",
    "attempt.recorded",
    "checkpoint.saved",
    "checkpoint.recovered",
    "review.completed",
    "host.observed",
];
pub const REASON_CODES: &[&str] = &[
    "OPERATION_FAILED",
    "INVALID_INPUT",
    "INPUT_TOO_LARGE",
    "CONFIG_INVALID",
    "ACCESS_DENIED",
    "NOT_FOUND",
    "STATE_BUSY",
    "STATE_UNAVAILABLE",
    "STORAGE_FULL",
    "CACHE_UNAVAILABLE",
    "CHECKPOINT_UNAVAILABLE",
    "CHECKPOINT_STALE",
    "CONTEXT_UNAVAILABLE",
    "HOST_CAPABILITY_UNAVAILABLE",
    "HOST_OBSERVATION_UNAVAILABLE",
    "HOOK_FAILED",
    "SEARCH_FAILED",
    "SEARCH_INCOMPLETE",
    "ROUTING_FAILED",
    "REVIEW_INVALID",
    "VALIDATION_FAILED",
    "INTEGRATION_UNAVAILABLE",
    "ATTEMPT_BUDGET_EXHAUSTED",
    "MODEL_ADVICE_UNAVAILABLE",
    "TELEMETRY_CONFIG_INVALID",
    "TELEMETRY_EVENT_INVALID",
    "TELEMETRY_METADATA_UNAVAILABLE",
    "TELEMETRY_IDENTITY_UNAVAILABLE",
    "TELEMETRY_STORAGE_UNAVAILABLE",
    "TELEMETRY_EVENTS_DROPPED",
    "EXPORT_QUEUE_FULL",
    "EXPORT_FAILED",
    "EXPORT_TIMEOUT",
    "EXPORT_PARTIAL_REJECTED",
    "EXPORT_WARNING",
    "EXPORT_INVALID_RESPONSE",
];

/// One completed observation or operation, before sanitizing.
#[derive(Debug, Clone, Default)]
pub struct Event<'a> {
    pub name: &'a str,
    pub timestamp: String,
    pub session_id: &'a str,
    pub trace_id: &'a str,
    pub operation_id: String,
    pub parent_operation_id: Option<String>,
    pub pack_version: Option<&'a str>,
    pub host: Option<&'a str>,
    pub attributes: Map<String, Value>,
    pub outcome: Option<&'a str>,
    pub duration_ms: Option<i64>,
    pub reason_code: Option<&'a str>,
    /// Operations have a duration and may parent other events.
    pub operation: bool,
}

/// Build the stored envelope. Unknown attributes, hosts and values are dropped;
/// `skills` lists the catalog identifiers accepted for a `skill` attribute.
pub fn envelope(event: &Event, skills: &[&str]) -> Result<Value, String> {
    if !EVENT_NAMES.contains(&event.name)
        || !hex(event.session_id, 32)
        || event.pack_version.is_some_and(|v| !version(v))
    {
        return Err("invalid telemetry envelope".into());
    }
    if !hex(event.trace_id, 32)
        || event.trace_id.bytes().all(|b| b == b'0')
        || !operation_id(&event.operation_id)
        || event
            .parent_operation_id
            .as_ref()
            .is_some_and(|p| !operation_id(p) || *p == event.operation_id)
    {
        return Err("invalid trace context".into());
    }
    let diagnostic = event.name == "diagnostic";
    if diagnostic
        && !event
            .reason_code
            .is_some_and(|code| REASON_CODES.contains(&code))
    {
        return Err("diagnostic requires a stable reason code".into());
    }
    let mut attributes = Map::new();
    for (key, value) in event.attributes.iter().take(64) {
        let enumerated = ENUMS.iter().find(|(name, _)| name == key);
        let safe = if COUNTS.contains(&key.as_str()) {
            crate::values::safe_integer(value).is_some_and(|n| (0..=1_000_000_000).contains(&n))
        } else if let Some((_, values)) = enumerated {
            value.as_str().is_some_and(|v| values.contains(&v))
        } else if FLAGS.contains(&key.as_str()) {
            value.is_boolean()
        } else {
            key == "skill" && value.as_str().is_some_and(|v| skills.contains(&v))
        };
        if safe {
            attributes.insert(key.clone(), value.clone());
        }
    }
    let mut stored = json!({
        "schemaVersion": 1,
        "name": event.name,
        "kind": if diagnostic { "diagnostic" } else if event.operation { "operation" } else { "observation" },
        "timestamp": event.timestamp,
        "sessionId": event.session_id,
        "traceId": event.trace_id,
        "operationId": event.operation_id,
    });
    let map = stored.as_object_mut().expect("envelope object");
    if let Some(parent) = &event.parent_operation_id {
        map.insert("parentOperationId".into(), json!(parent));
    }
    map.insert("packVersion".into(), json!(event.pack_version));
    if let Some(host) = event.host.filter(|h| HOSTS.contains(h)) {
        map.insert("host".into(), json!(host));
    }
    map.insert("attributes".into(), Value::Object(attributes));
    if let Some(code) = event.reason_code.filter(|c| REASON_CODES.contains(c)) {
        map.insert("reasonCode".into(), json!(code));
    }
    if let Some(outcome) = event
        .outcome
        .filter(|o| ["success", "failure", "deferred"].contains(o))
    {
        map.insert("outcome".into(), json!(outcome));
    }
    if let Some(duration) = event.duration_ms.filter(|d| (0..=86_400_000).contains(d)) {
        map.insert("durationMs".into(), json!(duration));
    }
    Ok(stored)
}

fn version(text: &str) -> bool {
    let parts: Vec<_> = text.split('.').collect();
    parts.len() == 3
        && parts
            .iter()
            .all(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit()))
}

fn operation_id(text: &str) -> bool {
    let groups: Vec<_> = text.split('-').collect();
    groups.len() == 5
        && groups
            .iter()
            .zip([8, 4, 4, 4, 12])
            .all(|(group, length)| hex(group, length))
        && text.replace('-', "")[..16] != *"0".repeat(16)
}

/// A session pseudonym: `HMAC-SHA256(key, session)` truncated to 32 hex digits. The key
/// is used as its UTF-8 text, as Node's `createHmac('sha256', key)` does with a string.
pub fn session_identity(key: &str, session: &str) -> String {
    let mut block = [0_u8; 64];
    if key.len() > 64 {
        block[..32].copy_from_slice(&Sha256::digest(key.as_bytes()));
    } else {
        block[..key.len()].copy_from_slice(key.as_bytes());
    }
    let pad = |byte: u8| block.map(|b| b ^ byte);
    let inner = Sha256::new()
        .chain_update(pad(0x36))
        .chain_update(session.as_bytes())
        .finalize();
    let outer = Sha256::new()
        .chain_update(pad(0x5c))
        .chain_update(inner)
        .finalize();
    format!("{outer:x}")[..32].to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn event(name: &'static str, attributes: Value) -> Event<'static> {
        Event {
            name,
            timestamp: "2026-10-19T00:00:00.000Z".into(),
            session_id: "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            trace_id: "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
            operation_id: "12345678-1234-4234-a234-123456789abc".into(),
            pack_version: Some("0.0.2"),
            host: Some("codex"),
            attributes: attributes.as_object().unwrap().clone(),
            outcome: Some("success"),
            duration_ms: Some(12),
            ..Event::default()
        }
    }

    #[test]
    fn envelopes_keep_only_bounded_safe_metadata() {
        let stored = envelope(
            &event(
                "routing.completed",
                json!({"intent": "implement", "guidanceBytes": 10, "evidenceCount": null, "batchSize": -1,
                    "sourceCode": "secret", "command": "secret", "host": "secret",
                    "skill": "consumer:secret", "reason": "token-value", "prompt": "secret"}),
            ),
            &[],
        )
        .unwrap();
        assert_eq!(
            stored["attributes"],
            json!({"intent": "implement", "guidanceBytes": 10})
        );
        assert!(!stored.to_string().contains("secret"));
        let skill = json!({"skill": "craft-review:review-change"});
        let named = envelope(
            &event("routing.completed", skill.clone()),
            &["craft-review:review-change"],
        )
        .unwrap();
        assert_eq!(named["attributes"], skill);
        let mut unknown = event("arbitrary", json!({}));
        assert!(envelope(&unknown, &[]).is_err());
        unknown.name = "routing.completed";
        unknown.session_id = "raw-session";
        assert!(envelope(&unknown, &[]).is_err());
        let mut version = event("routing.completed", json!({}));
        version.pack_version = Some("bad");
        assert!(envelope(&version, &[]).is_err());
        let mut host = event("routing.completed", json!({}));
        host.host = Some("secret");
        assert_eq!(envelope(&host, &[]).unwrap().get("host"), None);
    }

    #[test]
    fn trace_context_and_diagnostics_are_validated() {
        let mut diagnostic = event(
            "diagnostic",
            json!({"area": "project", "constructor": "private", "toString": "private", "error": "private",
                "path": "/private/secret", "command": "rm secret", "droppedEvents": 2}),
        );
        diagnostic.reason_code = Some("CONFIG_INVALID");
        diagnostic.parent_operation_id = Some("87654321-1234-4234-a234-123456789abc".into());
        let stored = envelope(&diagnostic, &[]).unwrap();
        assert_eq!(stored, envelope(&diagnostic, &[]).unwrap());
        assert_eq!(
            stored["attributes"],
            json!({"area": "project", "droppedEvents": 2})
        );
        assert_eq!(stored["kind"], "diagnostic");
        assert!(!stored.to_string().contains("private"));
        let mut raw = diagnostic.clone();
        raw.reason_code = Some("raw private error");
        assert!(envelope(&raw, &[]).is_err());
        let mut zero = diagnostic.clone();
        zero.trace_id = "00000000000000000000000000000000";
        assert!(envelope(&zero, &[]).is_err());
        let mut own_parent = diagnostic;
        own_parent.parent_operation_id = Some(own_parent.operation_id.clone());
        assert!(envelope(&own_parent, &[]).is_err());
    }

    #[test]
    fn identity_matches_node_hmac() {
        // Reference values from Node's createHmac('sha256', key).update(session).
        assert_eq!(
            session_identity(&"k".repeat(64), "s1"),
            "64a1ad3872583d2eeb587fbdbd4f4603"
        );
        // Keys longer than one block are hashed first.
        assert_eq!(
            session_identity(&"x".repeat(70), ""),
            "979a542ffef7c6369070ee82f2f7f657"
        );
    }
}
