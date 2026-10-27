//! `task-start`: bind one task to the session for the current checkout, route its stage,
//! files and risks, and snapshot its selected source. Nothing is accepted, delegated or
//! switched; model advice is a recommendation.
use crate::cli::routed;
use crate::failure::Failure;
use crate::scratch::{self, Action};
use crate::session::{self as records, BINDING_BYTES};
use crate::telemetry::{Operation, Recorder};
use craft_engine::{actions, models, repository, routing, session};
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
            crate::checkpoint::save(root, name, &metadata, None, recorder, parent)
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

pub struct Step<'a> {
    pub root: &'a Path,
    pub session: &'a str,
    pub task: &'a str,
    /// The engine step input; the attempt budget is added here.
    pub input: Map<String, Value>,
    pub host: Option<&'a str>,
    pub bundles: &'a [String],
}

const BOUND: &str = "task-step requires the active task binding; use task-start to switch tasks";

/// Record one acceptance step for the bound task under the active binding lock, save its
/// continuation, observe status and offer a review the status recommends. Nothing is
/// executed or approved.
pub fn step(
    request: Step,
    load: crate::actions::Load,
    recorder: &mut Recorder,
    parent: &Operation,
) -> Result<Value, Failure> {
    let Step {
        root,
        session: name,
        task,
        mut input,
        host,
        bundles,
    } = request;
    session::identity(&json!(name), &json!(task))?;
    if records::active_task(root, Some(name))?.as_deref() != Some(task) {
        return Err(BOUND.into());
    }
    let file = records::binding_file(root, name, true)?;
    let Some(_lock) = state::lock(&file)? else {
        return Err("active task writer is busy".into());
    };
    if records::active_task(root, Some(name))?.as_deref() != Some(task) {
        return Err(BOUND.into());
    }
    let settings = &load(recorder, parent)?;
    let saved = records::read_state(root, name, task, recorder, parent)?;
    let repository = snapshot::workspace(root, saved["taskRecord"].get("repositories"), false)?;
    let profile = input.get("profile").and_then(Value::as_str);
    let limit = profile
        .and_then(|profile| settings["execution"]["profiles"][profile].get("maxAttempts"))
        .filter(|limit| !limit.is_null())
        .or_else(|| settings.pointer("/efficiency/attempts/maxAttempts"))
        .cloned()
        .unwrap_or(Value::Null);
    let mut bounded = Map::from_iter([("maxAttempts".to_string(), limit)]);
    bounded.append(&mut input);
    let input = Value::Object(bounded);
    let result = input["result"].clone();
    let id = craft_platform::telemetry::uuid()?;
    let at = craft_engine::timestamp::format(crate::telemetry::now_ms());
    let mut recorded = None;
    let updated = records::update_state(root, name, task, recorder, parent, |state| {
        recorded = Some(session::step::step(state, &input, &repository, &id, &at)?);
        Ok(())
    })?;
    let recorded = recorded.expect("recorded step");
    let checkpoint = if truthy(&settings["checkpoint"]["enabled"]) {
        (|| -> Result<Value, Failure> {
            let state = records::read_state(root, name, task, recorder, parent)?;
            let record = Some(&state["taskRecord"]).filter(|r| truthy(r));
            let metadata = json!({"taskId": task, "taskContract": session::contract(record)?, "continuation": recorded.continuation});
            crate::checkpoint::save(root, name, &metadata, Some(&recorded.receipt["source"]), recorder, parent)
        })()
        .unwrap_or_else(|failure| json!({"status": "unavailable", "reason": failure.message}))
    } else {
        json!({"status": "disabled"})
    };
    let declared = match host {
        Some(host) => json!({"host": {"name": host}}),
        None => json!({}),
    };
    let observed = crate::status::Request {
        root,
        input: &declared,
        settings,
        session: Some(name),
        task: Some(task),
        bundles,
    };
    let mut status = crate::status::run(observed, recorder, parent)
        .unwrap_or_else(|failure| json!({"diagnostic": failure.message}));
    let mut offer = None;
    if status["recommendation"]["action"] == "review"
        && settings["efficiency"]["review"]["mode"] != "manual"
    {
        let proposed = records::update_state(root, name, task, recorder, parent, |state| {
            let mut proposal = json!({
                "action": "review", "scope": "review:current-source",
                "benefit": "Review the verified changes for declared risks.", "evidence": recorded.receipt["evidence"],
            });
            if let Some(prior) = state["taskRecord"]
                .get("reviewAuthorization")
                .filter(|p| truthy(p))
            {
                proposal["priorAuthorization"] = prior.clone();
            }
            let facts = json!({"observations": {"task": status["task"]}});
            let context = actions::Context {
                settings,
                repository: &repository,
                status: &status,
                facts: &facts,
                session: Some(name),
                now: crate::telemetry::now_ms(),
            };
            offer = Some(actions::propose(state, &proposal, &context)?);
            Ok(())
        });
        if let Err(failure) = proposed {
            status["actionDiagnostic"] = json!(failure.message);
        }
    }
    if truthy(&status["notice"]) {
        let notice = status["notice"].clone();
        let written = records::update_state(root, name, task, recorder, parent, |state| {
            state["notice"] = notice;
            Ok(())
        });
        if let Err(failure) = written {
            status["noticeDiagnostic"] = json!(failure.message);
        }
    }
    let record = &updated["taskRecord"];
    let complete = record["goalComplete"] == true
        && (!truthy(&record["engineeringContract"]) || status["task"]["goalComplete"] == true);
    let boundary = scratch::Request {
        root,
        session: Some(name),
        task: Some(task),
        action: if complete {
            Action::Complete
        } else {
            Action::Touch
        },
        input: &json!({}),
        settings,
    };
    let workspace = scratch::run(boundary, recorder, parent).unwrap_or_else(|failure| {
        recorder.report("state", "STATE_UNAVAILABLE", Map::new(), parent);
        json!({"status": "retained", "reason": failure.message})
    });
    status["workspace"] = workspace.clone();
    let suppressed = offer.as_ref().is_some_and(|o| o["suppressed"] == true);
    let acceptance = match status.get("acceptance") {
        Some(acceptance) if !acceptance.is_null() => acceptance.clone(),
        _ => recorded.acceptance,
    };
    let mut response = json!({
        "session": name, "task": task, "recorded": recorded.receipt, "result": result, "complete": complete,
        "acceptance": acceptance, "workspace": workspace, "checkpoint": checkpoint,
    });
    if let Some(offer) = &offer {
        response["actionOffer"] = json!({"offer": offer["offer"], "suppressed": offer["suppressed"], "applicability": offer["applicability"], "executed": false});
    }
    response["nextStep"] = if suppressed {
        Value::Null
    } else {
        status.get("suggestion").cloned().unwrap_or(Value::Null)
    };
    response["status"] = status;
    response["authorization"] =
        json!("Agent-declared acceptance recorded locally; no recommended action was executed.");
    Ok(response)
}
