//! Model-profile advice over validated effective settings: which configured logical profile
//! suits a role, given declared capabilities and cited current-target attempts. Nothing
//! here calls or switches a model, dispatches a worker or grants permission.
use crate::{
    contracts::{ROLES, kebab},
    values::{js_blank, js_length, safe_integer},
};
use serde_json::{Map, Value, json};

type Result<T> = std::result::Result<T, String>;

const CLASSES: [&str; 3] = ["economical", "standard", "strong"];
const OUTCOMES: [&str; 4] = [
    "quality-failure",
    "permission-denied",
    "tool-unavailable",
    "passed",
];
const BLOCKERS: [&str; 2] = ["permission-denied", "tool-unavailable"];
/// Escalation chains are acyclic over at most 16 profiles; this bound is only a guard.
const MAX_CHAIN: usize = 16;

fn text(value: Option<&Value>) -> Option<&str> {
    value
        .and_then(Value::as_str)
        .filter(|text| !js_blank(text) && js_length(text) <= 1024)
}

fn rank(class: &str) -> Option<usize> {
    CLASSES.iter().position(|item| *item == class)
}

fn object<'a>(value: &'a Value, allowed: &[&str]) -> Option<&'a Map<String, Value>> {
    value
        .as_object()
        .filter(|map| map.keys().all(|key| allowed.contains(&key.as_str())))
}

/// Absent and `null` both mean the default, as `??` does.
fn present(value: Option<&Value>) -> Option<&Value> {
    value.filter(|value| !value.is_null())
}

struct Attempt<'a> {
    id: &'a str,
    profile: &'a str,
    task: &'a str,
    revision: &'a str,
    outcome: &'a str,
    evidence: &'a str,
}

struct Request<'a> {
    host: &'a str,
    role: &'a str,
    task: &'a str,
    revision: &'a str,
    capability_class: Option<&'a str>,
    current_profile: Option<&'a str>,
    required: Vec<&'a str>,
    attempts: Vec<Attempt<'a>>,
}

fn attempt(value: &Value) -> Result<Attempt<'_>> {
    let invalid = || "attempt requires identity, profile, task, revision, outcome and evidence";
    let map = object(
        value,
        &["id", "profile", "task", "revision", "outcome", "evidence"],
    )
    .ok_or_else(invalid)?;
    let field = |name| text(map.get(name)).ok_or_else(invalid);
    let outcome = map
        .get("outcome")
        .and_then(Value::as_str)
        .filter(|outcome| OUTCOMES.contains(outcome))
        .ok_or_else(invalid)?;
    Ok(Attempt {
        id: field("id")?,
        profile: field("profile")?,
        task: field("task")?,
        revision: field("revision")?,
        outcome,
        evidence: field("evidence")?,
    })
}

fn request(input: &Value) -> Result<Request<'_>> {
    let map = object(
        input,
        &[
            "host",
            "role",
            "task",
            "revision",
            "requiredCapabilities",
            "attempts",
            "capabilityClass",
            "currentProfile",
        ],
    )
    .ok_or("invalid model advice input")?;
    let field = |name| text(map.get(name)).ok_or(format!("model advice {name} required"));
    let (host, task, revision) = (field("host")?, field("task")?, field("revision")?);
    let role = map
        .get("role")
        .and_then(Value::as_str)
        .filter(|role| ROLES.contains(role))
        .ok_or("unknown model advice role")?;
    let capability_class = match map.get("capabilityClass") {
        None => None,
        Some(class) => Some(
            class
                .as_str()
                .filter(|class| rank(class).is_some())
                .ok_or("invalid capability class")?,
        ),
    };
    let current_profile = match map.get("currentProfile") {
        None => None,
        Some(profile) => Some(text(Some(profile)).ok_or("invalid current profile")?),
    };
    let required = match present(map.get("requiredCapabilities")) {
        None => Vec::new(),
        Some(value) => value
            .as_array()
            .filter(|items| items.len() <= 32)
            .and_then(|items| {
                items
                    .iter()
                    .map(|item| item.as_str().filter(|item| kebab(item)))
                    .collect::<Option<Vec<_>>>()
            })
            .ok_or("invalid requiredCapabilities")?,
    };
    let attempts = match present(map.get("attempts")) {
        None => Vec::new(),
        Some(value) => value
            .as_array()
            .filter(|items| items.len() <= 80)
            .ok_or("attempts must be a bounded array")?
            .iter()
            .map(attempt)
            .collect::<Result<Vec<_>>>()?,
    };
    for (index, item) in attempts.iter().enumerate() {
        if attempts[..index].iter().any(|other| other.id == item.id) {
            return Err("attempt IDs must be unique".into());
        }
    }
    Ok(Request {
        host,
        role,
        task,
        revision,
        capability_class,
        current_profile,
        required,
        attempts,
    })
}

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

