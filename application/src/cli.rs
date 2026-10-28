//! `craft <command> [options]`: argument rules, rendering and exit codes of every command an
//! installed package runs through its `bin/craft` launcher.
mod options;
mod render;

use crate::Outcome;
use crate::actions::Kind;
use crate::failure::Failure;
use crate::project::{self, Settings, absolute, read_input};
use crate::review::Command as Review;
use crate::scratch::Action;
use crate::telemetry::observe;
use crate::telemetry::{Operation, Recorder, retained};
use craft_engine::session::views::{self, Reader};
use craft_engine::values::normalize_numbers;
use craft_engine::{models, routing, services, values};
use options::{Options, parse};
use render::{
    action_receipt, adopt_receipt, assign_receipt, attempt_receipt, integrate_receipt,
    plan_receipt, queue_receipt, receipt, release_receipt, render, session_view, task_receipt,
};
use serde_json::{Map, Value, json};
use std::path::{Path, PathBuf};

type Run = fn(&Options, &mut Recorder, &Operation) -> Result<Value, Failure>;

#[derive(Clone, Copy)]
enum Handler {
    Run(Run),
    Workspace(Action),
    /// Parses its own options and writes its own usage.
    Review(crate::review::Command),
    /// Streams its own output; only `main` runs it.
    Evidence,
}

/// How a successful result is written.
#[derive(Clone, Copy)]
enum Render {
    Json,
    /// One JSON line, whatever the detail requested.
    Line,
    /// A compact receipt unless `--json` or `--verbose` asks for the full result.
    Receipt(fn(&str, &Value) -> String),
}

struct Command {
    name: &'static str,
    handler: Handler,
    /// Accounts the installed package's guidance; the launcher names its root.
    bundled: bool,
    render: Render,
}

impl Command {
    const fn new(name: &'static str, handler: Handler) -> Self {
        Self {
            name,
            handler,
            bundled: false,
            render: Render::Json,
        }
    }
    const fn run(name: &'static str, run: Run) -> Self {
        Self::new(name, Handler::Run(run))
    }
    const fn bundled(mut self) -> Self {
        self.bundled = true;
        self
    }
    const fn receipt(mut self, receipt: fn(&str, &Value) -> String) -> Self {
        self.render = Render::Receipt(receipt);
        self
    }
}

/// Every `craft <command>`, in the order the general help lists them.
const COMMANDS: &[Command] = &[
    Command::run("host-setup", host_setup),
    Command::run("permissions", permission_suggestions),
    Command::run("host-observe", host_observe),
    Command::new("workspace-open", Handler::Workspace(Action::Open)),
    Command::new("workspace-write", Handler::Workspace(Action::Write)),
    Command::new("workspace-promote", Handler::Workspace(Action::Promote)),
    Command::new("workspace-release", Handler::Workspace(Action::Release)),
    Command::new("workspace-clean", Handler::Workspace(Action::Maintain)),
    Command::run("state-clean", state_clean),
    Command::run("grounded-context", grounded_context),
    Command::run("worker-status", worker_status),
    Command::run("task-feature", |o, r, p| task_features(true, o, r, p)),
    Command::run("task-features", |o, r, p| task_features(false, o, r, p)),
    Command::run("telemetry-capture", telemetry_capture),
    Command::run("telemetry-forget", telemetry_forget),
    Command::run("telemetry-report", telemetry_report),
    Command::run("task-offer", |o, r, p| task_action(Kind::Offer, o, r, p))
        .bundled()
        .receipt(action_receipt),
    Command::run("task-decision", |o, r, p| {
        task_action(Kind::Decision, o, r, p)
    })
    .receipt(action_receipt),
    Command::run("task-action-check", |o, r, p| {
        task_action(Kind::Check, o, r, p)
    })
    .bundled()
    .receipt(action_receipt),
    Command::run("task-start", task_start).receipt(|_, value| task_receipt(value)),
    Command::run("task-adopt", task_adopt).receipt(|_, value| adopt_receipt(value)),
    Command::run("task-step", task_step)
        .bundled()
        .receipt(action_receipt),
    Command::run("plan", plan_show).receipt(|_, value| plan_receipt(value)),
    Command::run("plan-create", plan_create).receipt(|_, value| plan_receipt(value)),
    Command::run("plan-update", plan_update).receipt(|_, value| plan_receipt(value)),
    Command::run("plan-envelope", plan_envelope),
    Command::run("plan-next", plan_next).receipt(|_, value| queue_receipt(value)),
    Command::run("plan-release", plan_release).receipt(|_, value| release_receipt(value)),
    Command::run("plan-assign", plan_assign).receipt(|_, value| assign_receipt(value)),
    Command::run("plan-integrate", plan_integrate).receipt(|_, value| integrate_receipt(value)),
    Command::run("settings", settings),
    Command::run("route", route),
    Command::run("context-report", context_report).bundled(),
    Command::run("session", session_status)
        .bundled()
        .receipt(|_, value| session_view(value)),
    Command::run("session-advice", session_status)
        .bundled()
        .receipt(|_, value| session_view(value)),
    Command::run("session-attempt", session_attempt).receipt(|_, value| attempt_receipt(value)),
    Command::run("model-advice", model_advice),
    Command::run("lookup", lookup),
    Command {
        render: Render::Line,
        ..Command::run("locate", locate)
    },
    Command::run("prepare-services", prepare_services),
    Command::run("operation", plan_operation),
    Command::run("checkpoint-save", checkpoint_save).receipt(|_, value| receipt(value)),
    Command::run("checkpoint-read", checkpoint_read),
    Command::new("review-prepare", Handler::Review(Review::Prepare)),
    Command::new("review-check", Handler::Review(Review::Check)),
    Command::new("review-publication", Handler::Review(Review::Publication)),
    Command::new("review-followup", Handler::Review(Review::Followup)),
    Command::run("acceptance-view", acceptance_view),
    Command::new("project-evidence", Handler::Evidence),
];

fn command(name: &str) -> Option<&'static Command> {
    COMMANDS.iter().find(|command| command.name == name)
}

fn root(options: &Options) -> PathBuf {
    absolute(options.get("root").unwrap_or("."))
}

fn setup_bundle(options: &Options) -> Result<PathBuf, Failure> {
    let bundles = strings(options, "bundle");
    match bundles.as_slice() {
        [bundle] => Ok(absolute(bundle)),
        [] => std::env::var(crate::statusline::PACKAGE_ROOT)
            .ok()
            .filter(|s| !s.is_empty())
            .map(|s| absolute(&s))
            .ok_or_else(|| {
                "setup requires the installed launcher or one --bundle package-root".into()
            }),
        _ => Err("setup requires exactly one installed package root".into()),
    }
}

fn host_setup(
    options: &Options,
    recorder: &mut Recorder,
    parent: &Operation,
) -> Result<Value, Failure> {
    let settings = loaded(options, recorder, parent)?;
    let mut report = crate::setup::run(
        &root(options),
        options.get("host").ok_or("host-setup requires --host")?,
        options
            .get("package")
            .ok_or("host-setup requires --package")?,
        &setup_bundle(options)?,
        options.values.contains_key("apply"),
    )?;
    report["configuration"][0]["effective"] = settings["efficiency"]["context"].clone();
    if let Some(session) = options.get("session") {
        match crate::host_context::read(&root(options), session) {
            Ok(mut saved) if saved["host"] == report["host"] => {
                saved.as_object_mut().unwrap().remove("lastEvent");
                report["observation"] = saved;
            }
            Ok(_) => {}
            Err(error) => report["observationDiagnostic"] = json!(error.message),
        }
    }
    Ok(report)
}

