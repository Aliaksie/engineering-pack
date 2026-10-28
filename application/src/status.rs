//! `session`/`session-advice`: one bounded observation of the bound task (its record, source,
//! checkpoint, routed guidance and workspace) and the engine's advice over it. Read-only:
//! nothing is compacted, reset, switched, written or executed.
use crate::failure::Failure;
use crate::scratch::{self, Action};
use crate::session::{active_task, read_state};
use crate::telemetry::{Operation, Recorder, now_ms, pack_version};
use craft_engine::session::status::{self, Observations};
use craft_engine::values::truthy;
use craft_engine::{checkpoint, routing, session};
use craft_platform::repository::snapshot;
use serde_json::{Map, Value, json};
use std::path::Path;

/// Task fields a saved record owns, including their absence; `--data` fills only the rest.
const RECORDED: &[&str] = &[
    "stage",
    "status",
    "goalComplete",
    "files",
    "revision",
    "verification",
    "verificationRevision",
    "safeBoundary",
    "nextStep",
    "risks",
    "reviewRequested",
];

pub struct Request<'a> {
    pub root: &'a Path,
    /// The caller's declarations: a JSON object.
    pub input: &'a Value,
    pub settings: &'a Value,
    pub session: Option<&'a str>,
    pub task: Option<&'a str>,
    /// Verified installed package roots for guidance accounting.
    pub bundles: &'a [String],
}

/// The session status, observed under `session.observed`.
pub fn run(
    request: Request,
    recorder: &mut Recorder,
    parent: &Operation,
) -> Result<Value, Failure> {
    let declared = request.input.get("sessionId").and_then(Value::as_str);
    recorder.enrich(
        request.session.or(declared),
        request.input["host"]["name"].as_str(),
    );
    let operation = recorder.start("session.observed", Some(parent));
    let result = observe(request, recorder, &operation);
    let (attributes, failure) = match &result {
        Ok(status) => {
            let mut attributes = Map::new();
            if let Some(action) = status["recommendation"].get("action") {
                attributes.insert("decision".into(), action.clone());
            }
            attributes.insert("pressure".into(), status["pressure"].clone());
            attributes.insert(
                "occupancyKnown".into(),
                json!(status["usage"]["status"] == "observed"),
            );
            (attributes, None)
        }
        Err(error) => (Map::new(), Some(error.code("OPERATION_FAILED"))),
    };
    recorder.complete(operation, attributes, failure);
    result
}

fn identity(value: Option<&Value>) -> Result<&str, Failure> {
    value
        .and_then(Value::as_str)
        .ok_or_else(|| "session activity requires bounded session and task identities".into())
}

/// An unavailable repository observation, as task context reads it.
fn unobserved() -> Value {
    json!({"gitAvailable": false, "head": null, "checkout": null, "fingerprints": {"complete": false, "index": null, "workingTree": null}})
}

