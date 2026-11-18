//! Host lifecycle hooks: the decision for one hook event over host-neutral facts. `craft
//! hook` derives the facts from the host payload and serializes the decision for its wire
//! format; observation hooks never supply a permission decision or context.
use crate::failure::Failure;
use crate::project::{absolute, project_root};
use crate::telemetry::Recorder;
use crate::{hook::delivery, project};
use craft_engine::review;
use craft_platform::state;
use serde_json::{Map, Value, json};
use std::path::Path;

const LIFECYCLES: [&str; 9] = [
    "session-start",
    "before-prompt",
    "before-tool",
    "after-tool",
    "before-compact",
    "after-compact",
    "turn-end",
    "agent-start",
    "after-tool-failure",
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
    /// The directory the host was opened in, when it names one.
    project_directory: Option<&'a str>,
    session: Option<&'a str>,
    package: Option<&'a str>,
    command: &'a Value,
    file: Option<&'a str>,
    /// Every file an edit writes, when one edit writes several (a patch).
    files: Vec<&'a str>,
    prompt: Option<&'a str>,
    pattern: Option<&'a str>,
    /// A file or search filter the tool call names that reaches secret files.
    secret: Option<&'a str>,
    /// The host is continuing a stop a hook already held.
    stop_active: bool,
    /// The runner's verdict on a test command the host just ran.
    test_run: Option<&'a Value>,
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
        project_directory: optional("projectDirectory")?,
        session: optional("session")?,
        package: optional("package")?,
        command: &input["command"],
        file: optional("file")?,
        files: match &input["files"] {
            Value::Null => Vec::new(),
            Value::Array(files) => files
                .iter()
                .map(|file| file.as_str().ok_or("invalid hook files"))
                .collect::<Result<_, _>>()?,
            _ => return Err("invalid hook files".into()),
        },
        prompt: optional("prompt")?,
        pattern: optional("pattern")?,
        secret: optional("secret")?,
        stop_active: input["stopActive"] == true,
        test_run: match &input["testRun"] {
            Value::Null => None,
            run @ Value::Object(_) => Some(run),
            _ => return Err("invalid hook testRun".into()),
        },
    })
}

