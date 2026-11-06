//! `craft hook <host> <lifecycle> --package <name> --handler <handler> [--scope shell]`:
//! one host hook event from the host's own payload on stdin to its wire output. The host
//! mapping table is the data the build uses to register hooks.
use crate::host;
use crate::telemetry::Recorder;
use craft_engine::values::js_length;
use serde_json::{Value, json};
use std::io::{Read, Write};

const MAX_INPUT: usize = 65_536;
const MAX_CONTEXT: usize = 12_000;
const HOSTS: &str = include_str!("../../hosts/hooks.json");

pub struct Wire {
    pub stdout: String,
    pub stderr: String,
    pub exit_code: i32,
}

struct Hook {
    host: String,
    lifecycle: String,
    package: String,
    handler: String,
    shell: bool,
}

fn arguments(args: &[String]) -> Result<Hook, String> {
    let [host, lifecycle, rest @ ..] = args else {
        return Err("usage: craft hook <host> <lifecycle> --package <name> --handler <handler> [--scope shell]".into());
    };
    let (mut package, mut handler, mut shell) = (None, None, false);
    let mut options = rest.iter();
    while let Some(option) = options.next() {
        let value = options
            .next()
            .ok_or_else(|| format!("{option} requires a value"))?;
        match option.as_str() {
            "--package" => package = Some(value.clone()),
            "--handler" => handler = Some(value.clone()),
            "--scope" if value == "shell" || value == "any" => shell = value == "shell",
            _ => return Err(format!("unknown hook option {option} {value}")),
        }
    }
    let table: Value = serde_json::from_str(HOSTS).expect("embedded host hook table");
    if table.get(host).is_none() {
        return Err(format!("unknown hook platform {host}"));
    }
    Ok(Hook {
        host: host.clone(),
        lifecycle: lifecycle.clone(),
        package: package.ok_or("--package is required")?,
        handler: handler.ok_or("--handler is required")?,
        shell,
    })
}

/// Whether the host lists plugin skills as `<package>:<skill>`; otherwise skills live in one
/// flat folder as `<package>-<skill>`.
fn plugin_names(host: &str) -> bool {
    let table: Value = serde_json::from_str(HOSTS).expect("embedded host hook table");
    table[host]["listing"]["names"] == "plugin"
}

/// The host's registration of this event, if it supports one.
fn mapping(host: &str, lifecycle: &str) -> Option<Value> {
    let table: Value = serde_json::from_str(HOSTS).expect("embedded host hook table");
    table[host]["events"].get(lifecycle).cloned()
}

/// Whether `lifecycle` is the one tool hook that delivers an edit's context on `host`: before
/// the tool runs where the host shows context there, otherwise after it.
pub(crate) fn delivers_edit_context(host: &str, lifecycle: &str) -> bool {
    let shows = |lifecycle| mapping(host, lifecycle).is_some_and(|m| m["context"] == true);
    match lifecycle {
        "before-tool" => shows("before-tool"),
        "after-tool" => !shows("before-tool") && shows("after-tool"),
        _ => false,
    }
}

/// JSON text with lone UTF-16 surrogate escapes replaced by U+FFFD. JavaScript accepts
/// them; rejecting the whole payload would hide the rest of the command from policy.
pub(crate) fn repair_surrogates(text: &str) -> std::borrow::Cow<'_, str> {
    if !text.contains("\\u") {
        return text.into();
    }
    let bytes = text.as_bytes();
    let unit = |at: usize| -> Option<u16> {
        (bytes.get(at) == Some(&b'\\') && bytes.get(at + 1) == Some(&b'u'))
            .then(|| text.get(at + 2..at + 6))
            .flatten()
            .and_then(|hex| u16::from_str_radix(hex, 16).ok())
    };
    let (mut output, mut copied, mut index, mut in_string) = (String::new(), 0, 0, false);
    while index < bytes.len() {
        match bytes[index] {
            b'"' => in_string = !in_string,
            b'\\' if in_string => {
                if let Some(first) = unit(index) {
                    let high = (0xD800..0xDC00).contains(&first);
                    let paired =
                        high && unit(index + 6).is_some_and(|u| (0xDC00..0xE000).contains(&u));
                    if paired {
                        index += 12;
                        continue;
                    }
                    if (0xD800..0xE000).contains(&first) {
                        output.push_str(&text[copied..index]);
                        output.push_str("\\ufffd");
                        index += 6;
                        copied = index;
                        continue;
                    }
                }
                index += 2;
                continue;
            }
            _ => {}
        }
        index += 1;
    }
    if copied == 0 {
        return text.into();
    }
    output.push_str(&text[copied..]);
    output.into()
}

