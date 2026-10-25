//! Summaries of retained telemetry and the explicit captures that preserve them. Counts
//! describe retained observations, never applicability or semantic success.
use super::{Event, envelope};
use crate::{contracts::safe_integer, timestamp};
use serde_json::{Map, Value, json};

pub const CAPTURE_MAX_BYTES: usize = 65_536;
pub const CAPTURE_MAX_ENTRIES: usize = 8;

const FEATURES: &[(&str, &[&str])] = &[
    ("workspace", &["workspace.observed"]),
    ("routing", &["routing.completed"]),
    ("guidanceSelection", &["guidance.selected"]),
    ("discovery", &["discovery.completed"]),
    ("lookupCache", &["cache.observed"]),
    ("attempts", &["attempt.recorded"]),
    ("modelAdvice", &["model.advice"]),
    ("checkpoints", &["checkpoint.saved", "checkpoint.recovered"]),
    ("context", &["context.observed"]),
    ("sessionAdvice", &["session.advice"]),
    ("review", &["review.completed"]),
    ("services", &["integration.prepared", "integration.planned"]),
    ("nativeHooks", &["host.hook"]),
];
const LIMITS: &[&str] = &[
    "Retained best-effort observations cannot establish complete history; rotation, deletion, disabled sinks and unreported losses remain unknown.",
    "Storage and invalid-record counts cover the project files, including when events are filtered by session. Reads do not lock concurrent writers.",
    "Unobserved does not mean disabled, unsupported or inapplicable. Observed events do not prove guidance use, acceptance quality or native action execution.",
    "Attempt events are observations, not a sum of cumulative recordedAttempts. No task, model, worker or compaction execution is inferred.",
];
const PACKAGES: &[&str] = &[
    "craft-delivery",
    "craft-engineering",
    "craft-review",
    "craft-stories",
    "unknown",
];
const HOOK_INPUTS: &[&str] = &[
    "complete",
    "invalid",
    "oversized",
    "read-failure",
    "unknown",
];

/// Which events a summary counts.
#[derive(Clone, Copy)]
pub enum Scope<'a> {
    Project,
    /// Session-scoped events of this pseudonym; `None` when no key exists, so nothing matches.
    Session(Option<&'a str>),
}

/// Storage facts of the retained files; invalid records are counted by the summary.
#[derive(Debug, Default)]
pub struct Storage {
    pub files: u64,
    pub bytes: u64,
    pub backup_present: bool,
    pub malformed_lines: u64,
    pub unsupported_records: u64,
    pub oversized_lines: u64,
    pub max_file_bytes: u64,
}

impl Storage {
    fn to_value(&self, invalid_records: u64) -> Value {
        json!({
            "files": self.files, "bytes": self.bytes, "backupPresent": self.backup_present,
            "malformedLines": self.malformed_lines, "unsupportedRecords": self.unsupported_records,
            "oversizedLines": self.oversized_lines, "maxFileBytes": self.max_file_bytes,
            "backupCount": 1, "completeHistory": false, "concurrentSnapshot": true,
            "invalidRecords": invalid_records,
        })
    }
}

/// A timestamp as producers write it: `YYYY-MM-DDTHH:MM:SS.mmmZ`, a real instant, not before 1970.
pub fn stored_timestamp(text: &str) -> bool {
    let bytes = text.as_bytes();
    bytes.len() == 24
        && bytes.iter().enumerate().all(|(i, b)| match i {
            4 | 7 => *b == b'-',
            10 => *b == b'T',
            13 | 16 => *b == b':',
            19 => *b == b'.',
            23 => *b == b'Z',
            _ => b.is_ascii_digit(),
        })
        && timestamp::parse(text).is_some_and(|ms| ms >= 0)
}