/// Whether a compaction is kept in the session's host context. Without session records it
/// only updates a host context an explicit integration (the status line, `host-observe`)
/// already keeps, so occupancy from before the compaction is not read after it; it creates
/// none. Settings that fail to load leave the record on, as before the switch.
fn compaction_recorded(root: &Path, session: &str) -> bool {
    project::settings(root, None).map_or(true, |settings| {
        craft_engine::delivery::session_records(&settings)
    }) || state::file(root, "host-context.json", false, Some(session))
        .is_ok_and(|file| file.is_file())
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
    // The directory the host was opened in names the project even after the session's shell
    // moved elsewhere; without one, the shell's directory does.
    let opened = facts
        .project_directory
        .map(absolute)
        .filter(|directory| directory.is_dir());
    let root = project_root(opened.as_deref().unwrap_or(&cwd));
    // A session directory this hook creates names its host.
    let _describing = state::describing(facts.host);
    let mut recorder = Recorder::open(&root, facts.session, Some(facts.host));
    recorder.package(facts.package);
    let hook = recorder.start("host.hook", None);
    if facts.input_status == "complete"
        && matches!(facts.lifecycle, "before-compact" | "after-compact")
        && let Some(session) = facts.session
        && compaction_recorded(&root, session)
    {
        let event = craft_engine::session::host_context::lifecycle(
            facts.host,
            session,
            facts.lifecycle,
            &craft_engine::timestamp::format(crate::telemetry::now_ms()),
        );
        if event
            .map_err(Failure::from)
            .and_then(|event| crate::host_context::record_event(&root, session, &event))
            .is_err()
        {
            recorder.report("host", "HOST_OBSERVATION_UNAVAILABLE", Map::new(), &hook);
        }
    }
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
        // Every hook names the event and why it did not read it, so doctor can count it.
        let summary = attributes(&[("lifecycle", lifecycle), ("inputStatus", status)]);
        // Hosts continue after a failing hook, so an enabled shell guard denies explicitly:
        // the read-only review, and the safety policy, which every package's hook before a
        // tool enforces, before a shell command (or a tool the input did not name) that it
        // could not read.
        let before_shell = facts.lifecycle == "before-tool" && facts.tool == "shell";
        let guarded =
            match facts.handler {
                Handler::ReviewReadOnly => facts.shell_only && before_shell,
                Handler::Deliver | Handler::Observe => before_shell,
            } && project::settings(&root, None).map_or(true, |settings| match facts.handler {
                Handler::ReviewReadOnly => settings["review"]["readOnly"] == true,
                _ => craft_engine::safety::enabled(&settings),
            });
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
    // Observation records a session's boundaries; a prompt, tool call or turn end is recorded
    // once, by the handler that acts on it, not again by every installed package. The safety
    // policy holds with any package installed: each one's hook before a shell command
    // refuses what the policy refuses, and only that refusal is recorded.
    let mut refused = None;
    if observational
        && facts.input_status == "complete"
        && !matches!(
            facts.lifecycle,
            "session-start" | "before-compact" | "after-compact"
        )
    {
        if facts.lifecycle == "before-tool"
            && facts.tool == "shell"
            && let Some(command) = facts.command.as_str()
            && let Ok(settings) = project::hook_loaded(&mut recorder, &hook, &root)
        {
            let decision = crate::hook::safety::decision(&settings, &root, &cwd, command);
            refused = (decision["decision"] == "deny").then_some(decision);
        } else if facts.lifecycle == "before-tool"
            && let Some(secret) = facts.secret
            && let Ok(settings) = project::hook_loaded(&mut recorder, &hook, &root)
        {
            let decision =
                crate::hook::safety::read_decision(&settings, &cwd, facts.tool == "edit", secret);
            refused = (decision["decision"] == "deny").then_some(decision);
        }
        if refused.is_none() {
            return Ok(json!({"decision": "allow", "failed": false, "export": recorder.finish()}));
        }
    }
    let mut decision = json!({"decision": "allow"});
    let mut failed = false;
    let applies = facts.input_status == "complete" && (!facts.shell_only || facts.tool == "shell");
    if applies {
        let evaluated = match facts.handler {
            Handler::Observe => Ok(refused.take().unwrap_or(decision.clone())),
            Handler::ReviewReadOnly => {
                project::hook_loaded(&mut recorder, &hook, &root).map(|settings| {
                    review::read_only_decision(&settings, facts.tool == "shell", facts.command)
                })
            }
            Handler::Deliver => {
                let event = delivery::Event {
                    root: &root,
                    cwd: &cwd,
                    host: facts.host,
                    package: facts.package,
                    lifecycle: facts.lifecycle,
                    session: facts.session,
                    tool: facts.tool,
                    edit: facts.tool == "edit",
                    edit_context: crate::hook::delivers_edit_context(facts.host, facts.lifecycle),
                    file: facts.file,
                    files: &facts.files,
                    prompt: facts.prompt,
                    pattern: facts.pattern,
                    secret: facts.secret,
                    command: facts.command.as_str(),
                    context_supported: facts.context_supported,
                    stop_active: facts.stop_active,
                    test_run: facts.test_run,
                };
                delivery::run(&event, &mut recorder, &hook)
            }
        };
        match evaluated {
            Ok(evaluated) => decision = evaluated,
            Err(Failure { message, .. }) => {
                recorder.report("host", "HOOK_FAILED", Map::new(), &hook);
                failed = true;
                // Counted for the session summary, once per event: by the delivery hook.
                if facts.handler == Handler::Deliver
                    && let Some(session) = facts.session.filter(|s| !s.is_empty())
                    // A project file that cannot be read is the usual cause: still counted.
                    && project::settings(&root, None)
                        .map_or(true, |settings| craft_engine::delivery::session_records(&settings))
                    && crate::hook::delivered::update(&root, session, |record| {
                        Some(craft_engine::session::delivered::hook_failure(record))
                    })
                    .is_err()
                {
                    recorder.report("state", "STATE_UNAVAILABLE", Map::new(), &hook);
                }
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
    // What the hook gave the model: bytes of context, pieces left out to stay under the
    // host's ceiling, answers held back by the session budget.
    if let Some(Value::Object(delivered)) = decision
        .as_object_mut()
        .and_then(|fields| fields.shift_remove("delivered"))
    {
        summary.extend(delivered);
    }
    recorder.complete(hook, summary, failed.then_some("HOOK_FAILED"));
    decision["failed"] = json!(failed);
    decision["export"] = json!(recorder.finish());
    Ok(decision)
}