/// `{payload, inputStatus, inputError}` as the host delivered it, bounded to 64 KiB.
fn read_payload(input: &mut dyn Read) -> (Value, &'static str, Option<String>) {
    let mut bytes = Vec::new();
    match input.take(MAX_INPUT as u64 + 1).read_to_end(&mut bytes) {
        Err(error) => (json!({}), "read-failure", Some(error.to_string())),
        Ok(_) if bytes.len() > MAX_INPUT => (
            json!({}),
            "oversized",
            Some("hook input exceeds 64 KiB".into()),
        ),
        Ok(_) => {
            let text = String::from_utf8_lossy(&bytes);
            match serde_json::from_str::<Value>(&repair_surrogates(&text)) {
                Ok(payload @ Value::Object(_)) => (payload, "complete", None),
                _ => (json!({}), "invalid", None),
            }
        }
    }
}

/// JavaScript `String(value)` for the shapes a tool name can take.
fn js_string(value: &Value) -> String {
    match value {
        Value::Null => "null".into(),
        Value::String(text) => text.clone(),
        Value::Bool(flag) => flag.to_string(),
        Value::Number(number) => number.to_string(),
        Value::Array(items) => items
            .iter()
            .map(|item| {
                if item.is_null() {
                    String::new()
                } else {
                    js_string(item)
                }
            })
            .collect::<Vec<_>>()
            .join(","),
        Value::Object(_) => "[object Object]".into(),
    }
}

fn first<'a>(payload: &'a Value, names: &[&str]) -> Option<&'a Value> {
    names
        .iter()
        .map(|name| &payload[*name])
        .find(|value| !value.is_null())
}

fn tool(name: &str) -> &'static str {
    let lower = name.to_ascii_lowercase();
    let any =
        |names: &[&str]| names.iter().any(|n| n.eq_ignore_ascii_case(&lower)) && name.is_ascii();
    if any(&[
        "bash",
        "shell",
        "powershell",
        "pwsh",
        "exec_command",
        "run_shell_command",
    ]) {
        "shell"
    } else if any(&[
        "edit",
        "write",
        "multiedit",
        "apply_patch",
        "replace",
        "write_file",
    ]) {
        "edit"
    } else if any(&["read", "read_file", "view"]) {
        "read"
    } else {
        "other"
    }
}

/// The host-neutral event in a host payload: `{event, host, tool, contextSupported, cwd,
/// command, file?, prompt, session?}`.
fn normalize(payload: &Value, host: &str, lifecycle: &str) -> Value {
    let mut input = first(payload, &["tool_input", "toolArgs"])
        .cloned()
        .unwrap_or(json!({}));
    if let Value::String(text) = &input {
        input = serde_json::from_str(&repair_surrogates(text)).unwrap_or(json!({}));
    }
    let text = |value: &Value| value.as_str().map(str::to_string);
    let name = first(payload, &["tool_name", "toolName"]).map_or_else(String::new, js_string);
    let cwd = text(&payload["cwd"]).unwrap_or_else(|| {
        std::env::current_dir()
            .map(|dir| dir.to_string_lossy().into_owned())
            .unwrap_or_default()
    });
    let mut event = json!({
        "event": lifecycle, "host": host, "tool": tool(&name),
        "contextSupported": mapping(host, lifecycle).is_some_and(|m| m["context"] == true),
        "cwd": cwd,
        "command": text(&input["command"]).or_else(|| text(&input["cmd"])).unwrap_or_default(),
    });
    if let Some(file) = text(&input["file_path"]).or_else(|| text(&input["path"])) {
        event["file"] = json!(file);
    }
    // A search tool's pattern (Claude `Grep`, Gemini `search_file_content`).
    if let Some(pattern) = text(&input["pattern"]).filter(|p| js_length(p) <= 256) {
        event["pattern"] = json!(pattern);
    }
    event["prompt"] = json!(text(&payload["prompt"]).unwrap_or_default());
    // The host is already continuing a stop a hook held: Claude, Codex and Gemini say so,
    // Cursor counts the follow-ups it submitted.
    if payload["stop_hook_active"] == true || payload["loop_count"].as_u64().is_some_and(|n| n > 0)
    {
        event["stopActive"] = json!(true);
    }
    if let Some(session) = first(payload, &["session_id", "sessionId", "conversation_id"])
        .and_then(Value::as_str)
        .filter(|s| js_length(s) <= 512)
    {
        event["session"] = json!(session);
    }
    event
}