/// A stored record as the producer's envelope would have written it; `None` when invalid.
/// Legacy records without a trace ID take it from the operation ID.
fn event(record: &Value) -> Option<Value> {
    let text = |name: &str| record.get(name).and_then(Value::as_str);
    let timestamp = text("timestamp").filter(|t| stored_timestamp(t))?;
    let operation_id = record.get("operationId");
    let trace = match record.get("traceId") {
        None | Some(Value::Null) => operation_id?.as_str()?.replace('-', ""),
        Some(Value::String(trace)) => trace.clone(),
        Some(_) => return None,
    };
    let pack_version = match record.get("packVersion")? {
        Value::Null => None,
        Value::String(version) => Some(version.as_str()),
        _ => return None,
    };
    let parent_operation_id = match record.get("parentOperationId") {
        None => None,
        Some(parent) => Some(parent.as_str()?.to_string()),
    };
    let attributes = match record.get("attributes") {
        Some(Value::Null) => return None,
        Some(Value::Object(attributes)) => attributes.clone(),
        _ => Map::new(),
    };
    envelope(
        &Event {
            name: text("name")?,
            timestamp: timestamp.to_string(),
            session_id: text("sessionId")?,
            trace_id: &trace,
            operation_id: operation_id?.as_str()?.to_string(),
            parent_operation_id,
            pack_version,
            host: text("host"),
            attributes,
            outcome: text("outcome"),
            duration_ms: None,
            reason_code: text("reasonCode"),
            operation: false,
        },
        &[],
    )
    .ok()
}

/// The bounded report over retained records, including its interpretation limits.
pub fn summarize(records: &[Value], storage: &Storage, scope: Scope) -> Value {
    let mut counts = Map::new();
    for (name, _) in FEATURES {
        counts.insert(
            (*name).into(),
            json!({ "status": "unobserved", "events": 0, "failures": 0, "deferred": 0 }),
        );
    }
    let (mut retained, mut invalid, mut invocation, mut session) = (0, 0_u64, 0, 0);
    let (mut local_drops, mut queue_drops, mut export_failures) = (0, 0, 0);
    let (mut first, mut last): (Option<String>, Option<String>) = (None, None);
    let (mut packages, mut hook_inputs) = (Map::new(), Map::new());
    let increment = |map: &mut Map<String, Value>, key: &str| {
        let count = map.get(key).and_then(Value::as_u64).unwrap_or(0);
        map.insert(key.into(), json!(count + 1));
    };
    for record in records {
        let Some(event) = event(record) else {
            invalid += 1;
            continue;
        };
        let attributes = &event["attributes"];
        let session_scoped = attributes["identityScope"] == "session";
        if let Scope::Session(id) = scope
            && (id != event["sessionId"].as_str() || !session_scoped)
        {
            continue;
        }
        retained += 1;
        increment(
            &mut packages,
            attributes["package"].as_str().unwrap_or("unknown"),
        );
        let name = event["name"].as_str().unwrap_or_default();
        if name == "host.hook" {
            increment(
                &mut hook_inputs,
                attributes["inputStatus"].as_str().unwrap_or("unknown"),
            );
        }
        if session_scoped {
            session += 1;
        } else {
            invocation += 1;
        }
        let at = event["timestamp"].as_str().unwrap_or_default();
        if first.as_deref().is_none_or(|first| at < first) {
            first = Some(at.into());
        }
        if last.as_deref().is_none_or(|last| at > last) {
            last = Some(at.into());
        }
        let dropped = safe_integer(&attributes["droppedEvents"]).unwrap_or(0);
        match event["reasonCode"].as_str() {
            Some("TELEMETRY_EVENTS_DROPPED") => local_drops += dropped,
            Some("EXPORT_QUEUE_FULL") => queue_drops += dropped,
            _ => {}
        }
        let outcome = event["outcome"].as_str();
        if name == "telemetry.export" && outcome == Some("failure") {
            export_failures += 1;
        }
        if let Some((feature, _)) = FEATURES.iter().find(|(_, names)| names.contains(&name)) {
            let item = &mut counts[*feature];
            item["status"] = json!("observed");
            let mut add = |field: &str| item[field] = json!(item[field].as_i64().unwrap_or(0) + 1);
            add("events");
            match outcome {
                Some("failure") => add("failures"),
                Some("deferred") => add("deferred"),
                _ => {}
            }
        }
    }
    json!({
        "formatVersion": 1,
        "scope": if matches!(scope, Scope::Project) { "retained-project" } else { "retained-session" },
        "retainedEvents": retained,
        "identity": { "sessionEvents": session, "invocationOrUnknownEvents": invocation },
        "attribution": { "packages": packages, "hookInputs": hook_inputs },
        "window": { "first": first, "last": last },
        "storage": storage.to_value(invalid),
        "loss": {
            "reportedLocalDrops": local_drops, "reportedQueueDrops": queue_drops,
            "exportFailures": export_failures, "totalLostEvents": null,
        },
        "features": counts,
        "limits": LIMITS,
    })
}

pub fn limits() -> Value {
    json!(LIMITS)
}

