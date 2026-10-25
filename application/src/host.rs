//! Host lifecycle hooks: one native invocation per hook event. Host adapters send
//! host-neutral facts and serialize the decision for their wire format; observation
//! hooks never supply a permission decision or context.
use crate::cli::{Failure, observe, resolve};
use crate::project;
use crate::telemetry::Recorder;
use craft_engine::review;
use serde_json::{Map, Value, json};
use std::path::{Path, PathBuf};

const LIFECYCLES: [&str; 7] = [
    "session-start",
    "before-prompt",
    "before-tool",
    "after-tool",
    "before-compact",
    "after-compact",
    "turn-end",
];
const INPUT_STATUSES: [&str; 4] = ["complete", "invalid", "oversized", "read-failure"];
const TOOLS: [&str; 4] = ["shell", "edit", "read", "other"];

#[derive(Clone, Copy, PartialEq)]
enum Handler {
    /// Lifecycle telemetry only; independent of project configuration.
    Observe,
    /// The read-only review publication guard for shell tool calls.
    ReviewReadOnly,
}

struct Facts<'a> {
    handler: Handler,
    host: &'a str,
    lifecycle: &'a str,
    input_status: &'a str,
    tool: &'a str,
    /// Whether the hook is registered for shell tools only.
    shell_only: bool,
    context_supported: bool,
    cwd: &'a str,
    session: Option<&'a str>,
    package: Option<&'a str>,
    command: &'a Value,
}

fn one_of<'a>(input: &'a Value, name: &str, allowed: &[&str]) -> Result<&'a str, String> {
    input[name]
        .as_str()
        .filter(|value| allowed.contains(value))
        .ok_or_else(|| format!("invalid hook {name}"))
}

fn facts(input: &Value) -> Result<Facts<'_>, String> {
    let optional = |name: &str| match &input[name] {
        Value::Null => Ok(None),
        Value::String(text) => Ok(Some(text.as_str())),
        _ => Err(format!("invalid hook {name}")),
    };
    Ok(Facts {
        handler: match input["handler"].as_str() {
            Some("observe") => Handler::Observe,
            Some("review-read-only") => Handler::ReviewReadOnly,
            _ => return Err("unknown hook handler".into()),
        },
        host: input["host"].as_str().ok_or("invalid hook host")?,
        lifecycle: one_of(input, "lifecycle", &LIFECYCLES)?,
        input_status: one_of(input, "inputStatus", &INPUT_STATUSES)?,
        tool: one_of(input, "tool", &TOOLS)?,
        shell_only: one_of(input, "scope", &["any", "shell"])? == "shell",
        context_supported: input["contextSupported"]
            .as_bool()
            .ok_or("invalid hook contextSupported")?,
        cwd: input["cwd"].as_str().ok_or("invalid hook cwd")?,
        session: optional("session")?,
        package: optional("package")?,
        command: &input["command"],
    })
}

/// The nearest ancestor (at most 30 levels) with project configuration or a Git entry,
/// else the directory itself.
fn project_root(cwd: &Path) -> PathBuf {
    let mut directory = cwd;
    for _ in 0..30 {
        if directory.join(".craft/project.json").exists() || directory.join(".git").exists() {
            return directory.to_path_buf();
        }
        match directory.parent() {
            Some(parent) => directory = parent,
            None => break,
        }
    }
    cwd.to_path_buf()
}

fn attributes(pairs: &[(&str, Value)]) -> Map<String, Value> {
    pairs
        .iter()
        .map(|(name, value)| (name.to_string(), value.clone()))
        .collect()
}

/// One hook event: `{decision, reason?, failed, export}`.
pub fn run(input: &Value) -> Result<Value, String> {
    let facts = facts(input)?;
    let root = project_root(&resolve(facts.cwd));
    let mut recorder = Recorder::open(&root, facts.session, Some(facts.host));
    recorder.package(facts.package);
    let hook = recorder.start("host.hook", None);
    let observational = facts.handler == Handler::Observe;
    let status = json!(facts.input_status);
    let lifecycle = json!(facts.lifecycle);
    let unread = matches!(facts.input_status, "oversized" | "read-failure");
    if unread || facts.input_status == "invalid" {
        let reason = if facts.input_status == "oversized" {
            "INPUT_TOO_LARGE"
        } else {
            "INVALID_INPUT"
        };
        let details = attributes(&[
            ("inputStatus", status.clone()),
            ("lifecycle", lifecycle.clone()),
        ]);
        recorder.report("host", reason, details, &hook);
    }
    if unread {
        // Policy hooks reject unread input; observation records it and stays silent.
        let summary = if observational {
            attributes(&[("lifecycle", lifecycle), ("inputStatus", status)])
        } else {
            Map::new()
        };
        recorder.complete(hook, summary, Some("HOOK_FAILED"));
        return Ok(json!({"failed": true, "export": recorder.finish()}));
    }
    let mut decision = json!({"decision": "allow"});
    let mut failed = false;
    if facts.input_status == "complete" && (!facts.shell_only || facts.tool == "shell") {
        let observed = attributes(&[
            ("lifecycle", lifecycle.clone()),
            ("contextSupported", json!(facts.context_supported)),
        ]);
        recorder.observe("host.observed", observed, &hook, &[]);
        if facts.handler == Handler::ReviewReadOnly {
            let settings = observe(
                &mut recorder,
                "project.loaded",
                &hook,
                "CONFIG_INVALID",
                Map::new(),
                |_, _| project::settings(&root, None),
            );
            match settings {
                Ok(settings) => {
                    decision =
                        review::read_only_decision(&settings, facts.tool == "shell", facts.command)
                }
                Err(Failure { message, .. }) => {
                    recorder.report("host", "HOOK_FAILED", Map::new(), &hook);
                    failed = true;
                    decision = failure(
                        facts.shell_only && facts.lifecycle == "before-tool",
                        &message,
                    );
                }
            }
        }
    }
    let summary = attributes(&[
        ("lifecycle", lifecycle),
        ("inputStatus", status),
        ("decision", decision["decision"].clone()),
    ]);
    recorder.complete(hook, summary, failed.then_some("HOOK_FAILED"));
    decision["failed"] = json!(failed);
    decision["export"] = json!(recorder.finish());
    Ok(decision)
}

/// A hook that could not be evaluated: shell policy fails closed, other hooks continue
/// and report why their context is unavailable.
fn failure(enforcement: bool, message: &str) -> Value {
    if enforcement {
        json!({"decision": "deny", "reason": format!("Policy could not be evaluated: {message}")})
    } else {
        json!({"decision": "allow", "context": {"kind": "unavailable", "data": message}})
    }
}