/// Facts for the native `host` handler; only shell-scoped hooks and delivery receive the
/// command, and only delivery the file, prompt and search pattern.
fn facts(hook: &Hook, event: &Value, status: &str) -> Value {
    let mut facts = json!({
        "handler": hook.handler, "host": hook.host, "package": hook.package, "lifecycle": hook.lifecycle,
        "scope": if hook.shell { "shell" } else { "any" }, "inputStatus": status, "tool": event["tool"],
        "contextSupported": event["contextSupported"], "cwd": event["cwd"], "session": event["session"],
    });
    if hook.shell {
        facts["command"] = event["command"].clone();
    }
    if hook.handler == "deliver" {
        facts["file"] = event["file"].clone();
        facts["prompt"] = event["prompt"].clone();
        // A search the model ran, which the hook may answer from verified evidence.
        facts["pattern"] = event["pattern"].clone();
        if event["tool"] == "shell" {
            facts["command"] = event["command"].clone();
        }
        if event["stopActive"] == true {
            facts["stopActive"] = json!(true);
        }
    }
    facts
}

/// The context text a host receives; skill names use the host's native separator.
fn render(context: &Value, host: &str) -> String {
    match context["kind"].as_str() {
        _ if context.is_string() => context.as_str().unwrap().to_string(),
        Some("routing") => {
            let mut hint = craft_engine::delivery::shown(context)["data"].take();
            if let Some(skills) = hint.get_mut("skills").and_then(Value::as_array_mut) {
                let plugin = plugin_names(host);
                for skill in skills.iter_mut() {
                    if let Some(name) = skill.as_str() {
                        *skill = json!(craft_engine::skills::host_name(name, plugin));
                    }
                }
            }
            format!("Routing hints (no authorization implied; reuse until scope/config changes): {hint}")
        }
        Some("answer") => format!("Project Intelligence for the search just run (verified declaration and callers, complete coverage; no authorization implied): {}", context["data"]),
        Some("resume") => format!("Craft task records to resume from (saved by this or an earlier session; no authorization implied, recheck evidence when source changed): {}", context["data"]),
        Some("checkpoint") => format!("Checkpoint data; verify against the current tree before continuing: {}", context["data"]),
        Some("services") => format!("Configured service map (not live verification; reuse within this session, refresh on binding/tool/account changes or access failures): {}", context["data"]),
        Some("preserve") => "Preserve task scope, original authorization, evidence and next step in the project checkpoint. Compaction hooks do not replace milestone checkpoints.".into(),
        Some("unavailable") => format!("Routing/checkpoint unavailable: {}", context["data"].as_str().unwrap_or_default()),
        _ => String::new(),
    }
}

/// The host session identity, and unfinished tasks of earlier sessions it may adopt.
fn session_line(session: &Value) -> String {
    let identity = json!({"id": session["id"], "task": session["task"]});
    let mut line =
        format!("Craft session (pass as --session; no authorization implied): {identity}");
    if let Some(handoff) = session.get("handoff") {
        line.push_str(&format!("\nUnfinished tasks of earlier sessions in this worktree. Only if the user's request continues one, adopt it with `craft task-adopt --session <id> --from <from>`; records are copied and authorization is not renewed: {handoff}"));
    }
    if let Some(planning) = session.get("planning") {
        line.push_str(&format!("\nPlanning evidence of earlier sessions in this worktree, files unchanged since. A plan document, not a managed work plan: it holds no tasks and starts no execution: {planning}"));
    }
    if let Some(plans) = session.get("plans") {
        line.push_str(&format!("\nOpen work plans in this worktree. Only if the user's request continues one, start a ready task with `craft task-start --session <id> --plan <plan> --task <task>`; a task held in another session (here: false) needs --take-over, as that session may still be working: {plans}"));
    }
    line
}

