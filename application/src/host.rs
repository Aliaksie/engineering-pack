//! Host lifecycle hooks: the decision for one hook event over host-neutral facts. `craft
//! hook` derives the facts from the host payload and serializes the decision for its wire
//! format; observation hooks never supply a permission decision or context.
use crate::failure::Failure;
use crate::project::absolute;
use crate::telemetry::Recorder;
use crate::telemetry::observe;
use crate::{delivery, project};
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
    /// Routing hints, checkpoints and service metadata at lifecycle boundaries.
    Deliver,
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
    file: Option<&'a str>,
    prompt: Option<&'a str>,
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
            Some("deliver") => Handler::Deliver,
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
        file: optional("file")?,
        prompt: optional("prompt")?,
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
    let cwd = absolute(facts.cwd);
    let root = project_root(&cwd);
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
        // Hosts continue after a failing hook, so an enabled shell guard denies explicitly.
        let guarded = facts.handler == Handler::ReviewReadOnly
            && facts.shell_only
            && facts.lifecycle == "before-tool"
            && project::settings(&root, None)
                .map_or(true, |settings| settings["review"]["readOnly"] == true);
        recorder.complete(hook, summary, Some("HOOK_FAILED"));
        let mut result = if guarded {
            craft_engine::delivery::failure(true, "hook input could not be read in full")
        } else {
            json!({})
        };
        result["failed"] = json!(true);
        result["export"] = json!(recorder.finish());
        return Ok(result);
    }
    let mut decision = json!({"decision": "allow"});
    let mut failed = false;
    let applies = facts.input_status == "complete" && (!facts.shell_only || facts.tool == "shell");
    if applies {
        let evaluated = match facts.handler {
            Handler::Observe => Ok(decision.clone()),
            Handler::ReviewReadOnly => observe(
                &mut recorder,
                "project.loaded",
                &hook,
                "CONFIG_INVALID",
                Map::new(),
                |_, _| project::settings(&root, None),
            )
            .map(|settings| {
                review::read_only_decision(&settings, facts.tool == "shell", facts.command)
            }),
            Handler::Deliver => {
                let event = delivery::Event {
                    root: &root,
                    cwd: &cwd,
                    host: facts.host,
                    lifecycle: facts.lifecycle,
                    session: facts.session,
                    edit: facts.tool == "edit",
                    file: facts.file,
                    prompt: facts.prompt,
                    context_supported: facts.context_supported,
                };
                delivery::run(&event, &mut recorder, &hook)
            }
        };
        match evaluated {
            Ok(evaluated) => decision = evaluated,
            Err(Failure { message, .. }) => {
                recorder.report("host", "HOOK_FAILED", Map::new(), &hook);
                failed = true;
                let enforcement = facts.shell_only && facts.lifecycle == "before-tool";
                decision = craft_engine::delivery::failure(enforcement, &message);
            }
        }
    }
    let mut summary = attributes(&[
        ("lifecycle", lifecycle),
        ("inputStatus", status),
        ("decision", decision["decision"].clone()),
    ]);
    // Whether the host can take context is known only for a hook that applied.
    if applies {
        summary.insert("contextSupported".into(), json!(facts.context_supported));
    }
    recorder.complete(hook, summary, failed.then_some("HOOK_FAILED"));
    decision["failed"] = json!(failed);
    decision["export"] = json!(recorder.finish());
    Ok(decision)
}