fn permission_suggestions(
    options: &Options,
    _: &mut Recorder,
    _: &Operation,
) -> Result<Value, Failure> {
    if options.values.contains_key("apply") {
        return Err(
            "permissions prints suggestions only; review and apply host rules yourself".into(),
        );
    }
    crate::setup::permissions(
        options.get("host").ok_or("permissions requires --host")?,
        &setup_bundle(options)?,
    )
}

fn host_observe(
    options: &Options,
    recorder: &mut Recorder,
    operation: &Operation,
) -> Result<Value, Failure> {
    let payload = read_input(
        recorder,
        operation,
        options
            .get("data")
            .ok_or("host-observe requires --data native-event.json")?,
    )?;
    crate::host_context::record(
        &root(options),
        options.get("host").ok_or("host-observe requires --host")?,
        options
            .get("session")
            .ok_or("host-observe requires --session")?,
        &payload,
        options
            .get("observed-at")
            .ok_or("host-observe requires --observed-at transport-receive-time")?,
    )
}

fn grounded_context(
    options: &Options,
    recorder: &mut Recorder,
    parent: &Operation,
) -> Result<Value, Failure> {
    let Some(file) = options.get("data") else {
        return Err("grounded-context requires --data input.json".into());
    };
    let data = read_input(recorder, parent, file)?;
    crate::grounding::run(
        &root(options),
        &data,
        options.get("preferences").map(Path::new),
        recorder,
        parent,
    )
}

fn loaded(
    options: &Options,
    recorder: &mut Recorder,
    parent: &Operation,
) -> Result<Value, Failure> {
    let preferences = options.get("preferences").map(Path::new);
    project::loaded(recorder, parent, &root(options), preferences)
}

fn plan_operation(
    options: &Options,
    recorder: &mut Recorder,
    parent: &Operation,
) -> Result<Value, Failure> {
    let settings = loaded(options, recorder, parent)?;
    let mut params = match options.get("params") {
        Some(text) => {
            serde_json::from_str(text).map_err(|e| format!("invalid --params JSON: {e}"))?
        }
        None => json!({}),
    };
    values::normalize_numbers(&mut params);
    let name = options.get("operation");
    let summary = Map::from_iter(name.map(|name| ("operation".to_string(), json!(name))));
    observe(
        recorder,
        "integration.planned",
        parent,
        "INTEGRATION_UNAVAILABLE",
        summary,
        |_, _| {
            let name = name.ok_or("operation requires --operation <name>")?;
            Ok(services::plan(&settings, name, &params)?)
        },
    )
}

fn prepare_services(
    options: &Options,
    recorder: &mut Recorder,
    parent: &Operation,
) -> Result<Value, Failure> {
    let settings = loaded(options, recorder, parent)?;
    let commands = options.values.get("available-command");
    let tools = options.values.get("available-mcp-tool");
    crate::integration::prepared(&settings, commands, tools, recorder, parent)
}

fn model_advice(
    options: &Options,
    recorder: &mut Recorder,
    parent: &Operation,
) -> Result<Value, Failure> {
    let file = options
        .get("data")
        .ok_or("model-advice requires --data <request.json>")?;
    let request = read_input(recorder, parent, file)?;
    let settings = loaded(options, recorder, parent)?;
    let operation = recorder.start("model.advice", Some(parent));
    let result = models::advise(&settings, &request);
    let (attributes, failure) = match &result {
        Ok(advice) => {
            if advice["decision"] == "unavailable" {
                recorder.report("model", "MODEL_ADVICE_UNAVAILABLE", Map::new(), &operation);
            }
            let class = advice
                .get("capabilityClass")
                .or_else(|| request.get("capabilityClass"));
            let mut attributes =
                Map::from_iter([("decision".to_string(), advice["decision"].clone())]);
            if let Some(class) = class {
                attributes.insert("capabilityClass".into(), class.clone());
            }
            (attributes, None)
        }
        Err(_) => (Map::new(), Some("OPERATION_FAILED")),
    };
    recorder.complete(operation, attributes, failure);
    Ok(result?)
}

fn lookup(
    options: &Options,
    recorder: &mut Recorder,
    parent: &Operation,
) -> Result<Value, Failure> {
    let settings = loaded(options, recorder, parent)?;
    let files = options
        .values
        .get("file")
        .and_then(Value::as_array)
        .map_or(&[][..], Vec::as_slice);
    Ok(routing::lookup(&settings, files, options.get("module"))?)
}

fn strings(options: &Options, name: &str) -> Vec<String> {
    options
        .values
        .get(name)
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|item| item.as_str().map(str::to_string))
        .collect()
}

fn route_request<'a>(
    options: &'a Options,
    files: &'a [Value],
    risks: &'a [String],
) -> routing::Request<'a> {
    routing::Request {
        intent: options.get("intent"),
        files,
        risks,
        host: options.get("host").unwrap_or("agents"),
        delegate: options.values.contains_key("delegate"),
        task: options.get("task"),
        verification_mode: options.get("verification-mode"),
    }
}

fn files(options: &Options) -> &[Value] {
    options
        .values
        .get("file")
        .and_then(Value::as_array)
        .map_or(&[][..], Vec::as_slice)
}

/// The explicit `--task`, else the session's bound task. Advisory commands continue
/// without a task when the binding cannot be read.
fn advisory_task(options: &Options, recorder: &mut Recorder, parent: &Operation) -> Option<String> {
    if let Some(task) = options.get("task").filter(|t| !t.is_empty()) {
        return Some(task.to_string());
    }
    let session = options.get("session").filter(|s| !s.is_empty())?;
    match crate::session::active_task(&root(options), Some(session)) {
        Ok(task) => task,
        Err(_) => {
            recorder.report("state", "STATE_UNAVAILABLE", Map::new(), parent);
            None
        }
    }
}

/// Count `values` for the session's task, if both are known; a failure becomes
/// `activityDiagnostic` on `result`.
fn count_activity(
    options: &Options,
    task: Option<&str>,
    values: Map<String, Value>,
    result: &mut Value,
    recorder: &mut Recorder,
    parent: &Operation,
) {
    let session = options.get("session").filter(|s| !s.is_empty());
    let (Some(session), Some(task)) = (session, task) else {
        return;
    };
    if let Some(warning) =
        crate::session::add_activity(&root(options), session, task, &values, recorder, parent)
    {
        result["activityDiagnostic"] = json!(warning);
    }
}

