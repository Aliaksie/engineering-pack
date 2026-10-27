//! `task-offer`, `task-decision` and `task-action-check` for the bound task, under the active
//! binding lock: offers and decisions are recorded in task state; a check only reads it.
//! Nothing is dispatched, approved or granted.
use crate::failure::Failure;
use crate::session::{active_task, binding_file, read_state, update_state};
use crate::telemetry::{Operation, Recorder, now_ms};
use craft_engine::actions::{self, Context};
use craft_engine::session;
use craft_engine::values::lower_hex;
use craft_platform::{repository::snapshot, state};
use serde_json::{Value, json};
use std::path::Path;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Offer,
    Decision,
    Check,
}

pub struct Request<'a> {
    pub root: &'a Path,
    pub session: &'a str,
    pub task: &'a str,
    pub kind: Kind,
    pub input: &'a Value,
    /// Fresh caller-declared host facts; `observations` is the status input.
    pub facts: &'a Value,
    pub bundles: &'a [String],
}

/// Effective settings, loaded once the binding is held.
pub type Load<'a> = &'a mut dyn FnMut(&mut Recorder, &Operation) -> Result<Value, Failure>;

pub fn run(
    request: Request,
    load: Load,
    recorder: &mut Recorder,
    parent: &Operation,
) -> Result<Value, Failure> {
    let Request {
        root,
        session: name,
        task,
        kind,
        input,
        facts,
        bundles,
    } = request;
    session::identity(&json!(name), &json!(task))?;
    let file = binding_file(root, name, true)?;
    let Some(_lock) = state::lock(&file)? else {
        return Err("active task writer is busy".into());
    };
    if active_task(root, Some(name))?.as_deref() != Some(task) {
        return Err("action decisions require the active task binding".into());
    }
    actions::facts(facts)?;
    let settings = &load(recorder, parent)?;
    let saved = read_state(root, name, task, recorder, parent)?;
    let repository = snapshot::workspace(root, saved["taskRecord"].get("repositories"), false)?;
    let none = json!({});
    let status = if kind == Kind::Decision {
        json!({})
    } else {
        let observed = crate::status::Request {
            root,
            input: facts
                .get("observations")
                .filter(|o| !o.is_null())
                .unwrap_or(&none),
            settings,
            session: Some(name),
            task: Some(task),
            bundles,
        };
        crate::status::run(observed, recorder, parent)?
    };
    let context = Context {
        settings,
        repository: &repository,
        status: &status,
        facts,
        session: Some(name),
        now: now_ms(),
    };
    let started = |state: &Value| -> Result<(), Failure> {
        if !state.get("taskRecord").is_some_and(|r| r.is_object()) {
            return Err("action recommendations require task-start".into());
        }
        Ok(())
    };
    if kind == Kind::Check {
        let state = read_state(root, name, task, recorder, parent)?;
        started(&state)?;
        return Ok(actions::check(&state, input, &context)?);
    }
    let mut result = Value::Null;
    update_state(root, name, task, recorder, parent, |state| {
        started(state)?;
        result = match kind {
            Kind::Offer => actions::propose(state, input, &context)?,
            _ => actions::decide(state, input, settings, &repository, context.now)?,
        };
        Ok(())
    })?;
    Ok(result)
}

