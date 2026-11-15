//! OTLP/HTTP JSON mapping of stored telemetry events and the interpretation of a bounded
//! export response. No provider objects cross the event boundary.
//! <https://opentelemetry.io/docs/specs/otlp/#json-protobuf-encoding>
use crate::timestamp;
use crate::values::truthy;
use serde_json::{Map, Value, json};

fn attribute(key: &str, value: &Value) -> Value {
    let typed = match value {
        Value::Bool(_) => json!({"boolValue": value}),
        Value::Number(_) => json!({"doubleValue": value}),
        _ => json!({"stringValue": value}),
    };
    json!({"key": key, "value": typed})
}

fn compact(id: &Value) -> String {
    id.as_str().unwrap_or_default().replace('-', "")
}

fn span(event: &Value) -> Result<Value, String> {
    let end = event["timestamp"]
        .as_str()
        .and_then(timestamp::parse)
        .ok_or("invalid telemetry timestamp")?;
    let duration = event["durationMs"].as_i64().unwrap_or(0);
    let operation = compact(&event["operationId"]);
    let mut span = json!({
        "traceId": event.get("traceId").and_then(Value::as_str).map_or_else(|| operation.clone(), str::to_string),
        "spanId": operation.get(..16).unwrap_or(&operation),
        "name": event["name"],
        "kind": 1,
    });
    if truthy(event.get("parentOperationId")) {
        let parent = compact(&event["parentOperationId"]);
        span["parentSpanId"] = json!(parent.get(..16).unwrap_or(&parent));
    }
    let start = (end - duration).max(0);
    span["startTimeUnixNano"] = json!(format!("{}", i128::from(start) * 1_000_000));
    span["endTimeUnixNano"] = json!(format!("{}", i128::from(end) * 1_000_000));
    let mut attributes = event["attributes"].as_object().cloned().unwrap_or_default();
    let mut extra = Map::new();
    if truthy(event.get("reasonCode")) {
        extra.insert(
            "engineering-pack.reason.code".into(),
            event["reasonCode"].clone(),
        );
    }
    for (key, field) in [
        ("engineering-pack.event.kind", "kind"),
        ("engineering-pack.session.id", "sessionId"),
        ("engineering-pack.outcome", "outcome"),
    ] {
        if let Some(value) = event.get(field) {
            extra.insert(key.into(), value.clone());
        }
    }
    attributes.extend(extra);
    span["attributes"] = attributes
        .iter()
        .map(|(key, value)| attribute(key, value))
        .collect();
    span["status"] = json!({"code": if event["outcome"] == "failure" { 2 } else { 0 }});
    let mut resource = vec![attribute("service.name", &json!("engineering-pack"))];
    if truthy(event.get("packVersion")) {
        resource.push(attribute("service.version", &event["packVersion"]));
    }
    if truthy(event.get("host")) {
        resource.push(attribute("engineering-pack.host", &event["host"]));
    }
    let mut scope = json!({"name": "engineering-pack"});
    if let Some(version) = event.get("packVersion") {
        scope["version"] = version.clone();
    }
    Ok(json!({
        "resource": {"attributes": resource},
        "scopeSpans": [{"scope": scope, "spans": [span]}],
    }))
}

/// One resource span per stored event.
pub fn traces(events: &[Value]) -> Result<Value, String> {
    let spans = events.iter().map(span).collect::<Result<Vec<_>, _>>()?;
    Ok(json!({"resourceSpans": spans}))
}

/// The rejected span count of a partial success, if it is a non-negative integer:
/// `Some(None)` when it is too large to report.
fn rejected(count: &Value) -> Option<Option<u64>> {
    let digits = match count {
        Value::Number(number) => match (number.as_u64(), number.as_f64()) {
            (Some(n), _) => n.to_string(),
            // Integral doubles below 1e21 print as plain digits, as JavaScript's String does.
            (None, Some(0.0)) => "0".into(),
            (None, Some(n)) if n > 0.0 && n.fract() == 0.0 && n < 1e21 => format!("{n:.0}"),
            _ => return None,
        },
        Value::String(text) => text.clone(),
        _ => return None,
    };
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let trimmed = digits.trim_start_matches('0');
    Some(if trimmed.len() > 10 {
        None
    } else {
        trimmed.parse().ok().filter(|n| *n <= 1_000_000_000)
    })
}

/// `{ok, statusCode?, reasonCode?, rejectedEvents?}` for a transport observation
/// `{ok, statusCode?, json?, failure?}`. Fields are added in the order JavaScript builds them.
pub fn outcome(result: &Value) -> Value {
    let mut value = json!({"ok": false});
    let status = |value: &mut Value, present: bool| {
        if present {
            value["statusCode"] = result["statusCode"].clone();
        }
    };
    let invalid = |mut value: Value| {
        value["reasonCode"] = json!("EXPORT_INVALID_RESPONSE");
        value
    };
    if result["ok"] != true {
        status(&mut value, truthy(result.get("statusCode")));
        value["reasonCode"] = json!(match result["failure"].as_str() {
            Some("timeout") => "EXPORT_TIMEOUT",
            Some("invalid-response") => "EXPORT_INVALID_RESPONSE",
            _ => "EXPORT_FAILED",
        });
        return value;
    }
    let present = result.get("statusCode").is_some();
    status(&mut value, present);
    let Some(body) = result.get("json").and_then(Value::as_object) else {
        return invalid(value);
    };
    if let Some(partial) = body.get("partialSuccess") {
        let zero = json!(0);
        let count = partial
            .get("rejectedSpans")
            .filter(|c| !c.is_null())
            .unwrap_or(&zero);
        let Some(reported) = rejected(count) else {
            return invalid(value);
        };
        let positive = match count {
            Value::String(text) => text.bytes().any(|b| b != b'0'),
            _ => count.as_f64().is_some_and(|n| n > 0.0),
        };
        if positive {
            value["reasonCode"] = json!("EXPORT_PARTIAL_REJECTED");
            if let Some(count) = reported {
                value["rejectedEvents"] = json!(count);
            }
            return value;
        }
        if truthy(partial.get("errorMessage")) {
            value["ok"] = json!(true);
            value["reasonCode"] = json!("EXPORT_WARNING");
            return value;
        }
    }
    value["ok"] = json!(true);
    value
}
