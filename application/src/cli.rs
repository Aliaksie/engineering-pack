//! Commands owned by the Rust runtime, with the argument rules, output and exit codes of
//! `cli/main.mjs`, which forwards these commands here.
use crate::project::{self, Settings};
use crate::telemetry::{Operation, Recorder};
use craft_platform::documents::DocumentError;
use serde_json::{Map, Value, json};
use std::io::ErrorKind;
use std::path::{Component, Path, PathBuf};

pub const COMMANDS: &[&str] = &["settings"];

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

fn input(recorder: &mut Recorder, parent: &Operation, file: &str) -> Result<Value, Failure> {
    let operation = recorder.start("input.loaded", Some(parent));
    let result = project::input(Path::new(file));
    let failure = result.as_ref().err().map(|f| f.code("INVALID_INPUT"));
    recorder.complete(operation, Map::new(), failure);
    result
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
    project::effective_settings(&resolve(options.get("root").unwrap_or(".")), request)
}

/// Help for `settings`, the only migrated command so far.
fn help() -> Value {
    json!({
        "usage": "settings [--root project] [--preferences user.json] [--data task-overrides.json] [--session id --facts worker-observations.json]",
        "mode": "effective settings, value origins, applied controls and private state location; no action dispatch",
        "output": "Routine receipts are compact by default; --json or --verbose returns the full structured result."
    })
}

fn render(value: &Value) -> String {
    format!(
        "{}\n",
        serde_json::to_string_pretty(value).expect("JSON values serialize")
    )
}

/// Run one migrated command. `argv` excludes the executable name.
pub fn run(argv: &[String]) -> Outcome {
    let (action, args) = match argv.split_first() {
        Some((action, args)) if COMMANDS.contains(&action.as_str()) => (action.as_str(), args),
        _ => {
            return Outcome {
                exit_code: 2,
                stdout: String::new(),
                stderr: format!("usage: craft <{}> [options]\n", COMMANDS.join("|")),
                export: None,
            };
        }
    };
    if args.iter().any(|arg| arg == "--help") {
        return Outcome {
            exit_code: 0,
            stdout: render(&help()),
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
    let operation = recorder.start("cli.command", None);
    let result =
        parse(action, args).and_then(|options| settings(&options, &mut recorder, &operation));
    let outcome = match &result {
        Ok(value) => {
            recorder.complete(
                operation,
                Map::from_iter([("command".to_string(), json!(action))]),
                None,
            );
            (0, render(value), String::new())
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
