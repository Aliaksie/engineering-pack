//! `task-offer`, `task-decision` and `task-action-check` for the bound task, under the active
//! binding lock: offers and decisions are recorded in task state; a check only reads it.
//! Nothing is dispatched, approved or granted.
use crate::cli::Failure;
use crate::session::{active_task, binding_file, read_state, update_state};
use crate::telemetry::{Operation, Recorder, now_ms};
use craft_engine::actions::{self, Context};
use craft_engine::session;
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