/// Declared standards the file or commit just written does not meet.
fn standards_line(found: &Value) -> String {
    format!(
        "Project standards not met by what was just written (deterministic anchor check; report only, fix or tell the user why not): {found}"
    )
}

/// The stop summary a held stop gives the model as the reason to continue.
fn stop_line(summary: &Value) -> String {
    format!(
        "Craft stop review of the files this session changed (deterministic findings against HEAD and the project's review items; no authorization implied). Address each before reporting the work done: fix it, or tell the user why it stays: {summary}"
    )
}

/// Whether `host` can hold a stop and continue the turn with a reason.
pub(crate) fn continues_turn(host: &str) -> bool {
    mapping(host, "turn-end").is_some_and(|m| m["continue"] == true)
}

/// At most 12 000 UTF-16 units of context, cut at a character boundary.
fn bounded(context: String) -> String {
    if js_length(&context) <= MAX_CONTEXT {
        return context;
    }
    let mut units = 0;
    let end = context
        .char_indices()
        .find(|(_, c)| {
            units += c.len_utf16();
            units > MAX_CONTEXT
        })
        .map_or(context.len(), |(index, _)| index);
    format!(
        "{}\n[truncated; use the runtime checkpoint/route CLI for full data]",
        &context[..end]
    )
}

/// The host wire response for a decision.
fn serialize(result: &Value, host: &str, lifecycle: &str) -> Wire {
    let empty = Wire {
        stdout: String::new(),
        stderr: String::new(),
        exit_code: 0,
    };
    let Some(mapping) = mapping(host, lifecycle) else {
        return empty;
    };
    let denied = result["decision"] == "deny" && mapping["deny"] == true;
    let reason = &result["reason"];
    let context = if mapping["context"] == true {
        let parts = [
            result.get("session").map(session_line),
            result.get("context").map(|c| render(c, host)),
            result.get("standards").map(standards_line),
        ];
        bounded(
            parts
                .into_iter()
                .flatten()
                .filter(|part| !part.is_empty())
                .collect::<Vec<_>>()
                .join("\n"),
        )
    } else {
        String::new()
    };
    if host == "claude" && denied {
        return Wire {
            stdout: String::new(),
            stderr: format!("{}\n", reason.as_str().unwrap_or_default()),
            exit_code: 2,
        };
    }
    let name = &mapping["name"];
    if let Some(summary) = result
        .get("stop")
        .filter(|_| lifecycle == "turn-end" && mapping["continue"] == true)
    {
        let reason = bounded(stop_line(summary));
        let output = match host {
            "cursor" => json!({"followup_message": reason}),
            "gemini" => json!({"decision": "deny", "reason": reason}),
            _ => json!({"decision": "block", "reason": reason}),
        };
        return Wire {
            stdout: format!("{output}\n"),
            ..empty
        };
    }
    let output = match host {
        "cursor" => match lifecycle {
            "before-tool" if denied => {
                Some(json!({"permission": "deny", "user_message": reason, "agent_message": reason}))
            }
            // Cursor blocks a preToolUse answer without `permission` and has no neutral value;
            // it treats `allow` as advisory, so the user's allowlist and approvals still apply.
            "before-tool" => Some(json!({"permission": "allow"})),
            "before-prompt" => Some(json!({"continue": true})),
            _ => (!context.is_empty()).then(|| json!({"additional_context": context})),
        },
        // Empty output keeps Copilot's own permission flow; Craft grants nothing.
        "copilot" if lifecycle == "before-tool" => denied
            .then(|| json!({"permissionDecision": "deny", "permissionDecisionReason": reason})),
        "copilot" => (!context.is_empty()).then(|| json!({"additionalContext": context})),
        "gemini" if denied => Some(json!({"decision": "deny", "reason": reason})),
        "gemini" => (!context.is_empty()).then(
            || json!({"hookSpecificOutput": {"hookEventName": name, "additionalContext": context}}),
        ),
        _ if denied => Some(
            json!({"hookSpecificOutput": {"hookEventName": name, "permissionDecision": "deny", "permissionDecisionReason": reason}}),
        ),
        _ => (!context.is_empty()).then(
            || json!({"hookSpecificOutput": {"hookEventName": name, "additionalContext": context}}),
        ),
    };
    Wire {
        stdout: output.map_or_else(String::new, |o| format!("{o}\n")),
        ..empty
    }
}

