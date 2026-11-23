//! `craft hook <host> <lifecycle> --package <name> --handler <handler> [--scope shell]`:
//! one host hook event from the host's own payload on stdin to its wire output. The host
//! mapping table is the data the build uses to register hooks.
use crate::telemetry::Recorder;
use craft_engine::delivery::budget::{self, Piece};
use craft_engine::session::test_runs;
use craft_engine::values::js_length;
use craft_platform::files;
use serde_json::{Value, json};
use std::io::{Read, Write};
use std::path::Path;

pub(crate) mod delivered;
pub(crate) mod delivery;
pub mod host;
mod safety;
pub(crate) mod stop_review;
pub(crate) mod text;

const MAX_INPUT: usize = 1024 * 1024;
/// How much of an unread input is searched for the tool it names.
const TOOL_PEEK_BYTES: usize = 8 * 1024;
/// The ceiling for a host whose table states none.
const CONTEXT_BYTES: usize = 9_000;
/// How much of a long test output's end is read for the runner's summary.
const RUN_TAIL_BYTES: usize = 65_536;
const HOSTS: &str = include_str!("../../hosts/hooks.json");
const PLANNING: &str = include_str!("../../hosts/planning.json");
/// How many answered questions of one question tool call are read, and how long each text.
const ANSWERS: usize = 8;
const ANSWER_CHARS: usize = 2048;

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

/// The directory `host` was opened in, where it names one to its hooks in the environment:
/// the shell's directory in the payload moves with the session, this one does not.
fn project_directory(host: &str) -> Option<String> {
    let table: Value = serde_json::from_str(HOSTS).expect("embedded host hook table");
    let variable = table[host]["projectDirectory"].as_str()?;
    std::env::var(variable)
        .ok()
        .filter(|directory| std::path::Path::new(directory).is_absolute())
}

/// The host's own tool for a planning `responsibility` (`question`, `approval`, `tasks`) when
/// the planning table makes the host's path the active one; otherwise Craft's own path is.
pub(crate) fn host_path(host: &str, responsibility: &str) -> Option<String> {
    let table: Value = serde_json::from_str(PLANNING).expect("embedded host planning table");
    let entry = &table[host][responsibility];
    (entry["path"] == "host")
        .then(|| entry["tool"].as_str().map(str::to_owned))
        .flatten()
}

/// The tool `host` gives the model to ask the user a question with choices, where it has one.
pub(crate) fn question_tool(host: &str) -> Option<String> {
    host_path(host, "question")
}