/// Recommend a configured logical profile for `input.role` over validated `settings`.
pub fn advise(settings: &Value, input: &Value) -> Result<Value> {
    let request = request(input)?;
    let applicable: Vec<&Attempt> = request
        .attempts
        .iter()
        .filter(|attempt| attempt.task == request.task && attempt.revision == request.revision)
        .collect();
    let mut result = json!({
        "formatVersion": 1,
        "host": request.host,
        "role": request.role,
        "task": request.task,
        "revision": request.revision,
        "requiredCapabilities": request.required,
        "ignoredAttempts": request.attempts.len() - applicable.len(),
        "evidencePolicy": "Capabilities are consumer declarations; cited attempt outcomes are not independently judged. Stale/other-task attempts do not justify escalation.",
        "authorization": "recommendation-only; no model switch, worker, retry or tool permission was executed",
    });
    let mut finish = |fields: Value| {
        for (key, value) in fields.as_object().expect("result fields").clone() {
            result[key] = value;
        }
        Ok(result.take())
    };
    if let Some(blocker) = applicable
        .iter()
        .find(|attempt| BLOCKERS.contains(&attempt.outcome))
    {
        return finish(json!({
            "decision": "stop",
            "profile": blocker.profile,
            "reasons": [format!("Resolve {}; changing models does not remove this boundary.", blocker.outcome)],
            "evidence": [blocker.evidence],
        }));
    }
    let execution = settings.get("execution");
    let profiles = entry(execution, "profiles");
    let rule = entry(entry(execution, "roles"), request.role);
    let Some(mut profile) = entry(rule, "profile").and_then(Value::as_str) else {
        return finish(json!({
            "decision": "unavailable",
            "reasons": ["Role has no configured logical profile."],
        }));
    };
    let default_attempts = settings
        .pointer("/efficiency/attempts/maxAttempts")
        .and_then(safe_integer)
        .unwrap_or(1);
    let limit = |definition: &Value| {
        definition
            .get("maxAttempts")
            .and_then(safe_integer)
            .unwrap_or(default_attempts)
    };
    let capabilities = |definition: &Value| -> Vec<String> {
        definition["capabilities"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|item| item.as_str().map(str::to_string))
            .collect()
    };
    let mut reasons: Vec<String> = Vec::new();
    for _ in 0..MAX_CHAIN {
        let Some(definition) = entry(profiles, profile) else {
            reasons.push("Profile capability metadata is undeclared.".into());
            return finish(
                json!({ "decision": "unavailable", "profile": profile, "reasons": reasons }),
            );
        };
        let evidence: Vec<&&Attempt> = applicable
            .iter()
            .filter(|attempt| attempt.profile == profile)
            .collect();
        let passed: Vec<&str> = evidence
            .iter()
            .filter(|attempt| attempt.outcome == "passed")
            .map(|attempt| attempt.evidence)
            .collect();
        if !passed.is_empty() {
            reasons
                .push("Current-target acceptance is recorded passed; no escalation needed.".into());
            return finish(
                json!({ "decision": "completed", "profile": profile, "reasons": reasons, "evidence": passed }),
            );
        }
        let class = definition.get("class").and_then(Value::as_str);
        if request.capability_class.is_some() && class.is_none() {
            reasons
                .push("Requested capability class has no declared profile class mapping.".into());
            return finish(
                json!({ "decision": "unavailable", "profile": profile, "reasons": reasons }),
            );
        }
        let class_missing = match (request.capability_class, class) {
            (Some(requested), Some(class)) => rank(class) < rank(requested),
            _ => false,
        };
        let declared = capabilities(definition);
        let missing: Vec<&str> = request
            .required
            .iter()
            .copied()
            .filter(|capability| !declared.iter().any(|item| item == capability))
            .collect();
        let failures = evidence
            .iter()
            .filter(|attempt| attempt.outcome == "quality-failure")
            .count();
        let exhausted = failures as i64 >= limit(definition);
        if class_missing || !missing.is_empty() || exhausted {
            reasons.push(if class_missing {
                format!(
                    "Profile {profile} is below the requested {} capability class.",
                    request.capability_class.unwrap_or_default()
                )
            } else if !missing.is_empty() {
                format!("Profile {profile} lacks declared capabilities: {}.", missing.join(", "))
            } else {
                let cited: Vec<&str> = evidence.iter().map(|attempt| attempt.evidence).collect();
                format!(
                    "Profile {profile} reached its configured quality-attempt limit; cited evidence: {}.",
                    cited.join(", ")
                )
            });
            match definition.get("escalateTo").and_then(Value::as_str) {
                Some(next) => {
                    profile = next;
                    continue;
                }
                None => {
                    reasons.push("No further escalation is configured.".into());
                    return finish(
                        json!({ "decision": "stop", "profile": profile, "reasons": reasons }),
                    );
                }
            }
        }
        let delegated = delegation(settings, request.role, request.host, Some(profile));
        let worker = delegated.is_some()
            && settings
                .pointer("/efficiency/delegation/mode")
                .and_then(Value::as_str)
                != Some("manual");
        let mapping = entry(entry(execution, "hosts"), request.host);
        let model = entry(entry(mapping, "models"), profile)
            .cloned()
            .unwrap_or(Value::Null);
        let switch = entry(mapping, "mainModelSwitch") == Some(&Value::Bool(true));
        let current_sufficient = request
            .current_profile
            .and_then(|name| entry(profiles, name).map(|current| (name, current)))
            .is_some_and(|(name, current)| {
                let declared = capabilities(current);
                let current_class = current.get("class").and_then(Value::as_str);
                let failures = applicable
                    .iter()
                    .filter(|attempt| {
                        attempt.profile == name && attempt.outcome == "quality-failure"
                    })
                    .count();
                request
                    .required
                    .iter()
                    .all(|capability| declared.iter().any(|item| item == capability))
                    && class.is_none_or(|class| {
                        current_class.is_some_and(|current| rank(current) >= rank(class))
                    })
                    && (failures as i64) < limit(current)
            });
        let limitation = if model.is_null() {
            Some("Exact host model mapping is unavailable; keep current execution.")
        } else if !switch {
            Some("Main-model switching is unsupported or unverified; keep current execution.")
        } else {
            None
        };
        let execution = match delegated {
            Some(mut delegated) if worker => {
                delegated["dispatchStatus"] = json!("requires-approval-and-fresh-capacity");
                delegated["approvalRequired"] = json!(true);
                delegated["requestedWorkerLimit"] = settings
                    .pointer("/efficiency/delegation/maxWorkers")
                    .cloned()
                    .unwrap_or(Value::Null);
                delegated
            }
            _ => {
                let mode = if entry(rule, "mode").and_then(Value::as_str) == Some("tool") {
                    "tool"
                } else {
                    "main"
                };
                json!({ "mode": mode, "role": request.role })
            }
        };
        if !worker {
            reasons.push("Continue in main; a worker requires supported role/model mapping, positive capacity and scoped authorization.".into());
        }
        reasons.extend(limitation.map(str::to_string));
        let comparison = match request.current_profile {
            None => "unknown",
            Some(current) if current == profile => "already-selected",
            Some(_) => {
                "configured-profile-recommended; prices and actual capability are not measured"
            }
        };
        return finish(json!({
            "decision": "recommend",
            "profile": profile,
            "execution": execution,
            "mainModel": {
                "profile": profile,
                "model": model,
                "evidence": "Consumer-declared model mapping/support; native switching and authorization must be verified separately.",
                "switchSupported": switch && !model.is_null(),
                "decision": if current_sufficient { "keep-current" } else { "retain-current-until-authorized" },
                "limitation": limitation,
            },
            "workerMappingAvailable": worker,
            "capabilityClass": class.unwrap_or("unknown"),
            "currentProfile": request.current_profile,
            "comparison": comparison,
            "attemptsRemaining": limit(definition) - evidence.len() as i64,
            "reasons": reasons,
        }));
    }
    reasons.push("Profile chain limit reached.".into());
    finish(json!({ "decision": "stop", "reasons": reasons }))
}

#[cfg(test)]
mod tests;
