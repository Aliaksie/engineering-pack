//! `craft hook <host> <lifecycle> --package <name> --handler <handler> [--scope shell]`:
//! one host hook event from the host's own payload on stdin to its wire output. The host
//! mapping table is the data the build uses to register hooks.
use crate::host;
use crate::telemetry::Recorder;
use craft_engine::delivery::budget::{self, Piece};
use craft_engine::session::test_runs;
use craft_engine::values::js_length;
use craft_platform::files;
use serde_json::{Value, json};
use std::io::{Read, Write};
use std::path::Path;

const MAX_INPUT: usize = 65_536;
/// The ceiling for a host whose table states none.
const CONTEXT_BYTES: usize = 9_000;
/// How much of a long test output's end is read for the runner's summary.
const RUN_TAIL_BYTES: usize = 65_536;
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

/// The most UTF-8 bytes of context one hook answer gives `host`: under the cap past which the
/// host replaces or cuts the answer.
fn ceiling(host: &str) -> usize {
    let table: Value = serde_json::from_str(HOSTS).expect("embedded host hook table");
    table[host]["contextBytes"]
        .as_u64()
        .map_or(CONTEXT_BYTES, |bytes| bytes as usize)
}

/// Whether `host` shows the user a message a hook returns at `lifecycle`.
pub(crate) fn shows_notice(host: &str, lifecycle: &str) -> bool {
    mapping(host, lifecycle).is_some_and(|m| m["notice"] == true)
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

/// How many files of one patch an edit event names: as many as a session's intent record
/// keeps, so every file a patch names is routed and stays in the session's scope.
const PATCH_FILES: usize = craft_engine::delivery::INTENT_FILES;

/// The files a patch in the `apply_patch` format writes, in order and once each: added,
/// updated and deleted files and the target of a move. Text without `*** Begin Patch`
/// names none.
fn patch_files(patch: &str) -> Vec<String> {
    if !patch.trim_start().starts_with("*** Begin Patch") {
        return Vec::new();
    }
    let mut files: Vec<String> = Vec::new();
    for line in patch.lines() {
        let path = [
            "*** Add File: ",
            "*** Update File: ",
            "*** Delete File: ",
            "*** Move to: ",
        ]
        .iter()
        .find_map(|header| line.strip_prefix(header))
        .map(str::trim)
        .filter(|path| !path.is_empty());
        if let Some(path) = path
            && !files.iter().any(|f| f == path)
        {
            files.push(path.to_string());
            if files.len() == PATCH_FILES {
                break;
            }
        }
    }
    files
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
    } else if event["tool"] == "edit"
        && let Some(files) = input["command"]
            .as_str()
            .map(patch_files)
            .filter(|files| !files.is_empty())
    {
        // Codex `apply_patch` passes the patch, not a path: the files are in its headers.
        event["file"] = json!(files[0]);
        event["files"] = json!(files);
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

/// The runner's verdict on a test command the host just ran, from the tool result it reports.
/// Hosts differ: Claude reports success and failure as separate events (the failure's
/// `error` starts with `Exit code N`) and keeps a long output in a file whose end holds the
/// summary; Codex passes the output text without an exit code. A host event declared as a
/// failure never passes; one without an exit code or a summary (a rejected or timed-out call)
/// stays `unknown`. An interrupted call, or one the host moved to the background, has not
/// finished and says nothing about the run.
fn test_run(payload: &Value, host: &str, lifecycle: &str, command: &str) -> Option<Value> {
    if !matches!(lifecycle, "after-tool" | "after-tool-failure")
        || !test_runs::is_test_command(command)
    {
        return None;
    }
    let response = first(
        payload,
        &["tool_response", "tool_output", "toolResult", "tool_result"],
    );
    let unfinished = payload["is_interrupt"] == true
        || payload["tool_input"]["run_in_background"] == true
        || response.is_some_and(|r| {
            r["interrupted"] == true || r.get("backgroundTaskId").is_some_and(|id| !id.is_null())
        });
    if unfinished {
        return None;
    }
    let outcome = mapping(host, lifecycle).and_then(|m| m["outcome"].as_str().map(str::to_owned));
    let mut exit = (outcome.as_deref() == Some("success")).then_some(0);
    let mut output = String::new();
    match response {
        Some(Value::String(text)) => output.clone_from(text),
        Some(Value::Object(response)) => {
            if let Some(path) = response.get("persistedOutputPath").and_then(Value::as_str)
                && let Ok(tail) = files::read_tail(Path::new(path), RUN_TAIL_BYTES)
            {
                output = String::from_utf8_lossy(&tail).into_owned();
            }
            if output.is_empty() {
                for key in [
                    "stdout",
                    "output",
                    "textResultForLlm",
                    "llmContent",
                    "stderr",
                ] {
                    if let Some(text) = response.get(key).and_then(Value::as_str) {
                        output.push_str(text);
                        output.push('\n');
                    }
                }
            }
            if let Some(code) = ["exit_code", "exitCode"]
                .iter()
                .find_map(|key| response.get(*key).and_then(Value::as_i64))
            {
                exit = Some(code);
            }
        }
        _ => {}
    }
    if let Some(error) = payload["error"].as_str() {
        let (head, rest) = error.split_once('\n').unwrap_or((error, ""));
        match head
            .strip_prefix("Exit code ")
            .and_then(|code| code.trim().parse().ok())
        {
            Some(code) => {
                exit = Some(code);
                output = rest.to_owned();
            }
            None => output = error.to_owned(),
        }
    }
    // A result with no output and no failing exit code (a command still running, or a host
    // that reports success without output) says nothing about the run.
    if output.trim().is_empty() && exit.is_none_or(|code| code == 0) {
        return None;
    }
    let mut verdict = test_runs::classify(exit, &output);
    if outcome.as_deref() == Some("failure") && verdict["result"] != "failed" {
        // A failed call never passes; without a summary or exit code it may not have run.
        if verdict["result"] == "passed" {
            verdict["result"] = json!("failed");
        }
        verdict["reason"] = json!("tool-failed");
    }
    Some(verdict)
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
        if let Some(files) = event.get("files") {
            facts["files"] = files.clone();
        }
        facts["prompt"] = event["prompt"].clone();
        // A search the model ran, which the hook may answer from verified evidence.
        facts["pattern"] = event["pattern"].clone();
        if event["tool"] == "shell" {
            facts["command"] = event["command"].clone();
        }
        if let Some(run) = event.get("testRun") {
            facts["testRun"] = run.clone();
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
        Some("resume") => format!("Craft task records to resume from (saved by this or an earlier session; no authorization implied, recheck evidence when source changed{}): {}", if context["data"].get("standards").is_some() { "; `standards` are the project standards of this scope, still in force" } else { "" }, context["data"]),
        Some("checkpoint") => format!("Checkpoint data; verify against the current tree before continuing{}: {}", if context["data"].get("standards").is_some() { "; `standards` are the project standards of this scope, still in force" } else { "" }, context["data"]),
        Some("services") => format!("Configured service map (not live verification; reuse within this session, refresh on binding/tool/account changes or access failures): {}", context["data"]),
        Some("preserve") => "Preserve task scope, original authorization, evidence and next step in the project checkpoint. Compaction hooks do not replace milestone checkpoints.".into(),
        Some("unavailable") => format!("Routing/checkpoint unavailable: {}", context["data"].as_str().unwrap_or_default()),
        _ => String::new(),
    }
}

/// Names of the context pieces whose delivery a session counts.
pub(crate) const ANSWER: &str = "lookup answer";
pub(crate) const ROUTING: &str = "routing hints";
pub(crate) const RESUME: &str = "task records";
pub(crate) const CHECKPOINT: &str = "checkpoint";
pub(crate) const STANDARDS: &str = "standards findings";
pub(crate) const TESTS: &str = "test evidence";

/// The host session identity, then what earlier sessions left that it may adopt: each a
/// piece of its own, so a long list never costs the identity.
fn session_pieces(session: &Value, pieces: &mut Vec<Piece>) {
    let identity = json!({"id": session["id"], "task": session["task"]});
    let mut push = |name, rank, text| pieces.push(Piece { name, rank, text });
    push(
        "session identity",
        0,
        format!("Craft session (pass as --session; no authorization implied): {identity}"),
    );
    if let Some(handoff) = session.get("handoff") {
        push(
            "unfinished tasks of earlier sessions",
            6,
            format!(
                "Unfinished tasks of earlier sessions in this worktree. Only if the user's request continues one, adopt it with `craft task-adopt --session <id> --from <from>`; records are copied and authorization is not renewed: {handoff}"
            ),
        );
    }
    if let Some(planning) = session.get("planning") {
        push(
            "planning evidence of earlier sessions",
            7,
            format!(
                "Planning evidence of earlier sessions in this worktree, files unchanged since. A plan document, not a managed work plan: it holds no tasks and starts no execution: {planning}"
            ),
        );
    }
    if let Some(plans) = session.get("plans") {
        push(
            "open work plans",
            5,
            format!(
                "Open work plans in this worktree. Only if the user's request continues one, start a ready task with `craft task-start --session <id> --plan <plan> --task <task>`; a task held in another session (here: false) needs --take-over, as that session may still be working: {plans}"
            ),
        );
    }
}

/// A decision's context in the pieces a host is given, in the order it reads them. What the
/// model must not lose ranks lowest: who the session is, the task it resumes, a test run that
/// ran nothing; lists of what earlier sessions left rank highest and go first.
fn pieces(result: &Value, host: &str) -> Vec<Piece> {
    let mut pieces = Vec::new();
    if let Some(session) = result.get("session") {
        session_pieces(session, &mut pieces);
    }
    if let Some(context) = result.get("context") {
        let (name, rank) = match context["kind"].as_str() {
            Some("resume") => (RESUME, 1),
            Some("answer") => (ANSWER, 2),
            Some("routing") => (ROUTING, 3),
            Some("checkpoint") => (CHECKPOINT, 3),
            Some("services") => ("service map", 6),
            _ => ("context note", 1),
        };
        pieces.push(Piece {
            name,
            rank,
            text: render(context, host),
        });
    }
    let mut push = |name, rank, text| pieces.push(Piece { name, rank, text });
    if let Some(found) = result.get("standards") {
        push(STANDARDS, 2, standards_line(found));
    }
    if let Some(delegation) = result.get("delegation") {
        push("delegated standards", 2, delegation_line(delegation));
    }
    if let Some(run) = result.get("tests") {
        push(TESTS, 1, tests_line(run));
    }
    pieces
}

/// Declared standards the file or commit just written does not meet.
fn standards_line(found: &Value) -> String {
    format!(
        "Project standards not met by what was just written (deterministic anchor check; report only, fix or tell the user why not): {found}"
    )
}

/// A test run that exited cleanly without running a test.
fn tests_line(run: &Value) -> String {
    let summary = json!({
        "runner": run["runner"], "reason": run["reason"], "executed": run["executed"],
        "skipped": run["skipped"], "command": run["command"],
    });
    format!(
        "Craft test evidence: the test command just run executed no test, so it verifies nothing (a filter or skip matched everything). Run tests that exercise the change before reporting them as passing: {summary}"
    )
}

/// The standards a worker the host started inherits from the session that delegated to it.
fn delegation_line(delegation: &Value) -> String {
    format!(
        "Project standards of the work this task was delegated from (named by the project; no authorization implied; the delegating instructions define your task and scope): {delegation}"
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

/// What a decision gives the host at one event, within the host's output ceiling.
pub(crate) struct Shaped {
    /// The context text; empty where the host shows none at this event or the call is denied.
    pub context: String,
    /// Names of the context pieces delivered and of those left out to fit.
    pub kept: Vec<&'static str>,
    pub dropped: Vec<&'static str>,
    /// The reason a held stop continues the turn with.
    pub stop: Option<String>,
}

impl Shaped {
    /// Bytes the model receives from this hook answer.
    pub fn bytes(&self) -> usize {
        self.context.len() + self.stop.as_ref().map_or(0, String::len)
    }
}

/// The context and stop reason `result` gives `host` at `lifecycle`.
pub(crate) fn shaped(result: &Value, host: &str, lifecycle: &str) -> Shaped {
    let mut shaped = Shaped {
        context: String::new(),
        kept: Vec::new(),
        dropped: Vec::new(),
        stop: None,
    };
    let Some(mapping) = mapping(host, lifecycle) else {
        return shaped;
    };
    let denied = result["decision"] == "deny" && mapping["deny"] == true;
    if mapping["context"] == true && !denied {
        let pieces = pieces(result, host);
        let named: Vec<&'static str> = pieces
            .iter()
            .filter(|piece| !piece.text.is_empty())
            .map(|piece| piece.name)
            .collect();
        let fitted = budget::fit(pieces, ceiling(host));
        shaped.kept = named
            .into_iter()
            .filter(|name| !fitted.dropped.contains(name))
            .collect();
        shaped.context = fitted.text;
        shaped.dropped = fitted.dropped;
    }
    if let Some(summary) = result
        .get("stop")
        .filter(|_| lifecycle == "turn-end" && mapping["continue"] == true && !denied)
    {
        let piece = Piece {
            name: "stop review",
            rank: 0,
            text: stop_line(summary),
        };
        shaped.stop = Some(budget::fit(vec![piece], ceiling(host)).text);
    }
    shaped
}

/// The answer of a hook with nothing to say, where the host needs one. Cursor blocks a
/// permission hook whose answer does not match its schema and has no neutral value; it treats
/// `allow` as advisory, so the user's allowlist and approvals still apply. Other hosts keep
/// their own flow on empty output.
fn no_opinion(host: &str, lifecycle: &str) -> Option<Value> {
    match (host, lifecycle) {
        ("cursor", "before-tool") => Some(json!({"permission": "allow"})),
        ("cursor", "before-prompt") => Some(json!({"continue": true})),
        _ => None,
    }
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
    let Shaped { context, stop, .. } = shaped(result, host, lifecycle);
    if host == "claude" && denied {
        return Wire {
            stdout: String::new(),
            stderr: format!("{}\n", reason.as_str().unwrap_or_default()),
            exit_code: 2,
        };
    }
    let name = &mapping["name"];
    if let Some(reason) = stop {
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
            _ => no_opinion(host, lifecycle)
                .or_else(|| (!context.is_empty()).then(|| json!({"additional_context": context}))),
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
    // One line for the user on what Craft did, where the host shows a hook's message.
    let notice = result["notice"]
        .as_str()
        .filter(|_| mapping["notice"] == true);
    let output = match (output, notice) {
        (Some(mut output), Some(notice)) => {
            output["systemMessage"] = json!(notice);
            Some(output)
        }
        (None, Some(notice)) => Some(json!({"systemMessage": notice})),
        (output, None) => output,
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
    let mut event = normalize(&payload, &hook.host, &hook.lifecycle);
    // Only delivery records test runs; other packages' hooks do not read the result.
    if hook.handler == "deliver"
        && event["tool"] == "shell"
        && let Some(verdict) = event["command"]
            .as_str()
            .and_then(|command| test_run(&payload, &hook.host, &hook.lifecycle, command))
    {
        event["testRun"] = verdict;
    }
    let mut result = host::run(&facts(&hook, &event, status)).unwrap_or_else(|message| {
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
        let answer = mapping(&hook.host, &hook.lifecycle)
            .and_then(|_| no_opinion(&hook.host, &hook.lifecycle));
        return (
            Wire {
                stdout: answer.map_or_else(String::new, |a| format!("{a}\n")),
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

    /// Codex `apply_patch` passes the patch text (openai/codex
    /// `core/src/tools/handlers/apply_patch.rs`); the files come from its headers.
    #[test]
    fn a_patch_edit_names_every_file_it_writes() {
        let patch = "*** Begin Patch\n*** Update File: src/a.rs\n@@\n-x\n+y\n*** Add File: docs/b.md\n+b\n*** Delete File: old.rs\n*** Update File: src/c.rs\n*** Move to: src/d.rs\n@@\n-x\n+y\n*** Update File: src/a.rs\n*** End Patch";
        let payload = json!({"session_id": "S", "cwd": "/r", "tool_name": "apply_patch",
            "tool_input": {"command": patch}});
        let event = normalize(&payload, "codex", "after-tool");
        assert_eq!(event["tool"], "edit");
        assert_eq!(event["file"], "src/a.rs");
        assert_eq!(
            event["files"],
            json!(["src/a.rs", "docs/b.md", "old.rs", "src/c.rs", "src/d.rs"])
        );
        // A path-carrying edit keeps its one file; text that is not a patch names none.
        let path =
            json!({"tool_name": "Edit", "tool_input": {"file_path": "x.rs", "command": patch}});
        let event = normalize(&path, "claude", "after-tool");
        assert_eq!(
            (event["file"].as_str(), event.get("files")),
            (Some("x.rs"), None)
        );
        let prose =
            json!({"tool_name": "apply_patch", "tool_input": {"command": "*** Update File: a.rs"}});
        assert!(
            normalize(&prose, "codex", "after-tool")
                .get("file")
                .is_none()
        );
        let many: String = (0..100)
            .map(|i| format!("*** Add File: f{i}.rs\n"))
            .collect();
        let many = json!({"tool_name": "apply_patch", "tool_input": {"command": format!("*** Begin Patch\n{many}")}});
        assert_eq!(
            normalize(&many, "codex", "after-tool")["files"]
                .as_array()
                .unwrap()
                .len(),
            PATCH_FILES
        );
    }

    /// Payloads a released Codex sent to Craft's hooks, recorded live: the tool calls a code
    /// mode `exec` script makes reach hooks as `apply_patch` and `Bash`, with absolute paths.
    #[test]
    fn observed_codex_payloads_name_their_tool_and_files() {
        let observed: Value = serde_json::from_str(include_str!(
            "../../tests/fixtures/hosts/codex-observed.json"
        ))
        .unwrap();
        for case in observed["cases"].as_array().unwrap() {
            let lifecycle = case["lifecycle"].as_str().unwrap();
            let event = normalize(&case["payload"], "codex", lifecycle);
            let expect = &case["expect"];
            assert_eq!(event["tool"], expect["tool"], "{}", case["id"]);
            for key in ["file", "files"] {
                if let Some(value) = expect.get(key) {
                    assert_eq!(&event[key], value, "{} {key}", case["id"]);
                }
            }
            assert_eq!(
                event["session"], case["payload"]["session_id"],
                "{}",
                case["id"]
            );
        }
    }

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

    /// A session start that would pass the host's ceiling keeps who the session is and the
    /// task it resumes, leaves out whole lists of what earlier sessions left and says which.
    #[test]
    fn a_context_over_the_ceiling_loses_whole_low_priority_pieces() {
        let list = |name: &str| json!([{"name": name, "notes": "n".repeat(3_500)}]);
        let result = json!({"decision": "allow",
            "session": {"id": "S1", "task": "T1", "handoff": list("handoff"),
                "planning": list("planning"), "plans": list("plans")},
            "context": {"kind": "resume", "data": {"task": "T1", "next": "r".repeat(2_000)}}});
        for host in ["claude", "codex", "copilot", "cursor", "gemini"] {
            let shaped = shaped(&result, host, "session-start");
            assert!(shaped.context.len() <= ceiling(host), "{host}");
            assert_eq!(
                shaped.dropped,
                [
                    "unfinished tasks of earlier sessions",
                    "planning evidence of earlier sessions"
                ],
                "{host}"
            );
            assert_eq!(
                shaped.kept,
                ["session identity", "open work plans", RESUME],
                "{host}"
            );
            assert!(
                shaped
                    .context
                    .starts_with("Craft session (pass as --session")
            );
            assert!(shaped.context.ends_with(
                "[Craft left out, to stay within the host's hook output limit: unfinished tasks of earlier sessions, planning evidence of earlier sessions]"
            ));
            // The wire carries exactly this text, and the same bytes every time.
            let wire = serialize(&result, host, "session-start").stdout;
            assert!(wire.contains("Craft left out") && !wire.contains("[truncated"));
            assert_eq!(wire, serialize(&result, host, "session-start").stdout);
        }
        // Within the ceiling nothing is left out or added.
        let small = json!({"decision": "allow", "session": {"id": "S1", "task": null,
            "plans": [{"name": "p"}]}});
        let shaped = shaped(&small, "claude", "session-start");
        assert!(shaped.dropped.is_empty() && !shaped.context.contains("left out"));
    }

    /// The summary line reaches the user as the host's own message field, only at an event
    /// where the host shows one, and never beside a held stop.
    #[test]
    fn the_session_summary_is_a_user_message_where_the_host_shows_one() {
        let result = json!({"decision": "allow", "notice": "Craft this session: 1 lookup"});
        for host in ["claude", "codex", "gemini"] {
            let wire = serialize(&result, host, "turn-end").stdout;
            assert_eq!(
                wire, "{\"systemMessage\":\"Craft this session: 1 lookup\"}\n",
                "{host}"
            );
        }
        for host in ["cursor", "copilot", "jetbrains"] {
            assert_eq!(serialize(&result, host, "turn-end").stdout, "", "{host}");
        }
        assert_eq!(serialize(&result, "claude", "after-tool").stdout, "");
        let held =
            json!({"decision": "allow", "notice": "n", "stop": {"findings": [{"rule": "r"}]}});
        assert!(
            !serialize(&held, "claude", "turn-end")
                .stdout
                .contains("systemMessage")
        );
    }

    /// Delegated standards reach a host-started subagent only where its start event takes
    /// context; the answer never blocks the start.
    #[test]
    fn delegated_standards_answer_the_subagent_start_in_each_host_wire() {
        let result = json!({"decision": "allow", "delegation": {"intent": "story", "standards": [{"id": "stories"}]}});
        let output = |host: &str| -> Value {
            let wire = serialize(&result, host, "agent-start");
            serde_json::from_str(&wire.stdout).unwrap_or(Value::Null)
        };
        for host in ["claude", "codex"] {
            let answer = &output(host)["hookSpecificOutput"];
            assert_eq!(answer["hookEventName"], "SubagentStart");
            let context = answer["additionalContext"].as_str().unwrap();
            assert!(context.contains("\"stories\"") && context.contains("delegated from"));
            assert!(answer.get("permissionDecision").is_none());
        }
        assert!(
            output("copilot")["additionalContext"]
                .as_str()
                .unwrap()
                .contains("\"stories\"")
        );
        for host in ["cursor", "gemini", "jetbrains"] {
            assert_eq!(serialize(&result, host, "agent-start").stdout, "");
        }
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