fn execution_phase(
    input: &Value,
    root: &Path,
    recorder: &mut Recorder,
    parent: &Operation,
) -> Result<Value, Failure> {
    let name = input["session"]
        .as_str()
        .ok_or("action operation requires session")?;
    let task = input["task"]
        .as_str()
        .ok_or("action operation requires task")?;
    session::identity(&json!(name), &json!(task))?;
    if input["phase"] == "open" {
        return Ok(json!({}));
    }
    let id = &input["id"];
    let find = |state: &Value| -> Result<Value, Failure> {
        let id = id
            .as_str()
            .filter(|s| lower_hex(s, 64))
            .ok_or("invalid action proposal id")?;
        state["recommendations"]
            .get(id)
            .cloned()
            .ok_or_else(|| "unknown action proposal".into())
    };
    match input["phase"].as_str().unwrap_or_default() {
        "find" => find(&read_state(root, name, task, recorder, parent)?),
        "claim" => {
            let observation = &input["observation"];
            let now = now_ms();
            let observed = observation["observedAt"]
                .as_str()
                .and_then(craft_engine::timestamp::parse);
            if observation["sessionId"] != name
                || !observed.is_some_and(|at| at <= now && now - at <= 30000)
            {
                return Err("fresh matching action observations are required".into());
            }
            let facts = &observation["facts"];
            actions::facts(facts)?;
            let Some(_lock) = state::lock(&binding_file(root, name, true)?)? else {
                return Err("active task writer is busy".into());
            };
            if active_task(root, Some(name))?.as_deref() != Some(task) {
                return Err("action execution requires the active coordinator task".into());
            }
            let preferences = input["preferencesFile"].as_str().map(Path::new);
            let settings = crate::project::loaded(recorder, parent, root, preferences)?;
            let saved = read_state(root, name, task, recorder, parent)?;
            let offer = find(&saved)?;
            let repository =
                snapshot::workspace(root, saved["taskRecord"].get("repositories"), false)?;
            let bundles: Vec<String> = match input.get("bundle") {
                Some(Value::String(s)) => vec![s.clone()],
                Some(Value::Array(a)) => a
                    .iter()
                    .map(|v| v.as_str().map(str::to_owned).ok_or("invalid bundle root"))
                    .collect::<Result<_, _>>()?,
                _ => vec![],
            };
            let none = json!({});
            let status = crate::status::run(
                crate::status::Request {
                    root,
                    input: facts
                        .get("observations")
                        .filter(|v| !v.is_null())
                        .unwrap_or(&none),
                    settings: &settings,
                    session: Some(name),
                    task: Some(task),
                    bundles: &bundles,
                },
                recorder,
                parent,
            )?;
            let checkpoint = if offer["action"] == "compact" {
                let saved = crate::checkpoint::save(
                    root,
                    name,
                    &json!({"taskId":task}),
                    offer.get("source"),
                    recorder,
                    parent,
                )?;
                json!(craft_engine::digest(&saved["state"]))
            } else {
                Value::Null
            };
            let mut result = Value::Null;
            update_state(root, name, task, recorder, parent, |state| {
                result = actions::claim(
                    state,
                    &json!({"id":id}),
                    &Context {
                        settings: &settings,
                        repository: &repository,
                        status: &status,
                        facts,
                        session: Some(name),
                        now: now_ms(),
                    },
                    &checkpoint,
                )?;
                Ok(())
            })?;
            result.as_object_mut().unwrap().shift_remove("state");
            result["offer"] = offer;
            Ok(result)
        }
        "finish" => {
            let receipt = &input["receipt"];
            let mut result = Value::Null;
            update_state(root, name, task, recorder, parent, |state| {
                result = actions::finish(
                    state,
                    id,
                    &receipt["key"],
                    &receipt["generation"],
                    &input["result"],
                )?;
                if result["receipt"]["state"] == "completed" {
                    let action = &state["recommendations"][id.as_str().unwrap()]["action"];
                    craft_engine::session::record_feature(
                        state,
                        &json!({"feature":if action=="compact"{"compaction"}else{"review"},"status":"used","evidence":result["receipt"]["evidence"],"reason":"Host adapter reported the scoped action; delivery acceptance remains a separate task receipt."}),
                        &result["receipt"]["source"],
                        &craft_engine::timestamp::format(now_ms()),
                        "runtime-observed",
                    )?;
                }
                Ok(())
            })?;
            Ok(
                json!({"supported":true,"executed":result["receipt"]["state"]=="completed","uncertain":result["receipt"]["state"]=="uncertain","receipt":result["receipt"],"ignoredStaleResult":result["ignored"]}),
            )
        }
        _ => Err("unknown action phase".into()),
    }
}

/// Native durable phases around trusted asynchronous action callbacks.
pub fn execute(input: &Value) -> Result<Value, Failure> {
    let root = Path::new(
        input["root"]
            .as_str()
            .ok_or("action operation requires root")?,
    );
    let mut recorder = Recorder::open(root, input["session"].as_str(), None);
    let parent = recorder.start("host.session", None);
    let result = execution_phase(input, root, &mut recorder, &parent);
    let export = recorder.finish();
    Ok(match result {
        Ok(value) => json!({"value":value,"export":export}),
        Err(error) => json!({"error":error.message,"export":export}),
    })
}