fn observe(
    request: Request,
    recorder: &mut Recorder,
    operation: &Operation,
) -> Result<Value, Failure> {
    let Request {
        root,
        input,
        settings,
        session,
        task,
        bundles,
    } = request;
    let (session_id, mut task_id) = status::identities(input, session, task)?;
    let mut binding_error = None;
    if !truthy(task_id.as_ref()) && truthy(session_id.as_ref()) {
        match identity(session_id.as_ref()).and_then(|s| active_task(root, Some(s))) {
            Ok(bound) => task_id = bound.map(Value::String),
            Err(error) => {
                binding_error = Some(error.message);
                recorder.report("state", "STATE_UNAVAILABLE", Map::new(), operation);
            }
        }
    }
    let mut input = input.as_object().cloned().unwrap_or_default();
    if let Some(id) = session_id.as_ref().filter(|s| truthy(*s)) {
        input.insert("sessionId".into(), id.clone());
    }
    let mut declared = match input.get("task") {
        Some(Value::Object(task)) => task.clone(),
        _ => Map::new(),
    };
    if let Some(id) = task_id.as_ref().filter(|t| truthy(*t)) {
        declared.insert("id".into(), id.clone());
    }
    let bound = truthy(session_id.as_ref()) && truthy(task_id.as_ref());
    let names = || -> Result<(&str, &str), Failure> {
        Ok((identity(session_id.as_ref())?, identity(task_id.as_ref())?))
    };
    let mut observations = Observations {
        host_limits: input
            .get("host")
            .and_then(|host| host["name"].as_str())
            .and_then(crate::statusline::capabilities)
            .and_then(|host| host.get("contextLimits").cloned()),
        ..Observations::default()
    };
    if bound {
        match names().and_then(|(s, t)| read_state(root, s, t, recorder, operation)) {
            Ok(state) => observations.state = Some(state),
            Err(error) => {
                observations.state_error = Some(error.message);
                recorder.report("state", "STATE_UNAVAILABLE", Map::new(), operation);
            }
        }
    }
    let repositories = observations
        .state
        .as_ref()
        .and_then(|s| s["taskRecord"].get("repositories"))
        .filter(|r| !r.is_null())
        .cloned();
    let mut repository_error = None;
    let mut observe_repository =
        |recorder: &mut Recorder| match snapshot::workspace(root, repositories.as_ref(), false) {
            Ok(repository) => Some(repository),
            Err(error) => {
                repository_error = Some(error.to_string());
                recorder.report("state", "STATE_UNAVAILABLE", Map::new(), operation);
                None
            }
        };
    let mut context = None;
    if let Some(state) = observations
        .state
        .as_ref()
        .filter(|s| truthy(s.get("taskRecord")))
    {
        let repository = observe_repository(recorder).unwrap_or_else(unobserved);
        let observed = session::context(state, &repository)?;
        if truthy(state.get("notice")) && !input.contains_key("previous") {
            input.insert("previous".into(), state["notice"].clone());
        }
        declared.retain(|key, _| !RECORDED.contains(&key.as_str()));
        for (key, value) in observed["task"].as_object().into_iter().flatten() {
            declared.insert(key.clone(), value.clone());
        }
        observations.repository = Some(repository);
        context = Some(observed);
    }
    input.insert("task".into(), Value::Object(declared));
    let input = Value::Object(input);
    if truthy(session_id.as_ref()) {
        match identity(session_id.as_ref())
            .and_then(|s| crate::checkpoint::read(root, s, recorder, operation))
        {
            Ok(saved) => observations.saved = saved,
            Err(error) => {
                observations.checkpoint_error = Some(error.message);
                recorder.report(
                    "checkpoint",
                    "CHECKPOINT_UNAVAILABLE",
                    Map::new(),
                    operation,
                );
            }
        }
    }
    let needs = status::needs(
        &input,
        settings,
        observations.state.as_ref(),
        &observations.saved,
    );
    if needs.recover {
        let recovered = match &observations.repository {
            Some(current) => Ok(checkpoint::compare(&observations.saved, current)),
            None => identity(session_id.as_ref())
                .and_then(|s| crate::checkpoint::recover(root, s, recorder, operation)),
        };
        match recovered {
            Ok(recovered) => {
                observations.repository = recovered.get("current").cloned();
                observations.recovered = Some(recovered);
            }
            Err(error) => {
                observations.checkpoint_error = Some(error.message);
                recorder.report(
                    "checkpoint",
                    "CHECKPOINT_UNAVAILABLE",
                    Map::new(),
                    operation,
                );
            }
        }
    }
    if needs.repository && observations.repository.is_none() {
        observations.repository = observe_repository(recorder);
    }
    match needs.plan {
        Some(Err(error)) => {
            observations.pack_error = Some(error);
            recorder.report("context", "CONTEXT_UNAVAILABLE", Map::new(), operation);
        }
        Some(Ok(plan)) => {
            let known = routing::catalog::skills(routing::catalog::catalog()?);
            let mut skills: Vec<&Value> = Vec::new();
            for skill in plan["skills"].as_array().into_iter().flatten() {
                if !skills.contains(&skill) {
                    skills.push(skill);
                }
            }
            for skill in skills.into_iter().take(32) {
                recorder.observe(
                    "guidance.selected",
                    Map::from_iter([("skill".to_string(), skill.clone())]),
                    operation,
                    &known,
                );
            }
            match crate::guidance::report(&plan, bundles, None, recorder, operation) {
                Ok(report) => observations.pack = Some(report),
                Err(error) => {
                    observations.pack_error = Some(error.message);
                    recorder.report("context", "CONTEXT_UNAVAILABLE", Map::new(), operation);
                }
            }
            observations.plan = Some(plan);
        }
        None => {}
    }
    let mut result = status::status(status::Request {
        input: &input,
        settings,
        session,
        task,
        now: now_ms(),
        observations: &observations,
    })?;
    if let Some(error) = &repository_error {
        diagnostics(&mut result).push(json!(format!(
            "Repository observation unavailable: {error}"
        )));
    }
    if result["checkpoint"]["status"] == "stale-or-incomplete" {
        recorder.report("checkpoint", "CHECKPOINT_STALE", Map::new(), operation);
    }
    if result["usage"]["status"] != "observed" {
        recorder.report(
            "host",
            "HOST_OBSERVATION_UNAVAILABLE",
            Map::new(),
            operation,
        );
    }
    if result["recommendation"]["action"] == "return-control" {
        recorder.report("attempt", "ATTEMPT_BUDGET_EXHAUSTED", Map::new(), operation);
    }
    let mut advised = Map::new();
    for (name, value) in [
        ("capabilityClass", &result["capability"]["class"]),
        ("decision", &result["modelAdvice"]["decision"]),
    ] {
        if !value.is_null() {
            advised.insert(name.into(), value.clone());
        }
    }
    recorder.observe("model.advice", advised, operation, &[]);
    if result["modelAdvice"]["decision"] == "unavailable" {
        recorder.report("model", "MODEL_ADVICE_UNAVAILABLE", Map::new(), operation);
    }
    let activity = &result["activity"];
    let exhausted = matches!((activity["attempts"].as_f64(), activity["maxAttempts"].as_f64()), (Some(a), Some(m)) if a >= m);
    let mut advice = Map::new();
    if let Some(action) = result["recommendation"].get("action") {
        advice.insert("decision".into(), action.clone());
    }
    advice.insert(
        "reason".into(),
        json!(if exhausted {
            "budget-exhausted"
        } else {
            "observed"
        }),
    );
    recorder.observe("session.advice", advice, operation, &[]);
    let known = result["usage"]["status"] == "observed";
    let host = Map::from_iter([
        ("occupancyKnown".to_string(), json!(known)),
        (
            "reason".to_string(),
            json!(if known { "observed" } else { "unavailable" }),
        ),
    ]);
    recorder.observe("host.observed", host, operation, &[]);
    let features = match &observations.state {
        Some(state) => {
            let source = observations
                .repository
                .as_ref()
                .map_or(Value::Null, session::source);
            session::feature_report(state, &source)?
        }
        None => Value::Null,
    };
    let workspace = scratch::run(
        scratch::Request {
            root,
            session: session_id.as_ref().and_then(Value::as_str),
            task: task_id.as_ref().and_then(Value::as_str),
            action: Action::Status,
            input: &json!({}),
            settings,
        },
        recorder,
        operation,
    )
    .unwrap_or_else(|failure| {
        recorder.report("state", "STATE_UNAVAILABLE", Map::new(), operation);
        json!({"status": "retained", "reason": failure.message})
    });
    if let Some(context) = &context {
        for name in ["acceptance", "evidenceDigest", "taskContract", "binding"] {
            result[name] = context[name].clone();
        }
    }
    result["workspace"] = workspace;
    if let Some(repositories) = repositories.filter(|r| truthy(r)) {
        result["repositories"] = repositories;
    }
    if let Some(error) = binding_error {
        diagnostics(&mut result).push(json!(format!("Task binding unavailable: {error}")));
    }
    result["task"] = input["task"].clone();
    result["featureOutcomes"] = features;
    if context.as_ref().is_some_and(|c| c["stale"] == true) {
        result["taskEvidenceStale"] = json!(true);
    }
    result["packVersion"] = pack_version().map_or(Value::Null, Value::String);
    Ok(result)
}

fn diagnostics(result: &mut Value) -> &mut Vec<Value> {
    result["diagnostics"]
        .as_array_mut()
        .expect("status diagnostics")
}

/// Session status over declared `data` with the project's effective settings, for a caller
/// that parses its own arguments.
pub(crate) fn for_root(
    root: &Path,
    data: &Value,
    bundles: &[String],
    recorder: &mut Recorder,
    parent: &Operation,
) -> Result<Value, Failure> {
    let settings = crate::project::loaded(recorder, parent, root, None)?;
    let request = Request {
        root,
        input: data,
        settings: &settings,
        session: None,
        task: None,
        bundles,
    };
    run(request, recorder, parent)
}
