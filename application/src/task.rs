//! `task-start`: bind one task to the session for the current checkout, route its stage,
//! files and risks, and snapshot its selected source. Nothing is accepted, delegated or
//! switched; model advice is a recommendation.
use crate::cli::{Failure, routed};
use crate::scratch::{self, Action};
use crate::session::{self as records, BINDING_BYTES};
use crate::telemetry::{Operation, Recorder};
use craft_engine::{models, repository, routing, session};
use craft_platform::{repository::snapshot, state};
use serde_json::{Map, Value, json};
use std::path::Path;

/// Execution roles with model profiles; others keep the current execution.
const ADVISED_ROLES: &[&str] = &[
    "locator",
    "researcher",
    "implementer",
    "reviewer",
    "planner",
    "verifier",
];

pub struct Start<'a> {
    pub root: &'a Path,
    pub session: &'a str,
    pub task: &'a str,
    /// The engine start input without `id`.
    pub input: Map<String, Value>,
    pub host: Option<&'a str>,
    pub settings: &'a Value,
}

fn truthy(value: &Value) -> bool {
    !matches!(value, Value::Null | Value::Bool(false))
        && value.as_str() != Some("")
        && value.as_f64() != Some(0.0)
}

/// Bind and route the task under the active binding lock.
pub fn start(
    request: Start,
    recorder: &mut Recorder,
    parent: &Operation,
) -> Result<Value, Failure> {
    let Start {
        root,
        session: name,
        task,
        mut input,
        host,
        settings,
    } = request;
    input.insert("id".into(), json!(task));
    let input = Value::Object(input);
    let file = records::binding_file(root, name, true)?;
    let Some(_lock) = state::lock(&file)? else {
        return Err("active task writer is busy".into());
    };
    let previous = records::read_state(root, name, task, recorder, parent)?;
    let repositories = match input.get("repositories") {
        None => previous["taskRecord"].get("repositories").cloned(),
        Some(value) => Some(json!(repository::scopes(value)?)),
    };
    let repository = snapshot::workspace(root, repositories.as_ref(), false)?;
    let mut started = previous;
    let intent = session::start(&mut started, &input, &repository)?;
    let record = &started["taskRecord"];
    let files = record["files"].as_array().cloned().unwrap_or_default();
    let risks: Vec<String> = record["risks"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|risk| risk.as_str().map(str::to_string))
        .collect();
    let route = routing::Request {
        intent: Some(intent),
        files: &files,
        risks: &risks,
        host: host.unwrap_or("agents"),
        delegate: false,
        task: None,
        verification_mode: None,
    };
    let plan = routed(settings, &route, recorder, parent)?;
    let role = plan["execution"]["role"].as_str().unwrap_or_default();
    let model_advice = if truthy(&repository["head"]) && ADVISED_ROLES.contains(&role) {
        let advice = json!({
            "host": host.unwrap_or("unknown"), "role": role, "task": task,
            "revision": repository["head"], "capabilityClass": plan["capability"]["class"],
        });
        models::advise(settings, &advice)?
    } else {
        json!({"decision": "unavailable", "reasons": ["A supported role and source revision are required; retain current execution."]})
    };
    // Revalidate under the task-state lock; a concurrent step must not be overwritten.
    records::update_state(root, name, task, recorder, parent, |state| {
        session::start(state, &input, &repository)?;
        let routed = Map::from_iter([("routingDecisions".to_string(), json!(1))]);
        Ok(session::add_activity(state, &routed)?)
    })?;
    let mut binding = json!({"formatVersion": 1, "task": task});
    if let Some(checkout) = repository.get("checkout") {
        binding["checkout"] = checkout.clone();
    }
    if let Some(repositories) = repositories.as_ref().filter(|r| truthy(r)) {
        binding["repositories"] = repositories.clone();
    }
    state::write(&file, &binding, BINDING_BYTES, false)?;
    let current = records::read_state(root, name, task, recorder, parent)?;
    let context = session::context(&current, &repository)?;
    // Optional housekeeping must never prevent useful task work.
    let maintain = scratch::Request {
        root,
        session: Some(name),
        task: Some(task),
        action: Action::Maintain,
        input: &json!({}),
        settings,
    };
    let workspace = scratch::run(maintain, recorder, parent).unwrap_or_else(|failure| {
        recorder.report("state", "STATE_UNAVAILABLE", Map::new(), parent);
        json!({"status": "retained", "reason": failure.message})
    });
    let checkpoint = if truthy(&settings["checkpoint"]["enabled"]) {
        (|| -> Result<Value, Failure> {
            let state = records::read_state(root, name, task, recorder, parent)?;
            let record = Some(&state["taskRecord"]).filter(|r| truthy(r));
            let metadata = json!({"taskId": task, "taskContract": session::contract(record)?});
            crate::checkpoint::save(root, name, &metadata, recorder, parent)
        })()
        .unwrap_or_else(|failure| json!({"status": "unavailable", "reason": failure.message}))
    } else {
        json!({"status": "disabled"})
    };
    let mut result = json!({"session": name, "task": task});
    if let Some(repositories) = repositories.filter(truthy) {
        result["repositories"] = repositories;
    }
    for (key, value) in [
        ("routing", plan),
        ("modelAdvice", model_advice),
        ("acceptance", context["acceptance"].clone()),
        ("evidenceDigest", context["evidenceDigest"].clone()),
        ("taskContract", context["taskContract"].clone()),
        ("binding", context["binding"].clone()),
        ("checkpoint", checkpoint),
        ("workspace", workspace),
        (
            "authorization",
            json!(
                "Local task binding and snapshot only; no acceptance, worker, model switch or remote action was inferred."
            ),
        ),
    ] {
        result[key] = value;
    }
    Ok(result)
}