fn route(options: &Options, recorder: &mut Recorder, parent: &Operation) -> Result<Value, Failure> {
    let task = advisory_task(options, recorder, parent);
    let settings = loaded(options, recorder, parent)?;
    let risks = strings(options, "risk");
    let request = routing::Request {
        task: task.as_deref(),
        ..route_request(options, files(options), &risks)
    };
    let mut plan = crate::guidance::route(&settings, &request, recorder, parent)?;
    let values = Map::from_iter([("routingDecisions".to_string(), json!(1))]);
    count_activity(
        options,
        task.as_deref(),
        values,
        &mut plan,
        recorder,
        parent,
    );
    Ok(plan)
}

/// Selected guidance accounting, counted as requested files and references for the task.
fn context_report(
    options: &Options,
    recorder: &mut Recorder,
    parent: &Operation,
) -> Result<Value, Failure> {
    let task = advisory_task(options, recorder, parent);
    let settings = loaded(options, recorder, parent)?;
    let risks = strings(options, "risk");
    // Guidance accounting neither delegates nor reads the task.
    let request = routing::Request {
        delegate: false,
        task: None,
        ..route_request(options, files(options), &risks)
    };
    let plan = crate::guidance::route(&settings, &request, recorder, parent)?;
    let budget = options.get("budget-bytes").map(crate::discovery::number);
    let mut report = crate::guidance::report(
        &plan,
        &strings(options, "bundle"),
        budget.as_ref(),
        recorder,
        parent,
    )?;
    let references = plan["references"].as_array().map_or(0, Vec::len);
    let values = Map::from_iter([
        (
            "filesRequested".to_string(),
            report["totals"]["files"].clone(),
        ),
        ("referencesRequested".to_string(), json!(references)),
    ]);
    count_activity(
        options,
        task.as_deref(),
        values,
        &mut report,
        recorder,
        parent,
    );
    Ok(report)
}

fn locate(
    options: &Options,
    recorder: &mut Recorder,
    parent: &Operation,
) -> Result<Value, Failure> {
    let task = advisory_task(options, recorder, parent);
    let session = options.get("session").filter(|s| !s.is_empty());
    let observed =
        crate::discovery::observation(&root(options), session, task.as_deref(), recorder, parent);
    let request = match options.get("data") {
        Some(file) => read_input(recorder, parent, file)?,
        None => {
            let mut request =
                json!({"terms":strings(options,"term"),"paths":strings(options,"path")});
            for (flag, field) in [
                ("max-files", "maxFiles"),
                ("max-matches", "maxMatches"),
                ("max-bytes", "maxBytes"),
                ("max-output-bytes", "maxOutputBytes"),
            ] {
                if let Some(value) = options.get(flag) {
                    request[field] = crate::discovery::number(value);
                }
            }
            request
        }
    };
    crate::discovery::run(
        &request,
        crate::discovery::Context {
            root: &root(options),
            session,
            task: task.as_deref(),
            preferences: options.get("preferences").map(Path::new),
            observed: &observed,
        },
        recorder,
        parent,
    )
}

/// The task a checkpoint describes: explicit, from the handoff, or the active binding.
fn checkpoint_save(
    options: &Options,
    recorder: &mut Recorder,
    parent: &Operation,
) -> Result<Value, Failure> {
    let root = root(options);
    let mut metadata = match options.get("data") {
        Some(file) => read_input(recorder, parent, file)?,
        None => json!({}),
    };
    let session = options.get("session").filter(|s| !s.is_empty());
    let mut task = options
        .get("task")
        .map(|t| json!(t))
        .or_else(|| metadata.get("taskId").filter(|t| !t.is_null()).cloned())
        .filter(|t| t.as_str() != Some(""));
    if task.is_none() && session.is_some() {
        match crate::session::active_task(&root, session) {
            Ok(active) => task = active.map(Value::String),
            Err(_) => recorder.report("state", "STATE_UNAVAILABLE", Map::new(), parent),
        }
    }
    if let Some(map) = metadata.as_object_mut() {
        if let Some(repositories) = options.values.get("repository") {
            map.insert("repositories".into(), repositories.clone());
        }
        if let Some(task) = &task {
            map.insert("taskId".into(), task.clone());
        }
    }
    let name = options.get("session").unwrap_or("manual");
    let mut saved = crate::checkpoint::save(&root, name, &metadata, None, recorder, parent)?;
    if let (Some(session), Some(task)) = (session, task.as_ref().and_then(Value::as_str)) {
        let values = Map::from_iter([("checkpoints".to_string(), json!(1))]);
        if let Some(warning) =
            crate::session::add_activity(&root, session, task, &values, recorder, parent)
        {
            saved["activityDiagnostic"] = json!(warning);
        }
    }
    Ok(saved)
}

/// A local attempt receipt for the explicit or bound task; nothing is retried or executed.
fn session_attempt(
    options: &Options,
    recorder: &mut Recorder,
    parent: &Operation,
) -> Result<Value, Failure> {
    let root = root(options);
    let session = options.get("session").filter(|s| !s.is_empty());
    let mut task = options
        .get("task")
        .filter(|t| !t.is_empty())
        .map(str::to_string);
    if task.is_none() && session.is_some() {
        task = crate::session::active_task(&root, session)?;
    }
    let (Some(file), Some(session), Some(task)) =
        (options.get("data").filter(|d| !d.is_empty()), session, task)
    else {
        return Err("session-attempt requires --session, --task and --data".into());
    };
    let attempt = read_input(recorder, parent, file)?;
    let operation = recorder.start("attempt.recorded", Some(parent));
    let result = loaded(options, recorder, &operation).and_then(|settings| {
        crate::session::record_attempt(
            &root, session, &task, &attempt, &settings, recorder, &operation,
        )
    });
    let (attributes, failure) = match &result {
        Ok(state) => (
            Map::from_iter([
                (
                    "recordedAttempts".to_string(),
                    state["activity"]["operationsAttempted"].clone(),
                ),
                ("reason".to_string(), json!("recorded")),
            ]),
            None,
        ),
        Err(error) => (Map::new(), Some(error.code("VALIDATION_FAILED"))),
    };
    recorder.complete(operation, attributes, failure);
    let state = result?;
    Ok(json!({
        "recorded": state["attempts"].as_array().and_then(|a| a.last()),
        "authorization": "local receipt only; no retry, model switch or command was executed",
    }))
}