/// Capture bounds, as every capture command reports them.
pub fn capture_limits() -> Value {
    json!({ "maxEntries": CAPTURE_MAX_ENTRIES, "maxBytes": CAPTURE_MAX_BYTES, "automaticEviction": false })
}

/// The identifier of a retained report: the first 24 digits of its digest with the session.
pub fn capture_id(session_id: &str, report: &Value) -> String {
    crate::digest(&json!([session_id, report]))[..24].to_string()
}

fn hex(text: &str, length: usize) -> bool {
    text.len() == length && text.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

pub fn valid_capture_id(text: &str) -> bool {
    hex(text, 24)
}

/// A retained report has exactly the shape of a session report without its limits.
fn valid_report(value: &Value, expected: &Value, field: &str) -> bool {
    let allowed = match field {
        "attribution.packages" => Some(PACKAGES),
        "attribution.hookInputs" => Some(HOOK_INPUTS),
        _ => None,
    };
    if let Some(allowed) = allowed {
        return value.as_object().is_some_and(|map| {
            map.iter().all(|(key, count)| {
                allowed.contains(&key.as_str()) && safe_integer(count).is_some_and(|n| n >= 0)
            })
        });
    }
    match expected {
        Value::Null if field.starts_with("window.") => {
            value.is_null() || value.as_str().is_some_and(stored_timestamp)
        }
        Value::Null => value.is_null(),
        Value::Number(_) => {
            safe_integer(value).is_some_and(|n| n >= 0 && (field != "formatVersion" || n == 1))
        }
        Value::Bool(fixed) => {
            value.is_boolean()
                && (!["storage.completeHistory", "storage.concurrentSnapshot"].contains(&field)
                    || value == &Value::Bool(*fixed))
        }
        Value::String(_) if field.ends_with(".status") => value
            .as_str()
            .is_some_and(|v| ["observed", "unobserved"].contains(&v)),
        Value::String(text) => value.as_str() == Some(text),
        Value::Object(expected) => value.as_object().is_some_and(|map| {
            map.len() == expected.len()
                && expected.iter().all(|(key, expected)| {
                    let path = if field.is_empty() {
                        key.clone()
                    } else {
                        format!("{field}.{key}")
                    };
                    map.get(key)
                        .is_some_and(|value| valid_report(value, expected, &path))
                })
        }),
        Value::Array(_) => false,
    }
}

/// Validate retained capture state, as numbers normalized the way `JSON.parse` reads them.
/// The state error covers the container; the capture error any invalid entry.
pub fn validate_captures(state: &Value) -> Result<(), &'static str> {
    let captures = state["captures"].as_array();
    if state.as_object().is_none_or(|map| map.len() != 2)
        || safe_integer(&state["formatVersion"]) != Some(1)
        || captures.is_none_or(|captures| captures.len() > CAPTURE_MAX_ENTRIES)
    {
        return Err("invalid telemetry capture state");
    }
    let mut template = summarize(&[], &Storage::default(), Scope::Session(None));
    template.as_object_mut().expect("report").remove("limits");
    let mut seen = Vec::new();
    for entry in captures.into_iter().flatten() {
        let text = |name: &str| entry[name].as_str().unwrap_or_default();
        let valid = entry.as_object().is_some_and(|map| map.len() == 4)
            && valid_capture_id(text("id"))
            && stored_timestamp(text("capturedAt"))
            && hex(text("sessionId"), 32)
            && valid_report(&entry["report"], &template, "")
            && capture_id(text("sessionId"), &entry["report"]) == text("id")
            && !seen.contains(&text("id"));
        if !valid {
            return Err("invalid retained telemetry capture");
        }
        seen.push(text("id"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(name: &str, extra: Value) -> Value {
        let mut record = json!({
            "schemaVersion": 1, "name": name, "kind": "observation",
            "timestamp": "2026-10-20T10:00:00.000Z", "sessionId": "a".repeat(32),
            "traceId": "b".repeat(32), "operationId": "1234abcd-1234-4abc-8abc-000000000001",
            "packVersion": "0.0.2", "attributes": { "identityScope": "session" },
        });
        for (key, value) in extra.as_object().unwrap() {
            record[key] = value.clone();
        }
        record
    }

    #[test]
    fn records_revalidate_like_their_producer() {
        assert!(event(&record("routing.completed", json!({}))).is_some());
        // A legacy record takes its trace from the operation ID; null trace means absent.
        let mut legacy = record("routing.completed", json!({ "traceId": null }));
        assert!(event(&legacy).is_some());
        legacy.as_object_mut().unwrap().remove("traceId");
        assert!(event(&legacy).is_some());
        for invalid in [
            json!({ "timestamp": "2026-02-30T00:00:00.000Z" }),
            json!({ "timestamp": "1969-12-31T23:59:59.999Z" }),
            json!({ "timestamp": "2026-10-20T10:00:00Z" }),
            json!({ "packVersion": "1.2" }),
            json!({ "parentOperationId": null }),
            json!({ "attributes": null }),
            json!({ "traceId": 5 }),
            json!({ "name": "unknown.event" }),
        ] {
            assert!(
                event(&record("routing.completed", invalid.clone())).is_none(),
                "{invalid}"
            );
        }
        let mut missing = record("routing.completed", json!({}));
        missing.as_object_mut().unwrap().remove("packVersion");
        assert!(event(&missing).is_none());
        assert!(event(&record("diagnostic", json!({}))).is_none());
        // Non-object attributes carry no attributes, as `Object.entries` reads them.
        assert!(event(&record("routing.completed", json!({ "attributes": "ab" }))).is_some());
    }

    #[test]
    fn summaries_filter_sessions_and_keep_losses_separate() {
        let session = "a".repeat(32);
        let records = [
            record("discovery.completed", json!({ "outcome": "deferred" })),
            record(
                "diagnostic",
                json!({ "reasonCode": "TELEMETRY_EVENTS_DROPPED", "attributes": { "droppedEvents": 3, "identityScope": "session" } }),
            ),
            record("telemetry.export", json!({ "outcome": "failure" })),
            record(
                "host.hook",
                json!({ "attributes": { "inputStatus": "oversized" } }),
            ),
            json!({ "schemaVersion": 1 }),
        ];
        let storage = Storage {
            files: 1,
            bytes: 10,
            ..Storage::default()
        };
        let all = summarize(&records, &storage, Scope::Project);
        assert_eq!(all["retainedEvents"], 4);
        assert_eq!(all["storage"]["invalidRecords"], 1);
        assert_eq!(all["identity"]["invocationOrUnknownEvents"], 1);
        assert_eq!(
            all["attribution"],
            json!({ "packages": { "unknown": 4 }, "hookInputs": { "oversized": 1 } })
        );
        assert_eq!(
            all["features"]["discovery"],
            json!({ "status": "observed", "events": 1, "failures": 0, "deferred": 1 })
        );
        assert_eq!(
            all["loss"],
            json!({ "reportedLocalDrops": 3, "reportedQueueDrops": 0, "exportFailures": 1, "totalLostEvents": null })
        );
        let own = summarize(&records, &storage, Scope::Session(Some(&session)));
        assert_eq!(
            (own["scope"].as_str(), own["retainedEvents"].as_i64()),
            (Some("retained-session"), Some(3))
        );
        assert_eq!(
            summarize(&records, &storage, Scope::Session(None))["retainedEvents"],
            0
        );
    }

    #[test]
    fn captures_validate_shape_identity_and_bounds() {
        let session = "a".repeat(32);
        let mut report = summarize(
            &[record("routing.completed", json!({}))],
            &Storage::default(),
            Scope::Session(Some(&session)),
        );
        report.as_object_mut().unwrap().remove("limits");
        let entry = json!({ "id": capture_id(&session, &report), "capturedAt": "2026-10-25T00:00:00.000Z", "sessionId": session, "report": report });
        assert_eq!(
            validate_captures(&json!({ "formatVersion": 1, "captures": [entry] })),
            Ok(())
        );
        assert_eq!(
            validate_captures(&json!({ "formatVersion": 1, "captures": [entry, entry] })),
            Err("invalid retained telemetry capture")
        );
        assert_eq!(
            validate_captures(&json!({ "formatVersion": 1, "captures": vec![json!({}); 9] })),
            Err("invalid telemetry capture state")
        );
        let mut changed = entry.clone();
        changed["report"]["retainedEvents"] = json!(2);
        assert!(validate_captures(&json!({ "formatVersion": 1, "captures": [changed] })).is_err());
        let mut scoped = entry.clone();
        scoped["report"]["scope"] = json!("retained-project");
        assert!(validate_captures(&json!({ "formatVersion": 1, "captures": [scoped] })).is_err());
    }
}
