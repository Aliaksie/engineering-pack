//! Commands owned by the Rust runtime, with the argument rules, output and exit codes of
//! `cli/main.mjs`, which forwards these commands here.
use crate::project::{self, Settings};
use crate::telemetry::{Operation, Recorder};
use craft_engine::{context, contracts, models, routing, services};
use craft_platform::documents::DocumentError;
use craft_platform::guidance;
use serde_json::{Map, Value, json};
use std::io::ErrorKind;
use std::path::{Component, Path, PathBuf};

pub const COMMANDS: &[&str] = &[
    "grounded-context",
    "worker-status",
    "settings",
    "operation",
    "prepare-services",
    "model-advice",
    "lookup",
    "route",
    "locate",
    "context-report",
    "checkpoint-save",
    "checkpoint-read",
    "session-attempt",
    "task-start",
    "session",
    "session-advice",
    "task-step",
    "task-offer",
    "task-decision",
    "task-action-check",
    "task-feature",
    "task-features",
    "workspace-open",
    "workspace-write",
    "workspace-promote",
    "workspace-release",
    "workspace-clean",
];
/// Every command `cli/main.mjs` lists in its general help, migrated or not.
const ALL_COMMANDS: &[&str] = &[
    "workspace-open",
    "workspace-write",
    "workspace-promote",
    "workspace-release",
    "workspace-clean",
    "grounded-context",
    "worker-status",
    "task-feature",
    "task-features",
    "telemetry-capture",
    "telemetry-forget",
    "telemetry-report",
    "task-offer",
    "task-decision",
    "task-action-check",
    "task-start",
    "task-step",
    "settings",
    "route",
    "context-report",
    "session",
    "session-advice",
    "session-attempt",
    "model-advice",
    "lookup",
    "locate",
    "prepare-services",
    "operation",
    "checkpoint-save",
    "checkpoint-read",
];

/// Options every command accepts, as the shared JS parser does; each command reads its own.
const OPTIONS: &[&str] = &[
    "contract",
    "acceptance-evidence",
    "expected-evidence-digest",
    "expected-task-contract",
    "repository",
    "feature",
    "feature-status",
    "reason",
    "capture",
    "review-authorization",
    "action",
    "scope",
    "benefit",
    "offer",
    "decision",
    "role",
    "prior-authorization",
    "facts",
    "goal",
    "stage",
    "target",
    "approach",
    "evidence",
    "outcome",
    "result",
    "next",
    "profile",
    "complete",
    "preferences",
    "root",
    "intent",
    "file",
    "host",
    "delegate",
    "task",
    "risk",
    "operation",
    "params",
    "session",
    "data",
    "module",
    "term",
    "path",
    "max-files",
    "max-matches",
    "max-bytes",
    "available-command",
    "available-mcp-tool",
    "bundle",
    "budget-bytes",
    "verification-mode",
    "json",
    "verbose",
    "max-output-bytes",
];
const FLAGS: &[&str] = &["delegate", "json", "verbose", "complete"];
const REPEATED: &[&str] = &[
    "repository",
    "file",
    "risk",
    "term",
    "path",
    "available-command",
    "available-mcp-tool",
    "bundle",
];

#[derive(Debug)]
pub struct Failure {
    pub message: String,
    io: Option<ErrorKind>,
}

