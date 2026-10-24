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
];
/// Every command `cli/main.mjs` lists in its general help, migrated or not.
const ALL_COMMANDS: &[&str] = &[
    "workspace-open",
    "workspace-write",
    "workspace-promote",
    "workspace-release",
    "workspace-clean",
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
    /// Session activity counters for the caller that owns session state until W2.
    pub activity: Option<Value>,
}

#[derive(Default)]
struct Options {
    values: Map<String, Value>,
    discovery: Value,
}

impl Options {
    fn get(&self, name: &str) -> Option<&str> {
        self.values.get(name).and_then(Value::as_str)
    }
}

/// `path.resolve`: absolute against the working directory, with `.` and `..` removed lexically.
fn resolve(path: &str) -> PathBuf {
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
    observe(
        recorder,
        "integration.prepared",
        parent,
        "INTEGRATION_UNAVAILABLE",
        Map::new(),
        |recorder, operation| {
            let result = services::prepare(
                &settings,
                options.values.get("available-command"),
                options.values.get("available-mcp-tool"),
            )?;
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
fn routed(
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

fn route(options: &Options, recorder: &mut Recorder, parent: &Operation) -> Result<Value, Failure> {
    let settings = loaded(options, recorder, parent)?;
    let risks = strings(options, "risk");
    let request = route_request(options, files(options), &risks);
    routed(&settings, &request, recorder, parent)
}

/// Selected guidance accounting, and the session activity counters it implies.
fn context_report(
    options: &Options,
    recorder: &mut Recorder,
    parent: &Operation,
) -> Result<(Value, Value), Failure> {
    let settings = loaded(options, recorder, parent)?;
    let risks = strings(options, "risk");
    // Guidance accounting neither delegates nor reads the task.
    let request = routing::Request {
        delegate: false,
        task: None,
        ..route_request(options, files(options), &risks)
    };
    let plan = routed(&settings, &request, recorder, parent)?;
    let operation = recorder.start("context.observed", Some(parent));
    let result = (|| -> Result<Value, Failure> {
        let requests = context::requests(&plan)?;
        let (entries, missing) = guidance::measure(&strings(options, "bundle"), &requests)?;
        let budget = match options.get("budget-bytes") {
            Some(text) => Some(context::budget(&crate::discovery::number(text))?),
            None => None,
        };
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
    let report = result?;
    let references = plan["references"].as_array().map_or(0, Vec::len);
    let activity =
        json!({ "filesRequested": report["totals"]["files"], "referencesRequested": references });
    Ok((report, activity))
}

fn locate(
    options: &Options,
    recorder: &mut Recorder,
    parent: &Operation,
) -> Result<Value, Failure> {
    let request = if let Some(request) = options.discovery.get("request") {
        request.clone()
    } else {
        match options.get("data") {
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
        }
    };
    crate::discovery::run(
        &request,
        crate::discovery::Context {
            root: &root(options),
            session: options.get("session"),
            task: options.get("task"),
            preferences: options.get("preferences").map(Path::new),
            bridge: &options.discovery,
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
    let mut saved = crate::checkpoint::save(&root, name, &metadata, recorder, parent)?;
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

/// The compact checkpoint receipt; `--json` or `--verbose` returns the full result.
fn receipt(saved: &Value) -> String {
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
    if let Some(file) = saved["file"].as_str().filter(|f| !f.is_empty()) {
        text.push_str(&format!(" — {file}"));
    }
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

/// A forwarded command can share the JS command's operation (`context.telemetry`), receive
/// the session observation its W2 owner validated (`context.discovery`) and ask for the
/// JSON result instead of a compact receipt (`context.structured`).
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
                activity: None,
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
            activity: None,
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
    let inherited = recorder.inherit(&context["telemetry"]);
    let forwarded = inherited.is_some();
    let operation = inherited.unwrap_or_else(|| recorder.start("cli.command", None));
    let command = match action {
        "operation" => plan_operation,
        "prepare-services" => prepare_services,
        "model-advice" => model_advice,
        "lookup" => lookup,
        "route" => route,
        "locate" => locate,
        "checkpoint-save" => checkpoint_save,
        "checkpoint-read" => checkpoint_read,
        _ => settings,
    };
    let mut activity = None;
    let result = parse(action, args).and_then(|mut options| {
        options.discovery = context["discovery"].clone();
        if action == "context-report" {
            let (report, counters) = context_report(&options, &mut recorder, &operation)?;
            activity = Some(counters);
            return Ok(report);
        }
        command(&options, &mut recorder, &operation)
    });
    let outcome = match &result {
        Ok(value) => {
            if !forwarded {
                recorder.complete(
                    operation,
                    Map::from_iter([("command".to_string(), json!(action))]),
                    None,
                );
            }
            let compact = action == "checkpoint-save"
                && context["structured"] != true
                && !args.iter().any(|arg| arg == "--json" || arg == "--verbose");
            let text = if compact {
                receipt(value)
            } else {
                render(value)
            };
            (0, text, String::new())
        }
        Err(failure) => {
            if !forwarded {
                recorder.complete(operation, Map::new(), Some(failure.code("INVALID_INPUT")));
            }
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
        activity,
    }
}