/// Bind, route and snapshot one task; `--file`, `--risk`, `--stage` and `--repository` rescope
/// a repeated start, and omitted ones keep the saved values.
fn task_start(
    options: &Options,
    recorder: &mut Recorder,
    parent: &Operation,
) -> Result<Value, Failure> {
    let given = |name: &str| options.get(name).filter(|value| !value.is_empty());
    if options.values.contains_key("ready") {
        return ready_start(options, recorder, parent);
    }
    let (Some(session), Some(task)) = (given("session"), given("task")) else {
        return Err("task-start requires --session, --task and --goal (or --plan)".into());
    };
    // A plan task supplies its goal and, unless given here, its files and repositories.
    let planned = match given("plan") {
        Some(plan) => Some(crate::plan::task(&root(options), plan, task)?),
        None if options.values.contains_key("take-over") => {
            return Err("--take-over requires --plan".into());
        }
        None => None,
    };
    let goal = match (given("goal"), &planned) {
        (Some(goal), Some(planned)) if planned["goal"] != goal => {
            return Err(format!("task {task} of the plan has the goal {}", planned["goal"]).into());
        }
        (Some(goal), _) => goal.to_string(),
        (None, Some(planned)) => planned["goal"].as_str().unwrap_or_default().to_string(),
        (None, None) => {
            return Err("task-start requires --session, --task and --goal (or --plan)".into());
        }
    };
    let mut start = Map::from_iter([("goal".to_string(), json!(goal))]);
    if let Some(planned) = &planned {
        if let Some(files) = planned["files"].as_array().filter(|f| !f.is_empty()) {
            start.insert("files".into(), json!(files));
        }
        if let Some(repositories) = planned.get("repositories") {
            start.insert("repositories".into(), repositories.clone());
        }
        // A task in worktrees works there: its worktree folders are its repositories.
        if let Some(work) = planned.get("work") {
            start.insert("repositories".into(), crate::delegation::scopes(work));
        }
    }
    // An explicit --contract replaces the contract file the plan task names.
    let planned_contract = planned
        .as_ref()
        .and_then(|p| p["contract"].as_str())
        .map(|file| root(options).join(file).to_string_lossy().into_owned());
    if let Some(file) = given("contract").map(str::to_string).or(planned_contract) {
        start.insert(
            "engineeringContract".into(),
            read_input(recorder, parent, &file)?,
        );
    }
    if let Some(repositories) = options.values.get("repository") {
        start.insert("repositories".into(), repositories.clone());
    }
    if let Some(risks) = options.values.get("risk") {
        start.insert("risks".into(), risks.clone());
    }
    if let Some(authorization) = given("review-authorization") {
        start.insert("reviewAuthorization".into(), json!(authorization));
    }
    if let Some(stage) = given("stage") {
        start.insert("stage".into(), json!(stage));
    }
    if let Some(files) = options.values.get("file") {
        start.insert("files".into(), files.clone());
    }
    craft_engine::session::identity(&json!(session), &json!(task))?;
    let settings = loaded(options, recorder, parent)?;
    let request = crate::task::Start {
        root: &root(options),
        session,
        task,
        input: start,
        host: options.get("host"),
        settings: &settings,
        plan: given("plan").map(|id| crate::task::Planned {
            id,
            take_over: options.values.contains_key("take-over"),
        }),
    };
    crate::task::start(request, recorder, parent)
}

/// Times `--ready` picks again when another session claimed the picked task first.
const READY_PICKS: usize = 3;

/// `task-start --plan P --ready`: start the first ready task of the plan. The pick reads
/// the plan; the claim under the plans lock decides, so a pick another session won first
/// is followed by the next ready task.
fn ready_start(
    options: &Options,
    recorder: &mut Recorder,
    parent: &Operation,
) -> Result<Value, Failure> {
    let (Some(plan), Some(session)) = (options.get("plan"), options.get("session")) else {
        return Err("task-start --ready requires --plan and --session".into());
    };
    if ["task", "goal", "take-over"]
        .iter()
        .any(|name| options.values.contains_key(*name))
    {
        return Err(
            "task-start --ready picks the task; omit --task, --goal and --take-over".into(),
        );
    }
    let root = root(options);
    let own = crate::plan::owner(&root, session)?;
    let mut tried = Vec::new();
    loop {
        let task = crate::plan::next(&root, plan, session)?;
        if tried.contains(&task) {
            return Err(format!(
                "task {task} of plan {plan} could not be claimed; see plan-next --plan {plan}"
            )
            .into());
        }
        let mut picked = Options {
            values: options.values.clone(),
        };
        picked.values.shift_remove("ready");
        picked.values.insert("task".into(), json!(task));
        let outcome = task_start(&picked, recorder, parent);
        let holder = crate::plan::task(&root, plan, &task)?["owner"]["session"].clone();
        let lost = holder.is_string() && holder != own.as_str();
        if outcome.is_ok() || !lost || tried.len() + 1 == READY_PICKS {
            return outcome;
        }
        tried.push(task);
    }
}

/// The coordinator's view of one plan for `--session`.
fn plan_next(
    options: &Options,
    _recorder: &mut Recorder,
    _parent: &Operation,
) -> Result<Value, Failure> {
    let Some(plan) = options.get("plan") else {
        return Err("plan-next requires --plan".into());
    };
    crate::plan::queue(&root(options), plan, options.get("session"))
}

/// Return a held plan task to pending.
fn plan_release(
    options: &Options,
    _recorder: &mut Recorder,
    _parent: &Operation,
) -> Result<Value, Failure> {
    let (Some(plan), Some(task), Some(session)) = (
        options.get("plan"),
        options.get("task"),
        options.get("session"),
    ) else {
        return Err("plan-release requires --plan, --task and --session".into());
    };
    crate::plan::release(
        &root(options),
        plan,
        task,
        session,
        options.values.contains_key("take-over"),
        options.get("reason"),
    )
}

/// Delegate a ready plan task to a worker of `--session`, in isolated worktrees unless
/// `--shared`.
fn plan_assign(
    options: &Options,
    recorder: &mut Recorder,
    parent: &Operation,
) -> Result<Value, Failure> {
    let (Some(plan), Some(task), Some(session)) = (
        options.get("plan"),
        options.get("task"),
        options.get("session"),
    ) else {
        return Err("plan-assign requires --plan, --task and --session".into());
    };
    let settings = loaded(options, recorder, parent)?;
    crate::delegation::assign(crate::delegation::Assign {
        root: &root(options),
        plan,
        task,
        session,
        shared: options.values.contains_key("shared"),
        settings: &settings,
    })
}

/// Merge a completed worktree task, or `--escalate` or `--discard` its integration.
fn plan_integrate(
    options: &Options,
    _recorder: &mut Recorder,
    _parent: &Operation,
) -> Result<Value, Failure> {
    let (Some(plan), Some(task), Some(session)) = (
        options.get("plan"),
        options.get("task"),
        options.get("session"),
    ) else {
        return Err("plan-integrate requires --plan, --task and --session".into());
    };
    let how = match (
        options.values.contains_key("escalate"),
        options.values.contains_key("discard"),
    ) {
        (true, true) => return Err("plan-integrate takes --escalate or --discard, not both".into()),
        (true, false) => crate::delegation::Integration::Escalate(
            options
                .get("reason")
                .ok_or("plan-integrate --escalate requires --reason")?,
        ),
        (false, true) => crate::delegation::Integration::Discard,
        (false, false) => crate::delegation::Integration::Merge,
    };
    crate::delegation::integrate(&root(options), plan, task, session, how)
}

/// One plan as its Markdown view or JSON, the plans linking `--reference`, or all plans.
fn plan_show(
    options: &Options,
    _recorder: &mut Recorder,
    _parent: &Operation,
) -> Result<Value, Failure> {
    let references = strings(options, "reference");
    if references.len() > 1 {
        return Err("plan takes one --reference".into());
    }
    crate::plan::show(
        &root(options),
        options.get("plan"),
        references.first().map(String::as_str),
    )
}