/// Run one hook event. The export is posted after the wire output is ready.
pub fn run(args: &[String], input: &mut dyn Read) -> (Wire, Option<Value>) {
    let hook = match arguments(args) {
        Ok(hook) => hook,
        Err(message) => {
            return (
                Wire {
                    stdout: String::new(),
                    stderr: format!("{message}\n"),
                    exit_code: 1,
                },
                None,
            );
        }
    };
    let (payload, status, input_error) = read_payload(input);
    let observational = hook.handler == "observe";
    let enforcement = hook.shell && hook.lifecycle == "before-tool";
    let mut result = host::run(&facts(
        &hook,
        &normalize(&payload, &hook.host, &hook.lifecycle),
        status,
    ))
    .unwrap_or_else(|message| {
        // Without a runtime decision, shell enforcement still fails closed and delivery says why.
        if enforcement {
            craft_engine::delivery::failure(true, &message)
        } else if observational {
            json!({"decision": "allow"})
        } else {
            craft_engine::delivery::failure(false, &message)
        }
    });
    let export = result
        .as_object_mut()
        .and_then(|r| r.remove("export"))
        .filter(|e| !e.is_null());
    if let Some(error) = input_error.filter(|_| !observational && result["decision"] != "deny") {
        return (
            Wire {
                stdout: String::new(),
                stderr: format!("{error}\n"),
                exit_code: 1,
            },
            export,
        );
    }
    if observational {
        return (
            Wire {
                stdout: String::new(),
                stderr: String::new(),
                exit_code: 0,
            },
            export,
        );
    }
    let mut wire = serialize(&result, &hook.host, &hook.lifecycle);
    if let Some(warning) = result["warning"].as_str() {
        wire.stderr.push_str(&format!("{warning}\n"));
    }
    (wire, export)
}