/// The questions a host's approval tool asked and the answers the user chose, as
/// `[{question, answer}]`, from the payload after it ran: Claude's `AskUserQuestion` reports
/// `answers` as an object from each question to the chosen label. Bounded in count and length.
fn answers(payload: &Value) -> Option<Value> {
    let response = first(payload, &["tool_response", "tool_output", "toolResult"])?;
    let pairs: Vec<Value> = response["answers"]
        .as_object()?
        .iter()
        .filter_map(|(question, answer)| Some((question, answer.as_str()?)))
        .filter(|(question, answer)| {
            js_length(question) <= ANSWER_CHARS && js_length(answer) <= ANSWER_CHARS
        })
        .take(ANSWERS)
        .map(|(question, answer)| json!({"question": question, "answer": answer}))
        .collect();
    (!pairs.is_empty()).then(|| json!(pairs))
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

/// The tool and working directory an unread input states near its start (`"tool_name":
/// "Bash"`, `"cwd"`). Hosts send them before the tool's input and result, so a payload too
/// large to read still says where it runs and whether a shell command is about to. Each value
/// is decoded as the JSON string it is (a Windows directory arrives with escaped backslashes);
/// one that does not end within the bytes looked at is left out.
fn peeked(bytes: &[u8]) -> Value {
    // Never the session: partial input does not claim one.
    const FIELDS: [&str; 3] = ["tool_name", "toolName", "cwd"];
    let text = String::from_utf8_lossy(&bytes[..bytes.len().min(TOOL_PEEK_BYTES)]);
    let mut found = serde_json::Map::new();
    for field in FIELDS {
        let key = format!("\"{field}\"");
        let value = text.find(&key).and_then(|at| {
            let rest = text[at + key.len()..].trim_start().strip_prefix(':')?;
            let literal = rest.trim_start();
            let body = literal.strip_prefix('"')?;
            // The closing quote is the first one no backslash escapes.
            let (mut end, mut escaped) = (None, false);
            for (index, character) in body.char_indices() {
                match character {
                    _ if escaped => escaped = false,
                    '\\' => escaped = true,
                    '"' => {
                        end = Some(index);
                        break;
                    }
                    _ => {}
                }
            }
            serde_json::from_str::<String>(&literal[..end? + 2]).ok()
        });
        if let Some(value) = value {
            found.insert(field.to_owned(), json!(value));
        }
    }
    Value::Object(found)
}

/// `{payload, inputStatus, inputError}` as the host delivered it, bounded to 1 MiB. An
/// oversized payload keeps only the plain fields it starts with.
fn read_payload(input: &mut dyn Read) -> (Value, &'static str, Option<String>) {
    let mut bytes = Vec::new();
    match input.take(MAX_INPUT as u64 + 1).read_to_end(&mut bytes) {
        Err(error) => (json!({}), "read-failure", Some(error.to_string())),
        Ok(_) if bytes.len() > MAX_INPUT => (
            peeked(&bytes),
            "oversized",
            Some("hook input exceeds 1 MiB".into()),
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

/// How many files of one tool call the safety policy judges.
const NAMED_FILES: usize = 64;

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

/// The normalized event alone, as the frozen host contracts record it.
#[cfg(test)]
fn normalize(payload: &Value, host: &str, lifecycle: &str) -> Value {
    normalized(payload, host, lifecycle).0
}

/// The host-neutral event in a host payload: `{event, host, tool, contextSupported, cwd,
/// command, file?, prompt, session?}`. Beside it, what a tool call names that no built-in
/// name marks secret (`{files, filters}`, or null): judged against the patterns a
/// configuration layer declares once settings are loaded.
fn normalized(payload: &Value, host: &str, lifecycle: &str) -> (Value, Value) {
    let mut unjudged = Value::Null;
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
    // The first file or search filter a tool call names that reaches secret files, judged
    // by the built-in names alone: `Read`/`view`/`read_file`, a search's `path` and `glob` or
    // `include`, Gemini `read_many_files`, notebooks and the files of a patch.
    if lifecycle == "before-tool" && event["tool"] != "shell" {
        let patch = event["files"].as_array().into_iter().flatten();
        let named: Vec<&str> = ["file_path", "path", "absolute_path", "notebook_path"]
            .iter()
            .map(|key| &input[*key])
            .chain(input["paths"].as_array().into_iter().flatten())
            .chain(patch)
            .filter_map(Value::as_str)
            .take(NAMED_FILES)
            .collect();
        let filters: Vec<&str> = ["glob", "include"]
            .iter()
            .filter_map(|key| input[*key].as_str())
            .collect();
        if let Some(secret) = craft_engine::safety::named_secret(&named, &filters) {
            event["secret"] = json!(secret);
        } else if !named.is_empty() || !filters.is_empty() {
            unjudged = json!({"files": named, "filters": filters});
        }
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
    (event, unjudged)
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
    let mut verdict = test_runs::classify_command(exit, &output, command);
    if outcome.as_deref() == Some("failure") && verdict["result"] != "failed" {
        // A failed call never passes; without a summary or exit code it may not have run.
        if verdict["result"] == "passed" {
            verdict["result"] = json!("failed");
        }
        verdict["reason"] = json!("tool-failed");
    }
    Some(verdict)
}

/// Facts for the native `host` handler; shell-scoped hooks, delivery and any hook before a
/// shell command (which the safety policy judges) receive the command, and only delivery the
/// file, prompt and search pattern.
fn facts(hook: &Hook, event: &Value, named: &Value, status: &str) -> Value {
    let mut facts = json!({
        "handler": hook.handler, "host": hook.host, "package": hook.package, "lifecycle": hook.lifecycle,
        "scope": if hook.shell { "shell" } else { "any" }, "inputStatus": status, "tool": event["tool"],
        "contextSupported": event["contextSupported"], "cwd": event["cwd"], "session": event["session"],
    });
    if let Some(directory) = project_directory(&hook.host) {
        facts["projectDirectory"] = json!(directory);
    }
    if hook.shell || hook.lifecycle == "before-tool" && event["tool"] == "shell" {
        facts["command"] = event["command"].clone();
    }
    if let Some(secret) = event.get("secret") {
        facts["secret"] = secret.clone();
    }
    if !named.is_null() {
        facts["named"] = named.clone();
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
        if let Some(answers) = event.get("answers") {
            facts["answers"] = answers.clone();
        }
        if event["stopActive"] == true {
            facts["stopActive"] = json!(true);
        }
    }
    facts
}

/// The context text a host receives; skill names use the host's native separator.
fn render(context: &Value, host: &str) -> String {
    let data = &context["data"];
    // Resumed records that carry the scope's standards say they still apply.
    let in_force = if data.get("standards").is_some() {
        text::IN_FORCE.text()
    } else {
        ""
    };
    match context["kind"].as_str() {
        _ if context.is_string() => context.as_str().unwrap().to_string(),
        Some("routing") => {
            let plugin = plugin_names(host);
            let skill = |name: &str| craft_engine::skills::host_name(name, plugin);
            let hint =
                craft_engine::delivery::hint_lines(&craft_engine::delivery::shown(context), &skill);
            text::ROUTING.render(&[("hint", &hint)])
        }
        Some("answer") => text::ANSWER.render(&[("answer", &data.to_string())]),
        Some("resume") => {
            text::RESUME.render(&[("in-force", in_force), ("records", &data.to_string())])
        }
        Some("checkpoint") => {
            text::CHECKPOINT.render(&[("in-force", in_force), ("checkpoint", &data.to_string())])
        }
        Some("services") => text::SERVICES.render(&[("services", &data.to_string())]),
        Some("preserve") => text::PRESERVE.render(&[]),
        Some("unavailable") => {
            text::UNAVAILABLE.render(&[("reason", data.as_str().unwrap_or_default())])
        }
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
pub(crate) const HOOK_GAP: &str = "hook gap";
pub(crate) const TICKET: &str = "ticket intake";
pub(crate) const WORK: &str = "work intake";

/// The host session identity, then what earlier sessions left that it may adopt: each a
/// piece of its own, so a long list never costs the identity.
fn session_pieces(session: &Value, pieces: &mut Vec<Piece>) {
    let identity = json!({"id": session["id"], "task": session["task"]});
    let mut push = |name, rank, text| pieces.push(Piece { name, rank, text });
    push(
        "session identity",
        0,
        text::SESSION.render(&[("identity", &identity.to_string())]),
    );
    if let Some(handoff) = session.get("handoff") {
        push(
            "unfinished tasks of earlier sessions",
            6,
            text::HANDOFF.render(&[("handoff", &handoff.to_string())]),
        );
    }
    if let Some(planning) = session.get("planning") {
        push(
            "planning evidence of earlier sessions",
            7,
            text::PLANNING.render(&[("planning", &planning.to_string())]),
        );
    }
    if let Some(plans) = session.get("plans") {
        push(
            "open work plans",
            5,
            text::PLANS.render(&[("plans", &plans.to_string())]),
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
    if let Some(offer) = result.get("conventions") {
        push("undeclared conventions", 6, conventions_line(offer));
    }
    if let Some(found) = result.get("hookGap") {
        push(HOOK_GAP, 4, hook_gap_line(found));
    }
    if let Some(intake) = result.get("ticket") {
        push(TICKET, 1, ticket_line(intake));
    }
    if let Some(offer) = result.get("work") {
        push(WORK, 1, work_line(offer));
    }
    pieces
}

/// Declared standards the file or commit just written does not meet.
fn standards_line(found: &Value) -> String {
    text::STANDARDS.render(&[("found", &found.to_string())])
}

/// Standards read from the project's own files, in effect while it declares none.
fn conventions_line(offer: &Value) -> String {
    text::CONVENTIONS.render(&[("offer", &offer.to_string())])
}

/// Earlier host sessions here that ran without Craft's hooks; the user may not see the notice.
fn hook_gap_line(found: &Value) -> String {
    text::HOOK_GAP.render(&[("notice", &craft_engine::hook_gap::line(found))])
}

/// Work that starts from a ticket: the command that decides how it is delivered.
fn ticket_line(intake: &Value) -> String {
    let text = |field: &str| intake[field].as_str().unwrap_or_default().to_owned();
    text::TICKET.render(&[
        ("reference", &text("reference")),
        ("command", &text("command")),
    ])
}

/// Work no ticket names: the command that decides how it is delivered.
fn work_line(offer: &Value) -> String {
    text::WORK.render(&[("command", offer["command"].as_str().unwrap_or_default())])
}

/// A test run that exited cleanly without running a test.
fn tests_line(run: &Value) -> String {
    let summary = json!({
        "runner": run["runner"], "reason": run["reason"], "executed": run["executed"],
        "skipped": run["skipped"], "command": run["command"],
    });
    text::TESTS.render(&[("summary", &summary.to_string())])
}

/// The standards a worker the host started inherits from the session that delegated to it.
fn delegation_line(delegation: &Value) -> String {
    text::DELEGATION.render(&[("delegation", &delegation.to_string())])
}

/// The stop summary a held stop gives the model as the reason to continue: one line per
/// finding and per review item, readable by the model and by the user the host shows it to.
fn stop_line(summary: &Value) -> String {
    let text = |value: &Value| value.as_str().unwrap_or_default().to_owned();
    let mut lines = Vec::new();
    for finding in summary["findings"].as_array().into_iter().flatten() {
        let mut place = text(&finding["file"]);
        if let Some(line) = finding["line"].as_u64() {
            place = format!("{place}:{line}");
        }
        let rule = match finding["standard"].as_str() {
            Some(standard) => format!("{standard} {}", text(&finding["rule"])),
            None => text(&finding["rule"]),
        };
        lines.push(format!("- {place} {rule}: {}", text(&finding["message"])));
    }
    if let Some(more) = summary["more"].as_u64() {
        lines.push(format!("- {more} more findings not listed"));
    }
    for item in summary["review"].as_array().into_iter().flatten() {
        let mut parts: Vec<String> = item["summary"]
            .as_str()
            .map(str::to_owned)
            .into_iter()
            .collect();
        let checklist: Vec<String> = item["checklist"]
            .as_array()
            .into_iter()
            .flatten()
            .map(text)
            .collect();
        if !checklist.is_empty() {
            parts.push(checklist.join("; "));
        }
        let mut line = format!("- {}: {}", text(&item["id"]), parts.join(" — "));
        if let Some(run) = item["run"]["command"].as_str() {
            let args: Vec<String> = item["run"]["args"]
                .as_array()
                .into_iter()
                .flatten()
                .map(text)
                .collect();
            line = format!(
                "{line}; run `{}`",
                [vec![run.to_owned()], args].concat().join(" ")
            );
        }
        for source in ["file", "folder"] {
            if let Some(reference) = item[source].as_str() {
                line = format!("{line} (see {reference})");
            }
        }
        lines.push(line);
    }
    let waiting = &summary["approval"];
    if let Some(files) = waiting["files"].as_array() {
        let files: Vec<String> = files.iter().map(text).collect();
        lines.push(format!(
            "- {} changed while edits wait for approval ({}). Tell the user these changed before they approved the plan. {}",
            files.join(", "),
            text(&waiting["why"]),
            text(&waiting["next"])
        ));
    }
    let count = summary["findings"].as_array().map_or(0, Vec::len)
        + summary["more"].as_u64().unwrap_or(0) as usize
        + summary["review"].as_array().map_or(0, Vec::len)
        + usize::from(waiting.is_object());
    text::STOP.render(&[
        ("count", &count.to_string()),
        ("items", if count == 1 { "item" } else { "items" }),
        ("lines", &lines.join("\n")),
    ])
}

/// Whether `host` can hold a stop and continue the turn with a reason.
pub(crate) fn continues_turn(host: &str) -> bool {
    mapping(host, "turn-end").is_some_and(|m| m["continue"] == true)
}

/// Whether `host` reports every tool call that writes a file before and after it runs, so a
/// stop can tell the files the session wrote from those another session or a person wrote
/// in the same checkout meanwhile.
pub(crate) fn attributes_writes(host: &str) -> bool {
    mapping(host, "turn-end").is_some_and(|m| m["attributeWrites"] == true)
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
    // Claude can hand a tool call to the user instead of refusing it: a secret read waits
    // for the person, who sees why; the model cannot approve it. Other hosts refuse it.
    if host == "claude"
        && denied
        && let Some(ask) = result["ask"].as_str()
    {
        let output = json!({"hookSpecificOutput": {"hookEventName": mapping["name"],
            "permissionDecision": "ask", "permissionDecisionReason": ask}});
        return Wire {
            stdout: format!("{output}\n"),
            ..empty
        };
    }
    let name = &mapping["name"];
    if let Some(reason) = stop {
        let output = match host {
            "cursor" => json!({"followup_message": reason}),
            "gemini" => json!({"decision": "deny", "reason": reason}),
            // Continues the turn as feedback, not as a hook error. Context reaches only the
            // model, so the user is shown the same list where the host shows a message.
            _ if mapping["holdAsContext"] == true => {
                let mut output = json!({"hookSpecificOutput": {"hookEventName": name, "additionalContext": reason}});
                if mapping["notice"] == true {
                    output["systemMessage"] = json!(reason);
                }
                output
            }
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
    let (mut event, named) = normalized(&payload, &hook.host, &hook.lifecycle);
    if status != "complete" {
        // Unread input whose tool is unknown may be a shell command: a shell policy judges it.
        if payload
            .get("tool_name")
            .or(payload.get("toolName"))
            .is_none()
        {
            event["tool"] = json!("shell");
        }
        // Hosts start a hook in the project; without a stated directory, that is where it is.
        if event["cwd"].is_null()
            && let Ok(directory) = std::env::current_dir()
        {
            event["cwd"] = json!(directory.to_string_lossy());
        }
    }
    // Only delivery records test runs; other packages' hooks do not read the result.
    if hook.handler == "deliver"
        && event["tool"] == "shell"
        && let Some(verdict) = event["command"]
            .as_str()
            .and_then(|command| test_run(&payload, &hook.host, &hook.lifecycle, command))
    {
        event["testRun"] = verdict;
    }
    // Only delivery reads the user's answer to the host's own plan approval question.
    if hook.handler == "deliver"
        && hook.lifecycle == "after-tool"
        && let Some(tool) = host_path(&hook.host, "approval")
        && first(&payload, &["tool_name", "toolName"]).and_then(Value::as_str) == Some(&tool)
        && let Some(answers) = answers(&payload)
    {
        event["answers"] = answers;
    }
    let mut result = host::run(&facts(&hook, &event, &named, status)).unwrap_or_else(|message| {
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
    // Unread input that no policy denied leaves the event to the host: a context handler stays
    // advisory, and the failure is in telemetry, not in front of the user.
    if let Some(error) =
        input_error.filter(|_| enforcement && !observational && result["decision"] != "deny")
    {
        return (
            Wire {
                stdout: String::new(),
                stderr: format!("{error}\n"),
                exit_code: 1,
            },
            export,
        );
    }
    // An observing hook has no opinion, unless the safety policy refused the command.
    if observational && result["decision"] != "deny" {
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
mod tests;