/// Create a plan from `--plan`, `--goal`, `--reference` and optional `--data` tasks.
fn plan_create(
    options: &Options,
    recorder: &mut Recorder,
    parent: &Operation,
) -> Result<Value, Failure> {
    let mut input = match options.get("data") {
        Some(file) => read_input(recorder, parent, file)?,
        None => json!({}),
    };
    if !input.is_object() {
        return Err("plan-create --data must hold an object".into());
    }
    for (name, flag) in [("id", "plan"), ("goal", "goal")] {
        if let Some(value) = options.get(flag) {
            input[name] = json!(value);
        }
    }
    let references = strings(options, "reference");
    if !references.is_empty() {
        let mut listed = input["references"].as_array().cloned().unwrap_or_default();
        for reference in &references {
            listed.push(craft_engine::plan::reference(reference)?);
        }
        input["references"] = json!(listed);
    }
    if input.get("id").is_none() || input.get("goal").is_none() {
        return Err("plan-create requires --plan and --goal".into());
    }
    crate::plan::create(&root(options), &input)
}

/// The small context of one plan task, or with `--brief` the worker brief.
fn plan_envelope(
    options: &Options,
    _recorder: &mut Recorder,
    _parent: &Operation,
) -> Result<Value, Failure> {
    let (Some(plan), Some(task)) = (options.get("plan"), options.get("task")) else {
        return Err("plan-envelope requires --plan and --task".into());
    };
    crate::plan::envelope(
        &root(options),
        plan,
        task,
        options.values.contains_key("brief"),
    )
}

/// Apply `--data` changes to a plan still at `--expected-revision`.
fn plan_update(
    options: &Options,
    recorder: &mut Recorder,
    parent: &Operation,
) -> Result<Value, Failure> {
    let (Some(plan), Some(expected), Some(file)) = (
        options.get("plan"),
        options.get("expected-revision"),
        options.get("data"),
    ) else {
        return Err("plan-update requires --plan, --expected-revision and --data".into());
    };
    let changes = read_input(recorder, parent, file)?;
    crate::plan::update(
        &root(options),
        plan,
        expected,
        &changes,
        options.get("session"),
        options.values.contains_key("take-over"),
    )
}

/// Copy an earlier session's unfinished task into this session and bind it.
fn task_adopt(
    options: &Options,
    recorder: &mut Recorder,
    parent: &Operation,
) -> Result<Value, Failure> {
    let given = |name: &str| options.get(name).filter(|value| !value.is_empty());
    let (Some(session), Some(from)) = (given("session"), given("from")) else {
        return Err("task-adopt requires --session and --from".into());
    };
    crate::handoff::adopt(&root(options), session, from, recorder, parent)
}

/// The explicit `--task`, else the session's bound task; binding failures are errors.
fn bound_task(options: &Options) -> Result<Option<String>, Failure> {
    if let Some(task) = options.get("task") {
        return Ok(Some(task.to_string()));
    }
    crate::session::active_task(&root(options), options.get("session"))
}

fn required_bundles(options: &Options, command: &str) -> Result<Vec<String>, Failure> {
    let bundles = strings(options, "bundle");
    if bundles.is_empty() {
        return Err(format!("{command} requires --bundle with the installed package root").into());
    }
    Ok(bundles)
}

/// Record one agent-declared acceptance step for the bound task.
fn task_step(
    options: &Options,
    recorder: &mut Recorder,
    parent: &Operation,
) -> Result<Value, Failure> {
    let (Some(session), Some(task)) = (options.get("session"), bound_task(options)?) else {
        return Err(
            "task-step requires --session and an explicit --task or a task-start binding".into(),
        );
    };
    let mut input = Map::new();
    for (name, flag) in [
        ("stage", "stage"),
        ("target", "target"),
        ("approach", "approach"),
        ("evidence", "evidence"),
        ("outcome", "outcome"),
        ("result", "result"),
        ("nextStep", "next"),
    ] {
        if let Some(value) = options.get(flag) {
            input.insert(name.into(), json!(value));
        }
    }
    if let Some(file) = options.get("acceptance-evidence").filter(|f| !f.is_empty()) {
        input.insert(
            "acceptanceEvidence".into(),
            read_input(recorder, parent, file)?,
        );
        for (name, flag) in [
            ("expectedTaskContract", "expected-task-contract"),
            ("expectedEvidenceDigest", "expected-evidence-digest"),
        ] {
            if let Some(value) = options.get(flag) {
                input.insert(name.into(), json!(value));
            }
        }
    }
    if let Some(profile) = options.get("profile") {
        input.insert("profile".into(), json!(profile));
    }
    input.insert(
        "complete".into(),
        json!(options.values.get("complete") == Some(&json!(true))),
    );
    let bundles = required_bundles(options, "task-step")?;
    let request = crate::task::Step {
        root: &root(options),
        session,
        task: &task,
        input,
        host: options.get("host"),
        bundles: &bundles,
    };
    crate::task::step(
        request,
        &mut |recorder, parent| loaded(options, recorder, parent),
        recorder,
        parent,
    )
}

/// A reader's view of an engineering contract and its evidence ledger; reads files only.
fn acceptance_view(
    options: &Options,
    recorder: &mut Recorder,
    parent: &Operation,
) -> Result<Value, Failure> {
    let usage = || Failure::from(format!("usage: craft {}", ACCEPTANCE_VIEW_USAGE));
    let mut file = |name: &str| -> Result<Option<Value>, Failure> {
        options
            .get(name)
            .map(|file| {
                let mut value = read_input(recorder, parent, file)?;
                normalize_numbers(&mut value);
                Ok(value)
            })
            .transpose()
    };
    let contract = file("contract")?.ok_or_else(usage)?;
    let evidence = file("acceptance-evidence")?.ok_or_else(usage)?;
    let probes = file("probes")?;
    let focus = file("focus")?;
    let current = json!({
        "kind": options.get("current-kind").ok_or_else(usage)?,
        "identity": options.get("current-identity"),
    });
    let blockers = options.values.get("blockers") == Some(&json!(true));
    let view = match options.get("view").ok_or_else(usage)? {
        "qa" if focus.is_none() => views::qa(
            Some(&contract),
            Some(&evidence),
            &current,
            Some(probes.as_ref().unwrap_or(&json!([]))),
            blockers,
        ),
        view @ ("product" | "architecture") if probes.is_none() => views::reader(
            if view == "product" {
                Reader::Product
            } else {
                Reader::Architecture
            },
            Some(&contract),
            Some(&evidence),
            &current,
            focus.as_ref(),
            blockers,
        ),
        _ => return Err(usage()),
    };
    Ok(view?)
}

