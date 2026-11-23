//! The delegated execution a role may use on a host, from the validated effective settings.
//! Nothing here calls or switches a model, dispatches a worker or grants permission.
use serde_json::{Value, json};

/// Own-property lookup in a settings object; host and profile names come from callers.
fn entry<'a>(value: Option<&'a Value>, key: &str) -> Option<&'a Value> {
    value.and_then(|value| value.get(key))
}

/// The delegated execution a role may use on a host, or `None` when it stays with the
/// fallback. Exact host mappings are declarations, never a model switch or permission grant.
pub fn delegation(
    settings: &Value,
    role: &str,
    host: &str,
    profile: Option<&str>,
) -> Option<Value> {
    let workers = settings.pointer("/efficiency/delegation/maxWorkers");
    if workers.and_then(Value::as_f64) == Some(0.0) {
        return None;
    }
    let execution = settings.get("execution");
    let rule = entry(entry(execution, "roles"), role);
    let mapping = entry(entry(execution, "hosts"), host);
    if entry(rule, "mode").and_then(Value::as_str) != Some("delegate")
        || entry(mapping, "delegation") != Some(&Value::Bool(true))
    {
        return None;
    }
    let mut result = json!({ "mode": "delegate", "role": role });
    if let Some(profile) = profile {
        let model = entry(entry(mapping, "models"), profile)
            .and_then(Value::as_str)
            .filter(|model| !model.is_empty())?;
        result["model"] = json!(model);
        result["profile"] = json!(profile);
    }
    Some(result)
}