impl Failure {
    pub(crate) fn from_io(error: std::io::Error) -> Self {
        Self {
            message: error.to_string(),
            io: Some(error.kind()),
        }
    }
    pub(crate) fn from_error_code(code: &str) -> Self {
        let io = match code {
            "ENOENT" => Some(ErrorKind::NotFound),
            "EACCES" => Some(ErrorKind::PermissionDenied),
            "ENOTDIR" => Some(ErrorKind::NotADirectory),
            _ => None,
        };
        Self {
            message: code.into(),
            io,
        }
    }
    /// The stable reason code `observe` records for this failure.
    pub fn code(&self, fallback: &'static str) -> &'static str {
        match self.io {
            Some(ErrorKind::PermissionDenied) => "ACCESS_DENIED",
            Some(ErrorKind::StorageFull) => "STORAGE_FULL",
            Some(ErrorKind::NotFound) => "NOT_FOUND",
            Some(ErrorKind::ResourceBusy | ErrorKind::AlreadyExists) => "STATE_BUSY",
            _ => fallback,
        }
    }
}

impl From<String> for Failure {
    fn from(message: String) -> Self {
        Self { message, io: None }
    }
}

impl From<&str> for Failure {
    fn from(message: &str) -> Self {
        message.to_string().into()
    }
}

impl From<DocumentError> for Failure {
    fn from(error: DocumentError) -> Self {
        Self {
            io: error.io_kind(),
            message: error.to_string(),
        }
    }
}

pub struct Outcome {
    /// A portable Node-style I/O code for a caller-owned command telemetry envelope.
    pub error_code: Option<&'static str>,
    pub exit_code: i32,
    pub stdout: String,
    pub stderr: String,
    /// Remote-bound telemetry for the caller to post, when configured.
    pub export: Option<Value>,
}

#[derive(Default)]
struct Options {
    values: Map<String, Value>,
}

impl Options {
    fn get(&self, name: &str) -> Option<&str> {
        self.values.get(name).and_then(Value::as_str)
    }
}

/// `path.resolve`: absolute against the working directory, with `.` and `..` removed lexically.
pub(crate) fn resolve(path: &str) -> PathBuf {
    let joined = std::env::current_dir().unwrap_or_default().join(path);
    let mut resolved = PathBuf::new();
    for component in joined.components() {
        match component {
            Component::ParentDir => {
                resolved.pop();
            }
            Component::CurDir => {}
            other => resolved.push(other),
        }
    }
    resolved
}

fn parse(action: &str, args: &[String]) -> Result<Options, Failure> {
    let mut options = Options::default();
    let mut index = 0;
    while index < args.len() {
        let arg = &args[index];
        let name = arg
            .strip_prefix("--")
            .filter(|name| OPTIONS.contains(name))
            .ok_or_else(|| format!("unknown argument: {arg}"))?;
        index += 1;
        if FLAGS.contains(&name) {
            if options.values.insert(name.into(), json!(true)).is_some() {
                return Err(format!("duplicate option --{name}").into());
            }
            continue;
        }
        let value = args
            .get(index)
            .filter(|value| !value.starts_with("--"))
            .ok_or_else(|| format!("missing value for {arg}"))?;
        index += 1;
        if REPEATED.contains(&name) {
            options
                .values
                .entry(name)
                .or_insert_with(|| json!([]))
                .as_array_mut()
                .expect("repeated option")
                .push(json!(value));
        } else if options.values.insert(name.into(), json!(value)).is_some() {
            return Err(format!("duplicate option {arg}").into());
        }
    }
    let present = |name: &str| options.values.contains_key(name);
    if present("contract") && action != "task-start" {
        return Err("--contract is supported by task-start".into());
    }
    if [
        "acceptance-evidence",
        "expected-task-contract",
        "expected-evidence-digest",
    ]
    .iter()
    .any(|name| present(name))
        && action != "task-step"
    {
        return Err("structured evidence options require task-step".into());
    }
    if present("repository") && !["task-start", "checkpoint-save"].contains(&action) {
        return Err("--repository is supported by task-start and checkpoint-save; other commands reuse the saved scope".into());
    }
    Ok(options)
}

/// One child operation of `parent`: success records `summary`, failure the code of its
/// I/O error or `fallback`.
pub(crate) fn observe<T>(
    recorder: &mut Recorder,
    name: &'static str,
    parent: &Operation,
    fallback: &'static str,
    summary: Map<String, Value>,
    work: impl FnOnce(&mut Recorder, &Operation) -> Result<T, Failure>,
) -> Result<T, Failure> {
    let operation = recorder.start(name, Some(parent));
    let result = work(recorder, &operation);
    let (attributes, failure) = match &result {
        Ok(_) => (summary, None),
        Err(error) => (Map::new(), Some(error.code(fallback))),
    };
    recorder.complete(operation, attributes, failure);
    result
}

fn input(recorder: &mut Recorder, parent: &Operation, file: &str) -> Result<Value, Failure> {
    observe(
        recorder,
        "input.loaded",
        parent,
        "INVALID_INPUT",
        Map::new(),
        |_, _| project::input(Path::new(file)),
    )
}

fn root(options: &Options) -> PathBuf {
    resolve(options.get("root").unwrap_or("."))
}

fn grounded_context(
    options: &Options,
    recorder: &mut Recorder,
    parent: &Operation,
    forwarded: Option<&Value>,
) -> Result<Value, Failure> {
    let data = match (forwarded, options.get("data")) {
        (Some(data), _) => data.clone(),
        (None, Some(file)) => input(recorder, parent, file)?,
        _ => return Err("grounded-context requires --data input.json".into()),
    };
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
    observe(
        recorder,
        "project.loaded",
        parent,
        "CONFIG_INVALID",
        Map::new(),
        |_, _| project::settings(&root(options), options.get("preferences").map(Path::new)),
    )
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
    contracts::normalize_numbers(&mut params);
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
    prepared_services(&settings, commands, tools, recorder, parent)
}

/// Configured service metadata under `integration.prepared`, reporting operations whose
/// command or tool was not observed.
pub(crate) fn prepared_services(
    settings: &Value,
    commands: Option<&Value>,
    tools: Option<&Value>,
    recorder: &mut Recorder,
    parent: &Operation,
) -> Result<Value, Failure> {
    observe(
        recorder,
        "integration.prepared",
        parent,
        "INTEGRATION_UNAVAILABLE",
        Map::new(),
        |recorder, operation| {
            let result = services::prepare(settings, commands, tools)?;
            let unavailable = result["services"]
                .as_array()
                .into_iter()
                .flatten()
                .flat_map(|service| service["operations"].as_array().into_iter().flatten())
                .filter(|operation| operation["availability"] == "not-observed")
                .count();
            if unavailable > 0 {
                let attributes =
                    Map::from_iter([("unavailableCapabilities".to_string(), json!(unavailable))]);
                recorder.report(
                    "integration",
                    "INTEGRATION_UNAVAILABLE",
                    attributes,
                    operation,
                );
            }
            Ok(result)
        },
    )
}

fn model_advice(
    options: &Options,
    recorder: &mut Recorder,
    parent: &Operation,
) -> Result<Value, Failure> {
    let file = options
        .get("data")
        .ok_or("model-advice requires --data <request.json>")?;
    let request = input(recorder, parent, file)?;
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

/// The route with its `routing.completed` operation and `guidance.selected` observations.
pub(crate) fn routed(
    settings: &Value,
    request: &routing::Request,
    recorder: &mut Recorder,
    parent: &Operation,
) -> Result<Value, Failure> {
    let operation = recorder.start("routing.completed", Some(parent));
    let result = routing::route(settings, request);
    let (attributes, failure) = match &result {
        Ok(plan) => {
            let catalog = routing::catalog::catalog()?;
            let known = routing::catalog::skills(catalog);
            // Route skills are already unique.
            let skills = plan["skills"].as_array().map_or(&[][..], Vec::as_slice);
            for skill in skills.iter().take(32) {
                let attributes = Map::from_iter([("skill".to_string(), skill.clone())]);
                recorder.observe("guidance.selected", attributes, &operation, &known);
            }
            let attributes = Map::from_iter([
                ("intent".to_string(), plan["intent"].clone()),
                (
                    "capabilityClass".to_string(),
                    plan["capability"]["class"].clone(),
                ),
                ("selectedSkills".to_string(), json!(skills.len())),
            ]);
            (attributes, None)
        }
        Err(_) => (Map::new(), Some("OPERATION_FAILED")),
    };
    recorder.complete(operation, attributes, failure);
    Ok(result?)
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
    let mut plan = routed(&settings, &request, recorder, parent)?;
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
    let plan = routed(&settings, &request, recorder, parent)?;
    let budget = options.get("budget-bytes").map(crate::discovery::number);
    let mut report = guidance_report(
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

/// Selected guidance files for `plan` in the verified `bundles`, observed as `context.observed`.
pub(crate) fn guidance_report(
    plan: &Value,
    bundles: &[String],
    budget: Option<&Value>,
    recorder: &mut Recorder,
    parent: &Operation,
) -> Result<Value, Failure> {
    let operation = recorder.start("context.observed", Some(parent));
    let result = (|| -> Result<Value, Failure> {
        let requests = context::requests(plan)?;
        let (entries, missing) = guidance::measure(bundles, &requests)?;
        let budget = budget.map(context::budget).transpose()?;
        Ok(context::summary(entries, missing, budget))
    })();
    let (attributes, failure) = match &result {
        Ok(report) => {
            let totals = &report["totals"];
            let attributes = Map::from_iter([
                ("guidanceFiles".to_string(), totals["files"].clone()),
                ("guidanceBytes".to_string(), totals["bytes"].clone()),
                (
                    "estimatedTokens".to_string(),
                    totals["estimatedTokens"].clone(),
                ),
            ]);
            (attributes, None)
        }
        Err(error) => (Map::new(), Some(error.code("CONTEXT_UNAVAILABLE"))),
    };
    recorder.complete(operation, attributes, failure);
    result
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
        Some(file) => input(recorder, parent, file)?,
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
        Some(file) => input(recorder, parent, file)?,
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
    let attempt = input(recorder, parent, file)?;
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
    let (Some(session), Some(task), Some(goal)) = (given("session"), given("task"), given("goal"))
    else {
        return Err("task-start requires --session, --task and --goal".into());
    };
    let mut start = Map::from_iter([("goal".to_string(), json!(goal))]);
    if let Some(file) = given("contract") {
        start.insert("engineeringContract".into(), input(recorder, parent, file)?);
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
    };
    crate::task::start(request, recorder, parent)
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
            crate::cli::input(recorder, parent, file)?,
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
        Some(file) => crate::cli::input(recorder, parent, file)?,
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
    session_status_input(options, recorder, parent, None)
}

fn session_status_input(
    options: &Options,
    recorder: &mut Recorder,
    parent: &Operation,
    forwarded: Option<&Value>,
) -> Result<Value, Failure> {
    let data = match (forwarded, options.get("data")) {
        (Some(_), Some(_)) => return Err("choose forwarded session input or --data".into()),
        (Some(data), None) => data.clone(),
        (None, Some(file)) => input(recorder, parent, file)?,
        (None, None) => json!({}),
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
        Some(file) => input(recorder, parent, file)?,
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
        Some(file) => input(recorder, operation, file)?,
        None => json!({}),
    };
    let workers = match options.get("facts") {
        Some(file) => Some((input(recorder, operation, file)?, options.get("session"))),
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

/// Command help as `cli/main.mjs` gives it; commands without an entry get the general list.
fn help(action: &str) -> Value {
    let mut help = match action {
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
            "usage": "task-start --session id --task stable-id --goal \"outcome\" [--stage implement] [--file src/path] [--repository relative/repo ...] [--contract contract.json]",
            "mode": "bind task identity and optional selected repository roots within the workspace; route guidance and snapshot selected Git worktrees",
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
            "mode": "reclaim closed or stale abandoned storage; live or unverified owners are retained",
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
        "settings" => json!({
            "usage": "settings [--root project] [--preferences user.json] [--data task-overrides.json] [--session id --facts worker-observations.json]",
            "mode": "effective settings, value origins, applied controls and private state location; no action dispatch",
        }),
        _ => json!({
            "commands": ALL_COMMANDS,
            "detail": "Use <command> --help or docs/SESSION.md for session policy.",
        }),
    };
    help["output"] = json!(
        "Routine receipts are compact by default; --json or --verbose returns the full structured result."
    );
    help
}

fn render(value: &Value) -> String {
    format!(
        "{}\n",
        serde_json::to_string_pretty(value).expect("JSON values serialize")
    )
}

fn nonempty(value: &Value) -> Option<&str> {
    value.as_str().filter(|text| !text.is_empty())
}

/// A saved, unchanged, disabled or unavailable checkpoint in one line.
fn checkpoint_line(saved: &Value) -> String {
    if let Some(status) = nonempty(&saved["status"]) {
        return match nonempty(&saved["reason"]) {
            Some(reason) => format!("Checkpoint: {status} — {reason}"),
            None => format!("Checkpoint: {status}"),
        };
    }
    let incomplete = if saved["state"]["fingerprints"]["complete"] == false {
        " (repository inspection incomplete)"
    } else {
        ""
    };
    let status = if saved["unchanged"] == true {
        "unchanged"
    } else {
        "saved"
    };
    let mut text = format!("Checkpoint: {status}{incomplete}");
    if let Some(file) = nonempty(&saved["file"]) {
        text.push_str(&format!(" — {file}"));
    }
    text
}

fn joined(value: &Value) -> Option<String> {
    let items: Vec<&str> = value.as_array()?.iter().filter_map(Value::as_str).collect();
    (!items.is_empty()).then(|| items.join(", "))
}

/// `String(value)` for a status field; absent and `null` read as unknown.
fn shown(value: &Value) -> String {
    match value {
        Value::Null => "unknown".into(),
        Value::String(text) => text.clone(),
        Value::Number(number) => match number.as_f64() {
            Some(float) if float.fract() == 0.0 && float.abs() < 1e21 => {
                format!("{}", float as i64)
            }
            _ => number.to_string(),
        },
        other => other.to_string(),
    }
}

/// The compact session view: context, guidance, activity, checkpoint and the next action.
fn session_view(status: &Value) -> String {
    let usage = &status["usage"];
    let activity = &status["activity"];
    let pack = &status["packContext"];
    let suggestion = Some(&status["recommendation"])
        .filter(|s| !s.is_null())
        .unwrap_or(&status["suggestion"]);
    let observed = usage["status"] == "observed";
    let context = if observed {
        let percent = (usage["usedRatio"].as_f64().unwrap_or(0.0) * 100.0 + 0.5).floor();
        format!(
            "{}% ({}/{}); {}; {}",
            shown(&json!(percent)),
            shown(&usage["currentTokens"]),
            shown(&usage["capacityTokens"]),
            shown(&status["pressure"]),
            shown(&usage["source"])
        )
    } else {
        format!("unknown — {}", shown(&usage["reason"]))
    };
    let tokens = match &pack["estimatedTokens"] {
        Value::Null => "unknown".to_string(),
        tokens => format!("~{}", shown(tokens)),
    };
    let profile = nonempty(&status["modelAdvice"]["profile"])
        .map_or(String::new(), |p| format!("; configured profile {p}"));
    let reason = match suggestion["reason"].as_str() {
        Some(reason) => reason,
        None if usage["status"] == "unknown" => {
            "No reliable occupancy observation; continue useful work without inferring context pressure."
        }
        None => "Continue useful work; no context change is indicated.",
    };
    let or_unknown = |value: &Value| value.as_str().unwrap_or("unknown").to_string();
    let mut lines = vec![
        "Engineering Pack — Session".to_string(),
        format!("Pack version: {}", shown(&status["packVersion"])),
        format!(
            "Host: {} | Model: {}",
            shown(&status["host"]["name"]),
            shown(&status["host"]["model"])
        ),
        format!("Context: {context}"),
        format!(
            "Pack: {tokens} estimated tokens selected; {} skills / {} references",
            shown(&pack["skills"]),
            shown(&pack["references"])
        ),
        format!(
            "Task: {} | Attempts: {}/{}",
            shown(&status["operation"]),
            shown(&activity["attempts"]),
            shown(&activity["maxAttempts"])
        ),
        format!(
            "Discovery: {} reused / {} fresh / {} batches (pack activity)",
            shown(&activity["reuseHits"]),
            shown(&activity["freshLookups"]),
            shown(&activity["batchedOperations"])
        ),
        format!(
            "Capability: {} (current: {}){profile}",
            or_unknown(&status["capability"]["class"]),
            or_unknown(&status["capability"]["currentClass"])
        ),
        format!(
            "Checkpoint: {}",
            or_unknown(&status["checkpoint"]["status"])
        ),
        String::new(),
        format!(
            "Advice: {}",
            suggestion["action"].as_str().unwrap_or("continue")
        ),
        format!("Reason: {reason}"),
    ];
    let acceptance = &status["acceptance"];
    if acceptance.is_object() {
        lines.push(format!("Acceptance: {}", shown(&acceptance["assurance"])));
        for criterion in acceptance["criteria"].as_array().into_iter().flatten() {
            let optional = if criterion["required"] == true {
                ""
            } else {
                " (optional)"
            };
            lines.push(format!(
                "Criterion: {} — {}{optional}",
                shown(&criterion["id"]),
                shown(&criterion["status"])
            ));
        }
    }
    if let Some(contract) = nonempty(&status["taskContract"]) {
        lines.push(format!("Task contract: {contract}"));
    }
    if let Some(digest) = nonempty(&status["evidenceDigest"]) {
        lines.push(format!("Evidence digest: {digest}"));
    }
    if status["binding"].is_object() {
        lines.push(format!(
            "Source: {}",
            nonempty(&status["binding"]["identity"]).unwrap_or("unknown")
        ));
    }
    if let Some(advice) = nonempty(&status["capability"]["recommendation"]) {
        lines.push(format!("Model advice: {advice}"));
    }
    if let Some(repositories) = status["repositories"].as_array() {
        let names: Vec<String> = repositories.iter().map(shown).collect();
        lines.push(format!("Repositories: {}", names.join(", ")));
    }
    let workspace = &status["workspace"];
    if workspace["status"] == "available" {
        lines.push(format!(
            "Workspace: scratch {}; {} evidence artifacts; {} bytes",
            shown(&workspace["scratch"]),
            shown(&workspace["evidence"]["artifacts"]),
            shown(&workspace["diskBytes"])
        ));
    } else if workspace["status"] == "retained" {
        lines.push(format!(
            "Workspace: retained — {}",
            shown(&workspace["reason"])
        ));
    }
    if let Some(after) = nonempty(&suggestion["afterCheckpoint"]) {
        lines.push(format!("After checkpoint: {after}"));
    }
    if let Some(boundary) = nonempty(&suggestion["atBoundary"]) {
        lines.push(format!("At boundary: {boundary}"));
    }
    format!("{}\n", lines.join("\n"))
}

fn offer_lines(result: &Value) -> Vec<String> {
    let offer = &result["offer"];
    if !offer.is_object() {
        let reason = result["applicability"]["reason"]
            .as_str()
            .unwrap_or("No action proposed.");
        return vec![format!("Offer: unavailable — {reason}")];
    }
    let suppressed = if result["suppressed"] == true {
        " (unchanged; suppressed)"
    } else {
        ""
    };
    vec![
        format!("Offer: {}", shown(&offer["id"])),
        format!(
            "Action: {} | Scope: {}",
            shown(&offer["action"]),
            shown(&offer["scope"])
        ),
        format!("Decision: {}{suppressed}", shown(&offer["decision"])),
        format!(
            "Execution: {}",
            offer["execution"].as_str().unwrap_or("not-observed")
        ),
    ]
}

/// Compact task-step and action receipts: the receipt, next action or offer, acceptance and
/// any warnings.
fn action_receipt(action: &str, result: &Value) -> String {
    let mut lines = Vec::new();
    if action == "task-step" {
        let receipt = &result["recorded"];
        lines.push(format!(
            "Step recorded: {} / {} — {}",
            shown(&receipt["operation"]),
            shown(&receipt["target"]),
            shown(&receipt["outcome"])
        ));
        lines.push(checkpoint_line(&result["checkpoint"]));
        if receipt["outcome"] != "passed" {
            lines.push(format!("Result: {}", shown(&result["result"])));
        }
        if result["complete"] == true {
            lines.push(if result["acceptance"]["assurance"] == "attributed" {
                "Task: complete (attributed criterion evidence)".into()
            } else {
                "Task: complete (agent-declared)".into()
            });
        }
        let next = &result["nextStep"];
        if next.is_object() {
            let reason = nonempty(&next["reason"]).map_or(String::new(), |r| format!(" — {r}"));
            lines.push(format!("Next: {}{reason}", shown(&next["action"])));
            for check in next["checks"].as_array().into_iter().flatten() {
                let mut argv = vec![check["command"].clone()];
                argv.extend(check["args"].as_array().cloned().unwrap_or_default());
                lines.push(format!(
                    "Check: {} {}",
                    shown(&check["cwd"]),
                    serde_json::to_string(&argv).expect("JSON values serialize")
                ));
            }
            if let Some(after) = nonempty(&next["afterCheckpoint"]) {
                lines.push(format!("After checkpoint: {after}"));
            }
        }
        let offer = &result["actionOffer"];
        if offer.is_object() && offer["suppressed"] != true {
            lines.extend(offer_lines(offer));
        }
    } else {
        lines.extend(offer_lines(result));
        if action == "task-action-check" {
            lines.push(format!("Ready: {}", result["ready"] == true));
            lines.push(result["reason"].as_str().unwrap_or_default().to_string());
            lines.push("Readiness is not execution or a permission grant.".into());
        } else {
            lines.push("No action dispatched.".into());
        }
    }
    let acceptance = &result["acceptance"];
    if acceptance.is_object() {
        lines.push(format!("Acceptance: {}", shown(&acceptance["assurance"])));
        for criterion in acceptance["criteria"].as_array().into_iter().flatten() {
            let optional = if criterion["required"] == true {
                ""
            } else {
                " (optional)"
            };
            lines.push(format!(
                "Criterion: {} — {}{optional}",
                shown(&criterion["id"]),
                shown(&criterion["status"])
            ));
        }
    }
    if let Some(contract) = nonempty(&result["taskContract"]) {
        lines.push(format!("Task contract: {contract}"));
    }
    if let Some(digest) = nonempty(&result["evidenceDigest"]) {
        lines.push(format!("Evidence digest: {digest}"));
    }
    if result["binding"].is_object() {
        lines.push(format!(
            "Source: {}",
            nonempty(&result["binding"]["identity"]).unwrap_or("unknown")
        ));
    }
    let status = &result["status"];
    if result["workspace"]["status"] == "retained"
        && let Some(reason) = nonempty(&result["workspace"]["reason"])
    {
        lines.push(format!("Workspace retained: {reason}"));
    }
    if truthy_value(&status["taskEvidenceStale"]) {
        lines.push("Warning: task evidence is stale.".into());
    }
    let mut warnings: Vec<&Value> = vec![
        &result["activityDiagnostic"],
        &status["diagnostic"],
        &status["actionDiagnostic"],
        &status["noticeDiagnostic"],
    ];
    warnings.extend(status["diagnostics"].as_array().into_iter().flatten());
    for warning in warnings.into_iter().filter(|w| truthy_value(w)) {
        lines.push(format!("Warning: {}", shown(warning)));
    }
    lines.retain(|line| !line.is_empty());
    format!("{}\n", lines.join("\n"))
}

fn truthy_value(value: &Value) -> bool {
    match value {
        Value::Null | Value::Bool(false) => false,
        Value::String(s) => !s.is_empty(),
        Value::Number(n) => n.as_f64().is_some_and(|f| f != 0.0),
        _ => true,
    }
}

/// The compact task-start receipt: binding, route, checkpoint, advice and acceptance.
fn task_receipt(result: &Value) -> String {
    let routing = &result["routing"];
    let text = |value: &Value| value.as_str().unwrap_or_default().to_string();
    let mut lines = vec![
        format!("Task bound: {}", text(&result["task"])),
        format!(
            "Execution: {} / {}",
            text(&routing["execution"]["mode"]),
            text(&routing["execution"]["role"])
        ),
    ];
    for (label, value) in [
        ("Repositories", &result["repositories"]),
        ("Skills", &routing["skills"]),
        ("References", &routing["references"]),
    ] {
        if let Some(items) = joined(value) {
            lines.push(format!("{label}: {items}"));
        }
    }
    lines.push(checkpoint_line(&result["checkpoint"]));
    let model = &result["modelAdvice"];
    if model.is_object() {
        let profile = nonempty(&model["profile"]).map_or(String::new(), |p| format!(" / {p}"));
        let limitation = nonempty(&model["mainModel"]["limitation"])
            .map_or(" (recommendation only)".to_string(), |l| format!(" — {l}"));
        lines.push(format!(
            "Model advice: {}{profile}{limitation}",
            text(&model["decision"])
        ));
    }
    for missing in routing["missing"].as_array().into_iter().flatten() {
        lines.push(format!("Missing: {}", text(missing)));
    }
    let acceptance = &result["acceptance"];
    if acceptance.is_object() {
        lines.push(format!("Acceptance: {}", text(&acceptance["assurance"])));
        for criterion in acceptance["criteria"].as_array().into_iter().flatten() {
            let optional = if criterion["required"] == true {
                ""
            } else {
                " (optional)"
            };
            lines.push(format!(
                "Criterion: {} — {}{optional}",
                text(&criterion["id"]),
                text(&criterion["status"])
            ));
        }
    }
    if let Some(contract) = nonempty(&result["taskContract"]) {
        lines.push(format!("Task contract: {contract}"));
    }
    if let Some(digest) = nonempty(&result["evidenceDigest"]) {
        lines.push(format!("Evidence digest: {digest}"));
    }
    if result["binding"].is_object() {
        let source = nonempty(&result["binding"]["identity"]).unwrap_or("unknown");
        lines.push(format!("Source: {source}"));
    }
    if result["workspace"]["status"] == "retained"
        && let Some(reason) = nonempty(&result["workspace"]["reason"])
    {
        lines.push(format!("Workspace retained: {reason}"));
    }
    lines.retain(|line| !line.is_empty());
    format!("{}\n", lines.join("\n"))
}

/// The compact checkpoint receipt; `--json` or `--verbose` returns the full result.
fn receipt(saved: &Value) -> String {
    let mut text = checkpoint_line(saved);
    if let Some(warning) = saved["activityDiagnostic"]
        .as_str()
        .filter(|w| !w.is_empty())
    {
        text.push_str(&format!("\nWarning: {warning}"));
    }
    text.push('\n');
    text
}

/// Run one migrated command. `argv` excludes the executable name.
pub fn run(argv: &[String]) -> Outcome {
    run_in_context(argv, &Value::Null)
}

/// A forwarded command can ask for the JSON result instead of a compact receipt
/// (`context.structured`).
pub fn run_in_context(argv: &[String], context: &Value) -> Outcome {
    let (action, args) = match argv.split_first() {
        Some((action, args)) if COMMANDS.contains(&action.as_str()) => (action.as_str(), args),
        _ => {
            return Outcome {
                error_code: None,
                exit_code: 2,
                stdout: String::new(),
                stderr: format!("usage: craft <{}> [options]\n", COMMANDS.join("|")),
                export: None,
            };
        }
    };
    if args.iter().any(|arg| arg == "--help") {
        return Outcome {
            error_code: None,
            exit_code: 0,
            stdout: render(&help(action)),
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
        &resolve(hint("--root").unwrap_or(".")),
        hint("--session"),
        hint("--host"),
    );
    recorder.package(context["package"].as_str());
    let operation = recorder.start("cli.command", None);
    let command = match action {
        "grounded-context" => {
            |o: &Options, r: &mut Recorder, p: &Operation| grounded_context(o, r, p, None)
        }
        "operation" => plan_operation,
        "prepare-services" => prepare_services,
        "model-advice" => model_advice,
        "lookup" => lookup,
        "worker-status" => worker_status,
        "route" => route,
        "context-report" => context_report,
        "locate" => locate,
        "checkpoint-save" => checkpoint_save,
        "checkpoint-read" => checkpoint_read,
        "session-attempt" => session_attempt,
        "task-start" => task_start,
        "session" | "session-advice" => session_status,
        "task-step" => task_step,
        "task-offer" => |o: &Options, r: &mut Recorder, p: &Operation| {
            task_action(crate::actions::Kind::Offer, o, r, p)
        },
        "task-decision" => |o: &Options, r: &mut Recorder, p: &Operation| {
            task_action(crate::actions::Kind::Decision, o, r, p)
        },
        "task-action-check" => |o: &Options, r: &mut Recorder, p: &Operation| {
            task_action(crate::actions::Kind::Check, o, r, p)
        },
        "task-feature" => {
            |o: &Options, r: &mut Recorder, p: &Operation| task_features(true, o, r, p)
        }
        "task-features" => {
            |o: &Options, r: &mut Recorder, p: &Operation| task_features(false, o, r, p)
        }
        _ => settings,
    };
    let result = parse(action, args).and_then(|options| {
        if let Some(action) = crate::scratch::Action::from_command(action) {
            return workspace(action, &options, &mut recorder, &operation);
        }
        if action == "grounded-context" && context.get("data").is_some() {
            return grounded_context(&options, &mut recorder, &operation, context.get("data"));
        }
        if matches!(action, "session" | "session-advice") && context.get("data").is_some() {
            return session_status_input(&options, &mut recorder, &operation, context.get("data"));
        }
        command(&options, &mut recorder, &operation)
    });
    let outcome = match &result {
        Ok(value) => {
            recorder.complete(
                operation,
                Map::from_iter([("command".to_string(), json!(action))]),
                None,
            );
            let compact = context["structured"] != true
                && !args.iter().any(|arg| arg == "--json" || arg == "--verbose");
            let text = match action {
                "checkpoint-save" if compact => receipt(value),
                "task-start" if compact => task_receipt(value),
                "session" | "session-advice" if compact => session_view(value),
                "task-step" | "task-offer" | "task-decision" | "task-action-check" if compact => {
                    action_receipt(action, value)
                }
                "session-attempt" if compact => {
                    let recorded = &value["recorded"];
                    let field =
                        |name: &str| recorded[name].as_str().unwrap_or_default().to_string();
                    format!(
                        "Attempt recorded: {} / {} — {}\n",
                        field("operation"),
                        field("target"),
                        field("outcome")
                    )
                }
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
        error_code: result.err().and_then(|failure| match failure.io {
            Some(ErrorKind::PermissionDenied) => Some("EACCES"),
            Some(ErrorKind::StorageFull) => Some("ENOSPC"),
            Some(ErrorKind::NotFound) => Some("ENOENT"),
            Some(ErrorKind::NotADirectory) => Some("ENOTDIR"),
            Some(ErrorKind::ResourceBusy) => Some("EBUSY"),
            Some(ErrorKind::AlreadyExists) => Some("EEXIST"),
            _ => None,
        }),
        exit_code: outcome.0,
        stdout: outcome.1,
        stderr: outcome.2,
        export: recorder.finish(),
    }
}