/// Propose, decide or check one recommended action for the bound task; `--data` supplies
/// fresh host facts.
fn task_action(
    kind: crate::actions::Kind,
    options: &Options,
    recorder: &mut Recorder,
    parent: &Operation,
) -> Result<Value, Failure> {
    use crate::actions::Kind;
    let (Some(session), Some(task)) = (options.get("session"), bound_task(options)?) else {
        return Err("action recommendation requires --session and a task-start binding".into());
    };
    let mut input = Map::new();
    let fields: &[(&str, &str)] = match kind {
        Kind::Offer => &[
            ("action", "action"),
            ("scope", "scope"),
            ("benefit", "benefit"),
            ("evidence", "evidence"),
            ("role", "role"),
            ("profile", "profile"),
        ],
        Kind::Decision => &[
            ("id", "offer"),
            ("decision", "decision"),
            ("evidence", "evidence"),
        ],
        Kind::Check => &[("id", "offer")],
    };
    for (name, flag) in fields {
        if let Some(value) = options.get(flag) {
            input.insert(name.to_string(), json!(value));
        }
    }
    if kind == Kind::Offer
        && let Some(prior) = options.get("prior-authorization").filter(|p| !p.is_empty())
    {
        input.insert("priorAuthorization".into(), json!(prior));
    }
    let facts = match options.get("data").filter(|d| !d.is_empty()) {
        Some(file) => read_input(recorder, parent, file)?,
        None => json!({}),
    };
    let bundles = if kind == Kind::Decision {
        Vec::new()
    } else {
        required_bundles(
            options,
            if kind == Kind::Offer {
                "task-offer"
            } else {
                "task-action-check"
            },
        )?
    };
    let request = crate::actions::Request {
        root: &root(options),
        session,
        task: &task,
        kind,
        input: &Value::Object(input),
        facts: &facts,
        bundles: &bundles,
    };
    crate::actions::run(
        request,
        &mut |recorder, parent| loaded(options, recorder, parent),
        recorder,
        parent,
    )
}

/// Feature outcomes for the explicit or bound task; `task-feature` first saves one declaration.
fn task_features(
    record: bool,
    options: &Options,
    recorder: &mut Recorder,
    parent: &Operation,
) -> Result<Value, Failure> {
    let (Some(session), Some(task)) = (options.get("session"), bound_task(options)?) else {
        return Err(
            "task outcome/worker view requires --session and a task binding or explicit --task"
                .into(),
        );
    };
    let mut input = Map::new();
    for (name, flag) in [
        ("feature", "feature"),
        ("status", "feature-status"),
        ("reason", "reason"),
        ("evidence", "evidence"),
    ] {
        if let Some(value) = options.get(flag) {
            input.insert(name.into(), json!(value));
        }
    }
    let input = Value::Object(input);
    crate::session::features(
        &root(options),
        session,
        &task,
        record.then_some(&input),
        recorder,
        parent,
    )
}

/// Worker evidence for the explicit or bound task, without dispatch.
fn worker_status(
    options: &Options,
    recorder: &mut Recorder,
    parent: &Operation,
) -> Result<Value, Failure> {
    let name = options.get("session").filter(|s| !s.is_empty()).ok_or(
        "task outcome/worker view requires --session and a task binding or explicit --task",
    )?;
    let task = match options.get("task").filter(|s| !s.is_empty()) {
        Some(t) => Some(t.to_string()),
        None => match crate::session::active_task(&root(options), Some(name)) {
            Ok(t) => t,
            Err(_) => {
                recorder.report("state", "STATE_UNAVAILABLE", Map::new(), parent);
                None
            }
        },
    }
    .ok_or("task outcome/worker view requires --session and a task binding or explicit --task")?;
    crate::workers::status(&root(options), name, &task, recorder, parent)
}

/// Session status and advice for the explicit or bound task; `--data` declares the rest.
fn session_status(
    options: &Options,
    recorder: &mut Recorder,
    parent: &Operation,
) -> Result<Value, Failure> {
    let data = match options.get("data") {
        Some(file) => read_input(recorder, parent, file)?,
        None => json!({}),
    };
    if !data.is_object() {
        return Err("session input must be a JSON object".into());
    }
    let settings = loaded(options, recorder, parent)?;
    let bundles = strings(options, "bundle");
    if bundles.is_empty() {
        return Err("session requires --bundle with the installed package root".into());
    }
    let request = crate::status::Request {
        root: &root(options),
        input: &data,
        settings: &settings,
        session: options.get("session"),
        task: options.get("task"),
        bundles: &bundles,
    };
    crate::status::run(request, recorder, parent)
}

/// One `workspace-*` action for the explicit or bound task: `--data` supplies the artifact
/// or promotion, `--operation` the owner scope.
fn workspace(
    action: crate::scratch::Action,
    options: &Options,
    recorder: &mut Recorder,
    parent: &Operation,
) -> Result<Value, Failure> {
    let mut data = match options.get("data").filter(|d| !d.is_empty()) {
        Some(file) => read_input(recorder, parent, file)?,
        None => json!({}),
    };
    if !data.is_object() {
        return Err("workspace input must be a JSON object".into());
    }
    if let Some(operation) = options.get("operation").filter(|o| !o.is_empty()) {
        data["operation"] = json!(operation);
    }
    let settings = loaded(options, recorder, parent)?;
    let request = crate::scratch::Request {
        root: &root(options),
        session: options.get("session"),
        task: options.get("task"),
        action,
        input: &data,
        settings: &settings,
    };
    crate::scratch::run(request, recorder, parent)
}

fn telemetry_report(options: &Options, _: &mut Recorder, _: &Operation) -> Result<Value, Failure> {
    let root = root(options);
    match (options.get("capture"), options.get("session")) {
        (Some(_), Some(_)) => Err("choose --capture or --session for telemetry-report".into()),
        (Some(capture), None) => retained::read(&root, capture),
        (None, session) => retained::report(&root, session),
    }
}

fn telemetry_capture(options: &Options, _: &mut Recorder, _: &Operation) -> Result<Value, Failure> {
    retained::capture(&root(options), options.get("session"))
}

fn telemetry_forget(options: &Options, _: &mut Recorder, _: &Operation) -> Result<Value, Failure> {
    retained::forget(&root(options), options.get("capture").unwrap_or_default())
}

fn state_clean(
    options: &Options,
    recorder: &mut Recorder,
    parent: &Operation,
) -> Result<Value, Failure> {
    let settings = loaded(options, recorder, parent)?;
    crate::retention::run(
        &root(options),
        &settings,
        options.values.contains_key("apply"),
    )
}

fn checkpoint_read(
    options: &Options,
    recorder: &mut Recorder,
    parent: &Operation,
) -> Result<Value, Failure> {
    let name = options.get("session").unwrap_or("manual");
    crate::checkpoint::recover(&root(options), name, recorder, parent)
}

fn settings(
    options: &Options,
    recorder: &mut Recorder,
    operation: &Operation,
) -> Result<Value, Failure> {
    let overrides = match options.get("data") {
        Some(file) => read_input(recorder, operation, file)?,
        None => json!({}),
    };
    let workers = match options.get("facts") {
        Some(file) => Some((
            read_input(recorder, operation, file)?,
            options.get("session"),
        )),
        None => None,
    };
    let preferences = options.get("preferences").map(PathBuf::from);
    let request = Settings {
        preferences: preferences.as_deref(),
        overrides,
        workers,
    };
    project::effective_settings(&root(options), request)
}

/// Command help; commands without an entry get the general list.
const ACCEPTANCE_VIEW_USAGE: &str = "acceptance-view --view qa|product|architecture --contract contract.json --acceptance-evidence ledger.json --current-kind repository|environment [--current-identity id] [--probes probes.json (qa)] [--focus focus.json (product, architecture)] [--blockers]";