/// The `craft hook` process: write the wire response, then post the telemetry export.
pub fn main(args: &[String]) -> i32 {
    let (wire, export) = run(args, &mut std::io::stdin().lock());
    let mut stdout = std::io::stdout().lock();
    let _ = stdout.write_all(wire.stdout.as_bytes());
    let _ = stdout.flush();
    let _ = std::io::stderr().write_all(wire.stderr.as_bytes());
    Recorder::deliver(export);
    wire.exit_code
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The frozen host corpus: payload normalization and wire serialization per host.
    #[test]
    fn frozen_host_contracts() {
        let corpus: Value = serde_json::from_str(include_str!(
            "../../tests/fixtures/runtime/host-contracts.json"
        ))
        .unwrap();
        for case in corpus["cases"].as_array().unwrap() {
            let host = case["host"].as_str().unwrap();
            let lifecycle = case["definition"]["event"].as_str().unwrap();
            let actual = if case["operation"] == "normalize" {
                normalize(&case["input"], host, lifecycle)
            } else {
                let wire = serialize(&case["input"], host, lifecycle);
                json!({"output": wire.stdout, "error": wire.stderr, "exitCode": wire.exit_code})
            };
            assert_eq!(
                actual.to_string(),
                case["response"].to_string(),
                "{}",
                case["id"]
            );
        }
    }

    /// Structured contexts rendered as hosts receive them, incl. truncation.
    #[test]
    fn frozen_context_rendering() {
        let corpus: Value = serde_json::from_str(include_str!(
            "../../tests/fixtures/runtime/render-contracts.json"
        ))
        .unwrap();
        let mut different = Vec::new();
        // JavaScript renders an absent `data` as "undefined"; JSON input cannot omit it
        // into anything but null, so those captured cases are unreachable.
        let reachable = |case: &&Value| {
            !case["id"]
                .as_str()
                .unwrap()
                .starts_with("services-missing:")
        };
        for case in corpus["render"]
            .as_array()
            .unwrap()
            .iter()
            .filter(reachable)
        {
            let actual = json!(render(&case["context"], case["host"].as_str().unwrap()));
            if actual != case["expected"] {
                different.push(format!("{} {actual} {}", case["id"], case["expected"]));
            }
        }
        for case in corpus["wire"].as_array().unwrap().iter().filter(reachable) {
            let wire = serialize(
                &case["result"],
                case["host"].as_str().unwrap(),
                case["event"].as_str().unwrap(),
            );
            let actual =
                json!({"output": wire.stdout, "error": wire.stderr, "exitCode": wire.exit_code});
            if actual != case["expected"] {
                different.push(format!("{} {actual} {}", case["id"], case["expected"]));
            }
        }
        assert!(different.is_empty(), "{}", different.join("\n"));
    }

    /// JavaScript cuts at 12 000 UTF-16 units and may keep half a surrogate pair; the
    /// native cut keeps whole characters.
    #[test]
    fn truncated_context_keeps_whole_characters() {
        let context = format!("{}\u{1f642}tail", "x".repeat(MAX_CONTEXT - 1));
        let cut = bounded(context);
        assert!(cut.starts_with(&format!("{}\n[truncated;", "x".repeat(MAX_CONTEXT - 1))));
        assert_eq!(bounded("y".repeat(MAX_CONTEXT)), "y".repeat(MAX_CONTEXT));
    }

    /// A stop summary holds the stop in each host's own words, only where it can continue.
    #[test]
    fn a_stop_summary_holds_the_stop_where_the_host_continues() {
        let result = json!({"decision": "allow", "stop": {"findings": [{"rule": "todo-marker"}]}});
        let reason = |output: &Value, field: &str| {
            output[field]
                .as_str()
                .unwrap()
                .starts_with("Craft stop review")
        };
        for host in ["claude", "codex", "copilot"] {
            let wire = serialize(&result, host, "turn-end");
            let output: Value = serde_json::from_str(&wire.stdout).unwrap();
            assert_eq!(output["decision"], "block", "{host}");
            assert!(reason(&output, "reason"), "{host}");
        }
        let gemini: Value =
            serde_json::from_str(&serialize(&result, "gemini", "turn-end").stdout).unwrap();
        assert_eq!(gemini["decision"], "deny");
        assert!(reason(&gemini, "reason"));
        let cursor: Value =
            serde_json::from_str(&serialize(&result, "cursor", "turn-end").stdout).unwrap();
        assert!(reason(&cursor, "followup_message"));
        // No stop event, or another event: nothing to hold.
        assert_eq!(serialize(&result, "jetbrains", "turn-end").stdout, "");
        assert_eq!(serialize(&result, "claude", "after-tool").stdout, "");
        assert_eq!(
            serialize(&json!({"decision": "allow"}), "claude", "turn-end").stdout,
            ""
        );
    }

    #[test]
    fn a_continued_stop_is_marked_active() {
        let active =
            |payload: Value| normalize(&payload, "claude", "turn-end")["stopActive"] == true;
        assert!(active(json!({"stop_hook_active": true})));
        assert!(active(json!({"loop_count": 1})));
        assert!(!active(json!({"stop_hook_active": false, "loop_count": 0})));
        assert!(!active(json!({})));
    }

    #[test]
    fn lone_surrogate_escapes_become_replacement_characters() {
        let parse = |text: &str| serde_json::from_str::<Value>(&repair_surrogates(text)).unwrap();
        assert_eq!(parse(r#""a\ud800b""#), json!("a\u{fffd}b"));
        assert_eq!(parse(r#""\udc00""#), json!("\u{fffd}"));
        assert_eq!(parse(r#""\ud83d\ude00""#), json!("\u{1f600}"));
        // An escaped backslash starts no escape sequence.
        assert_eq!(parse(r#""\\ud800""#), json!("\\ud800"));
        assert_eq!(
            parse(r#"{"k":"\ud800\ud800"}"#),
            json!({"k": "\u{fffd}\u{fffd}"})
        );
    }
}