fn help(action: &str) -> Value {
    let mut help = match action {
        "host-setup" => {
            json!({"usage":"host-setup --host name --package name [--bundle installed-package-root] [--root project] [--apply]", "mode":"Preview display, compaction and configuration guidance; --apply connects only the Claude display. Permissions are suggestions only."})
        }
        "permissions" => {
            json!({"usage":"permissions --host name [--bundle installed-package-root] [--root project]", "mode":"Print optional, command-scoped host permission suggestions. Writes no permission settings."})
        }
        "host-observe" => {
            json!({"usage":"host-observe --host claude|codex --session id --data native-event.json --observed-at receive-time [--root project]", "mode":"Record one fresh native context or compaction event forwarded by a connected host integration."})
        }
        "locate" => json!({
            "usage":"locate --root project --term Foo --term Bar [--path src] [--session id --task task-id] [--max-output-bytes 32768]",
            "data":"Alternatively --data request.json: terms or independent requests {id,term,dependsOn}, paths, maxFiles, maxMatches, maxBytes, maxOutputBytes",
            "mode":"bounded literal scan; optional local reuse receipts"
        }),
        "grounded-context" => {
            json!({"usage":"grounded-context --root project --data input.json", "mode":"Read-only citation composition; no task-state changes or command execution."})
        }
        "context-report" => json!({
            "usage": "context-report --intent implement [--file src/main.py] [--bundle package-root ...]",
            "mode": "selected guidance accounting; repeat --bundle for verified separately installed packages; no cache discovery or resource loading",
        }),
        "checkpoint-save" => json!({
            "usage": "checkpoint-save --root project --session id [--task task-id] [--repository relative/repo ...] --data handoff.json",
            "continuationFields": craft_engine::checkpoint::CONTINUATION,
            "mode": "explicit local checkpoint write",
        }),
        "checkpoint-read" => json!({
            "usage": "checkpoint-read --root project --session id",
            "mode": "read-only recovery comparison",
        }),
        "task-start" => json!({
            "usage": "task-start --session id --task stable-id (--goal \"outcome\" | --plan id [--take-over]) [--stage implement] [--file src/path] [--repository relative/repo ...] [--contract contract.json]",
            "mode": "bind task identity and optional selected repository roots within the workspace; route guidance and snapshot selected Git worktrees",
        }),
        "plan" => json!({
            "usage": "plan [--root project] [--plan id | --reference tracker:PROJ-123] [--json]",
            "mode": "read-only: one plan as its Markdown view (JSON with --json), the plans linking a reference, or every plan's summary",
        }),
        "plan-create" => json!({
            "usage": "plan-create --plan id --goal \"outcome\" [--reference tracker:PROJ-123 ...] [--data plan.json] [--root project]",
            "data": {"tasks": [{"id": "a", "goal": "First step", "dependsOn": [], "files": [], "acceptance": []}, {"id": "a1", "goal": "Part of the first step", "parent": "a", "scope": {"paths": ["src/part/**"]}, "contract": "contracts/part.json", "returns": "What changed"}], "decisions": [{"id": "d1", "text": "Decision"}]},
            "mode": "create a local work plan; references are opaque and nothing is read from a tracker",
        }),
        "plan-update" => json!({
            "usage": "plan-update --plan id --expected-revision N --data changes.json [--session id] [--take-over] [--root project]",
            "changes": ["goal", "status", "references", "decisions", "tasks"],
            "taskStates": ["pending", "failed", "abandoned"],
            "mode": "revision-fenced change; a task another session holds changes only with --take-over",
        }),
        "plan-envelope" => json!({
            "usage": "plan-envelope --plan id --task task-id [--brief] [--root project]",
            "mode": "read-only: the task's goal, acceptance, owned scope, dependency results, parent checkpoint reference and latest decisions; --brief emits the worker brief",
        }),
        "plan-assign" => json!({
            "usage": "plan-assign --plan id --task task-id --session id [--shared] [--root project]",
            "mode": "claim a ready task that owns paths for a worker session of this session, in isolated worktrees below .craft/worktrees/<plan>/<task>-<attempt>/ on branch craft/<plan>/<task>-<attempt> unless --shared; refused at efficiency.delegation.maxWorkers or when a running task owns intersecting paths; no worker is started",
        }),
        "plan-integrate" => json!({
            "usage": "plan-integrate --plan id --task task-id --session id [--escalate --reason text | --discard] [--root project]",
            "mode": "merge a completed worktree task into the coordinator checkout when it merges cleanly (one merge commit per repository, nothing pushed); on conflicts the task runs again in this session to resolve them; --escalate hands it to the user, --discard returns it to pending",
        }),
        "task-adopt" => json!({
            "usage": "task-adopt --session id --from earlier-session [--root project]",
            "mode": "copy an earlier session's unfinished task and checkpoint into this session and bind it; the earlier session is unchanged and no authorization is renewed",
        }),
        "session-attempt" => json!({
            "usage": "session-attempt --root project --session id --task task-id --data attempt.json",
            "example": {"operation": "implement", "target": "acceptance:regression", "approach": "bounded-fix", "evidence": "artifacts/check.json", "outcome": "passed"},
            "outcomes": craft_engine::session::OUTCOMES,
            "optional": ["profile", "maxAttempts", "escalationReason"],
            "mode": "receipt only; no retry execution",
        }),
        "workspace-open" => json!({
            "usage": "workspace-open --root project --session id [--task id] [--operation scope]",
            "mode": "lazily allocate bounded private scratch for the active task; no host temp/cache changes",
        }),
        "workspace-write" => json!({
            "usage": "workspace-write --session id [--operation scope] --data artifact.json",
            "mode": "write a new bounded scratch file from {name,content}; existing files are never overwritten",
        }),
        "workspace-promote" => json!({
            "usage": "workspace-promote --session id [--operation scope] --data promotion.json",
            "mode": "intentionally retain bounded evidence from {name,purpose}; never promote secrets or transcripts",
        }),
        "workspace-release" => json!({
            "usage": "workspace-release --session id [--operation scope]",
            "mode": "release the current owner operation; task completion cleans disposable scratch when no other owner is active",
        }),
        "workspace-clean" => json!({
            "usage": "workspace-clean --root project",
            "mode": "reclaim closed or stale storage; open operations are retained until stale",
        }),
        "state-clean" => json!({
            "usage": "state-clean --root project [--apply]",
            "mode": "preview session directories idle for 14 days with no lock or open workspace; --apply removes them; shared records are only reported",
        }),
        "task-offer" => json!({
            "usage": "task-offer --session id --action review|delegate|compact --scope scope --benefit benefit --evidence evidence-id [--data observations.json] [--prior-authorization evidence-id]",
            "mode": "evaluate and record a recommendation; no dispatch",
        }),
        "task-decision" => json!({
            "usage": "task-decision --session id --offer proposal-id --decision approved|declined --evidence user-decision-id",
            "mode": "record an explicit scoped user decision; no action execution",
        }),
        "task-feature" => json!({
            "usage": "task-feature --session id --feature feature-id --feature-status used|eligible-but-skipped|disabled|unsupported|not-applicable|failed --reason reason --evidence reference",
            "mode": "explicit evaluation classification only; normal delivery uses runtime observations and leaves missing evidence unknown",
        }),
        "worker-status" => {
            json!({"usage":"worker-status --root project --session id [--task id]", "mode":"shared task-tree reservations and host observations; no dispatch"})
        }
        "task-features" => json!({
            "usage": "task-features --session id [--task id]",
            "mode": "all feature outcomes, provenance, missing evidence and stale-source markers",
        }),
        "task-action-check" => json!({
            "usage": "task-action-check --session id --offer proposal-id --data fresh-observations.json",
            "mode": "revalidate source, approval, policy, support and capacity; no dispatch",
        }),
        "task-step" => json!({
            "usage": "task-step --session id --stage verify --target acceptance:id --approach bounded-check --evidence evidence-id --outcome passed --result \"observed result\" --next \"next step\" [--acceptance-evidence ledger.json --expected-task-contract digest --expected-evidence-digest digest] [--complete]",
            "mode": "agent-declared semantic receipt, bounded continuation and next-step advice; no action dispatch",
        }),
        "session" => json!({
            "usage": "session [--root project] [--session id --task task-id] [--data task.json] [--json]",
            "taskFields": ["id", "revision", "stage", "operation", "status", "files", "safeBoundary", "phaseChanged", "almostComplete", "compacted", "staleAssumptions", "risks", "nextStep"],
            "mode": "recommendation-only",
        }),
        "acceptance-view" => json!({
            "usage": ACCEPTANCE_VIEW_USAGE,
            "mode": "read-only reader view of supplied contract, evidence and probes; executes nothing and writes no task state",
        }),
        "telemetry-capture" => json!({
            "usage": "telemetry-capture --root project --session id",
            "mode": "explicit private immutable report retention; eight captures / 64 KiB; no automatic eviction",
        }),
        "telemetry-forget" => json!({
            "usage": "telemetry-forget --root project --capture id",
            "mode": "explicitly remove one retained report; ordinary CLI telemetry still applies",
        }),
        "telemetry-report" => json!({
            "usage": "telemetry-report [--root project] [--session id | --capture id]",
            "mode": "bounded retained observation counts, loss reports and storage limits; no semantic or native execution claims",
        }),
        "settings" => json!({
            "usage": "settings [--root project] [--preferences user.json] [--data task-overrides.json] [--session id --facts worker-observations.json]",
            "mode": "effective settings, value origins, applied controls and private state location; no action dispatch",
        }),
        _ => json!({
            "commands": COMMANDS.iter().map(|command| command.name).collect::<Vec<_>>(),
            "detail": "Use <command> --help or docs/SESSION.md for session policy.",
        }),
    };
    help["output"] = json!(
        "Routine receipts are compact by default; --json or --verbose returns the full structured result."
    );
    help
}

/// Run one migrated command. `argv` excludes the executable name.
pub fn run(argv: &[String]) -> Outcome {
    execute(argv, false)
}

/// `run` with every command's JSON result in place of its compact receipt.
pub fn run_structured(argv: &[String]) -> Outcome {
    execute(argv, true)
}

/// The `craft <command>` process: write the streams, then post the telemetry export.
/// Without `--bundle`, guidance is accounted against the package whose launcher ran it.
pub fn main(argv: &[String]) -> i32 {
    let named = argv.first().and_then(|name| command(name));
    if named.is_some_and(|command| matches!(command.handler, Handler::Evidence)) {
        return crate::evidence::main(&argv[1..]);
    }
    let mut argv = argv.to_vec();
    let package = std::env::var(crate::statusline::PACKAGE_ROOT).unwrap_or_default();
    if named.is_some_and(|command| command.bundled)
        && !argv.iter().any(|arg| arg == "--bundle")
        && !package.is_empty()
    {
        argv.extend(["--bundle".into(), package]);
    }
    let outcome = run(&argv);
    print!("{}", outcome.stdout);
    eprint!("{}", outcome.stderr);
    crate::telemetry::Recorder::deliver(outcome.export);
    outcome.exit_code
}

fn execute(argv: &[String], structured: bool) -> Outcome {
    let (action, args) = argv
        .split_first()
        .map_or(("", &[][..]), |(action, args)| (action.as_str(), args));
    let command = command(action);
    if let Some(Handler::Review(review)) = command.map(|command| command.handler) {
        return crate::review::run(review, args);
    }
    let command = command.filter(|command| !matches!(command.handler, Handler::Evidence));
    let asks = args.iter().any(|arg| arg == "--help");
    if asks || matches!(action, "help" | "--help") {
        let topic = if command.is_some() { action } else { "" };
        return Outcome {
            exit_code: 0,
            stdout: render(&help(topic)),
            stderr: String::new(),
            export: None,
        };
    }
    let hint = |flag: &str| {
        let index = args.iter().position(|arg| arg == flag)?;
        args.get(index + 1)
            .filter(|value| !value.starts_with("--"))
            .map(String::as_str)
    };
    let mut recorder = Recorder::open(
        &absolute(hint("--root").unwrap_or(".")),
        hint("--session"),
        hint("--host"),
    );
    let operation = recorder.start("cli.command", None);
    let Some(command) = command else {
        recorder.complete(operation, Map::new(), Some("INVALID_INPUT"));
        let names: Vec<_> = COMMANDS.iter().map(|command| command.name).collect();
        return Outcome {
            exit_code: 2,
            stdout: String::new(),
            stderr: format!("usage: craft <{}> [options]\n", names.join("|")),
            export: recorder.finish(),
        };
    };
    let result = parse(action, args).and_then(|options| match command.handler {
        Handler::Run(run) => run(&options, &mut recorder, &operation),
        Handler::Workspace(action) => workspace(action, &options, &mut recorder, &operation),
        Handler::Review(_) | Handler::Evidence => unreachable!("dispatched before parsing"),
    });
    let outcome = match &result {
        Ok(value) => {
            recorder.complete(
                operation,
                Map::from_iter([("command".to_string(), json!(action))]),
                None,
            );
            let compact =
                !structured && !args.iter().any(|arg| arg == "--json" || arg == "--verbose");
            let text = match command.render {
                Render::Receipt(receipt) if compact => receipt(action, value),
                Render::Line => format!("{value}\n"),
                _ => render(value),
            };
            (0, text, String::new())
        }
        Err(failure) => {
            recorder.complete(operation, Map::new(), Some(failure.code("INVALID_INPUT")));
            (2, String::new(), format!("{}\n", failure.message))
        }
    };
    Outcome {
        exit_code: outcome.0,
        stdout: outcome.1,
        stderr: outcome.2,
        export: recorder.finish(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_command_table_names_help_usage_and_dispatch() {
        let names: Vec<_> = COMMANDS.iter().map(|command| command.name).collect();
        let unique: std::collections::BTreeSet<_> = names.iter().collect();
        assert_eq!(unique.len(), names.len(), "command names are unique");
        assert_eq!(help("")["commands"], json!(names));
        let unknown = run(&["no-such-command".to_string()]);
        assert_eq!(unknown.exit_code, 2);
        assert_eq!(
            unknown.stderr,
            format!("usage: craft <{}> [options]\n", names.join("|"))
        );
        // Evidence streams through `main`; `run` cannot host it and treats it as unknown.
        assert_eq!(run(&["project-evidence".to_string()]).exit_code, 2);
    }
}
