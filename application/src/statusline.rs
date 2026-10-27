//! `craft statusline <host> --package <name> [--root project] [--data task.json] [--json]
//! [--native-stdin] [--bundle root]`: one read-only session status for a host status line.
//! Only Claude's status-line payload has a verified contract here; it supplies the session,
//! model and current context usage. Other hosts use declared data and never scrape private
//! transcripts for a token count.
use crate::cli::{Outcome, session_status_for};
use crate::failure::Failure;
use crate::project::absolute;
use crate::project::read_input;
use crate::telemetry::{Operation, Recorder, now_ms};
use craft_engine::contracts::js_length;
use serde_json::{Map, Value, json};
use std::io::{IsTerminal, Read, Write};

const MAX_PAYLOAD: usize = 65_536;
const HOSTS: &str = include_str!("../../hosts/session.json");
const USAGE: &str = "usage: craft statusline <host> --package name [--root project] [--data task.json] [--json] [--native-stdin] [--bundle root]";
/// Set by the package launcher: the installed package whose guidance the status accounts.
pub(crate) const PACKAGE_ROOT: &str = "CRAFT_PACKAGE_ROOT";

/// The observation, display and native commands a host adapter offers for session advice.
pub fn capabilities(host: &str) -> Option<Value> {
    let table: Value = serde_json::from_str(HOSTS).expect("embedded host session table");
    table.get(host).cloned()
}

struct Request {
    host: String,
    package: String,
    root: String,
    data: Option<String>,
    bundle: Option<String>,
    json: bool,
    native_stdin: bool,
}

fn parse(args: &[String]) -> Result<Request, Failure> {
    let (host, rest) = args.split_first().ok_or(USAGE)?;
    if capabilities(host).is_none() {
        return Err(USAGE.into());
    }
    let mut request = Request {
        host: host.clone(),
        package: String::new(),
        root: ".".into(),
        data: None,
        bundle: None,
        json: false,
        native_stdin: false,
    };
    let mut seen = Vec::new();
    let mut rest = rest.iter();
    while let Some(flag) = rest.next() {
        if seen.contains(flag) {
            return Err(USAGE.into());
        }
        seen.push(flag.clone());
        let slot = match flag.as_str() {
            "--json" => {
                request.json = true;
                continue;
            }
            "--native-stdin" => {
                request.native_stdin = true;
                continue;
            }
            "--package" => &mut request.package,
            "--root" => &mut request.root,
            "--data" => request.data.insert(String::new()),
            "--bundle" => request.bundle.insert(String::new()),
            _ => return Err(USAGE.into()),
        };
        match rest
            .next()
            .filter(|v| !v.is_empty() && !v.starts_with("--"))
        {
            Some(value) => *slot = value.clone(),
            None => return Err(format!("missing {flag} value").into()),
        }
    }
    if request.package.is_empty() {
        return Err(USAGE.into());
    }
    Ok(request)
}

/// Claude's current context occupancy: input and cache tokens, excluding output.
fn observation(payload: &Value) -> Option<Value> {
    let window = &payload["context_window"];
    let current = window.get("current_usage").filter(|c| c.is_object())?;
    let mut total = 0u64;
    for field in [
        "input_tokens",
        "cache_creation_input_tokens",
        "cache_read_input_tokens",
    ] {
        // `Number.isSafeInteger` accepts integral floats such as `10.0`.
        let count = current[field]
            .as_f64()
            .filter(|n| *n >= 0.0 && n.fract() == 0.0 && *n <= 9_007_199_254_740_991.0)?;
        total += count as u64;
    }
    let mut usage = json!({"kind": "current-context", "currentTokens": total});
    if let Some(capacity) = window.get("context_window_size") {
        usage["capacityTokens"] = capacity.clone();
    }
    if let Some(session) = payload.get("session_id") {
        usage["sessionId"] = session.clone();
    }
    usage["source"] =
        json!("claude-statusline.current_usage (input and cache tokens; excludes output)");
    usage["observedAt"] = json!(craft_engine::timestamp::format(now_ms()));
    Some(usage)
}

/// The payload model id when it is a bounded single-line string.
fn model(payload: &Value) -> Option<&str> {
    payload["model"]["id"].as_str().filter(|id| {
        (1..=512).contains(&js_length(id)) && !id.chars().any(|c| c < ' ' || c == '\x7f')
    })
}

/// The status-line payload, or `None` when the host sent nothing.
fn payload(stdin: &mut dyn Read, required: bool) -> Result<Option<Value>, Failure> {
    let mut bytes = Vec::new();
    stdin
        .take(MAX_PAYLOAD as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(Failure::from)?;
    if bytes.len() > MAX_PAYLOAD {
        return Err("statusline payload exceeds size limit".into());
    }
    let text = String::from_utf8_lossy(&bytes);
    if text.trim().is_empty() {
        return match required {
            true => Err("statusline payload is empty".into()),
            false => Ok(None),
        };
    }
    match serde_json::from_str::<Value>(&crate::hook::repair_surrogates(&text)) {
        Ok(payload @ Value::Object(_)) => Ok(Some(payload)),
        Ok(_) => Err("statusline payload must be a JSON object".into()),
        Err(error) => Err(format!("invalid statusline payload: {error}").into()),
    }
}

/// The declared data with the host's own observations applied over it.
fn declared(
    request: &Request,
    stdin: &mut dyn Read,
    interactive: bool,
    recorder: &mut Recorder,
    parent: &Operation,
) -> Result<Value, Failure> {
    let claude = request.host == "claude";
    if request.native_stdin && !claude {
        recorder.report("host", "HOST_CAPABILITY_UNAVAILABLE", Map::new(), parent);
        return Err("native usage input is unsupported by this adapter".into());
    }
    let mut data = match &request.data {
        Some(file) => read_input(recorder, parent, file)?,
        None => json!({}),
    };
    let Some(fields) = data.as_object_mut() else {
        return Err("session input must be a JSON object".into());
    };
    match fields.get("host") {
        None | Some(Value::Object(_)) => {}
        Some(Value::Null) => {
            fields.shift_remove("host");
        }
        Some(_) => return Err("session input host must be a JSON object".into()),
    }
    // A terminal sends no payload; reading it would wait for input that never comes.
    let read = claude && (request.native_stdin || (request.data.is_none() && !interactive));
    if let Some(payload) = read
        .then(|| payload(stdin, request.native_stdin))
        .transpose()?
        .flatten()
    {
        if request.data.is_some() && data.get("sessionId") != payload.get("session_id") {
            return Err("task record does not identify this native session".into());
        }
        // The host is authoritative for the session, model and usage it reports.
        let fields = data.as_object_mut().expect("checked object");
        for (field, value) in [
            ("sessionId", payload.get("session_id").cloned()),
            ("usage", observation(&payload)),
        ] {
            match value {
                Some(value) => fields.insert(field.into(), value),
                None => fields.shift_remove(field),
            };
        }
        let host = fields.entry("host").or_insert_with(|| json!({}));
        match model(&payload) {
            Some(id) => host["model"] = json!(id),
            None => {
                host.as_object_mut()
                    .expect("checked object")
                    .shift_remove("model");
            }
        }
    }
    let compaction = capabilities(&request.host).expect("parsed host")["commands"]
        .get("compact")
        .is_some();
    let mut host = Map::from_iter([("reliableCompaction".to_string(), json!(compaction))]);
    if let Some(declared) = data["host"].as_object() {
        host.extend(declared.clone());
    }
    host.insert("name".into(), json!(request.host));
    data["host"] = Value::Object(host);
    Ok(data)
}

fn bundle(request: &Request) -> Result<Vec<String>, Failure> {
    request
        .bundle
        .clone()
        .or_else(|| std::env::var(PACKAGE_ROOT).ok().filter(|root| !root.is_empty()))
        .map(|root| vec![root])
        .ok_or_else(|| "statusline requires the installed package root: run it through the package's bin/craft or pass --bundle".into())
}

/// `Context 150015/200000 tokens (75%) | Next: /compact`, with the host's own command names.
fn render(status: &Value, commands: &Value) -> String {
    let usage = &status["usage"];
    let mut line = if usage["status"] == "observed" {
        let percent = (usage["usedRatio"].as_f64().unwrap_or(0.0) * 100.0 + 0.5).floor();
        format!(
            "Context {}/{} tokens ({percent}%)",
            usage["currentTokens"], usage["capacityTokens"]
        )
    } else {
        "Context usage unknown".into()
    };
    if let Some(action) = status["suggestion"]["action"].as_str() {
        let command = commands[action].as_str().unwrap_or(action);
        line.push_str(&format!(" | Next: {command}"));
        if let Some(checks) = status["suggestion"]["checks"].as_array() {
            line.push_str(&format!(
                " ({} declared check(s); inspect JSON for argv)",
                checks.len()
            ));
        }
    }
    line
}

fn status(
    request: &Request,
    stdin: &mut dyn Read,
    interactive: bool,
    recorder: &mut Recorder,
    parent: &Operation,
) -> Result<String, Failure> {
    let data = declared(request, stdin, interactive, recorder, parent)?;
    let bundles = bundle(request)?;
    let mut status =
        session_status_for(&absolute(&request.root), &data, &bundles, recorder, parent)?;
    let capabilities = capabilities(&request.host).expect("parsed host");
    if !request.json {
        return Ok(format!("{}\n", render(&status, &capabilities["commands"])));
    }
    let fields = status.as_object_mut().expect("status object");
    if let Some(session_host) = fields.insert("host".into(), capabilities) {
        fields.insert("sessionHost".into(), session_host);
    }
    Ok(format!(
        "{}\n",
        serde_json::to_string_pretty(&status).expect("JSON values serialize")
    ))
}

/// One status read under `host.session`. `interactive` says stdin is a terminal.
pub fn run(args: &[String], stdin: &mut dyn Read, interactive: bool) -> Outcome {
    let outcome = |exit_code, stdout: String, stderr: String, export| Outcome {
        exit_code,
        stdout,
        stderr,
        export,
    };
    if args.iter().any(|arg| arg == "--help") {
        return outcome(0, format!("{USAGE}\n"), String::new(), None);
    }
    // Telemetry follows the named project, host and package even when the arguments fail.
    let hint = |flag: &str| {
        let index = args.iter().position(|arg| arg == flag)?;
        args.get(index + 1)
            .filter(|value| !value.starts_with("--"))
            .map(String::as_str)
    };
    let host = args
        .first()
        .map(String::as_str)
        .filter(|host| capabilities(host).is_some());
    let mut recorder = Recorder::open(&absolute(hint("--root").unwrap_or(".")), None, host);
    recorder.package(hint("--package"));
    let operation = recorder.start("host.session", None);
    let result = parse(args)
        .and_then(|request| status(&request, stdin, interactive, &mut recorder, &operation));
    match result {
        Ok(text) => {
            recorder.complete(operation, Map::new(), None);
            outcome(0, text, String::new(), recorder.finish())
        }
        Err(failure) => {
            recorder.report("host", "INVALID_INPUT", Map::new(), &operation);
            recorder.complete(operation, Map::new(), Some("INVALID_INPUT"));
            outcome(
                2,
                String::new(),
                format!("{}\n", failure.message),
                recorder.finish(),
            )
        }
    }
}

/// The `craft statusline` process: write the line, then post the telemetry export.
pub fn main(args: &[String]) -> i32 {
    let stdin = std::io::stdin();
    let interactive = stdin.is_terminal();
    let outcome = run(args, &mut stdin.lock(), interactive);
    let _ = std::io::stdout().write_all(outcome.stdout.as_bytes());
    let _ = std::io::stderr().write_all(outcome.stderr.as_bytes());
    Recorder::deliver(outcome.export);
    outcome.exit_code
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn advice_names_the_hosts_own_command() {
        let status = json!({"usage": {"status": "observed", "currentTokens": 85, "capacityTokens": 100, "usedRatio": 0.845},
            "suggestion": {"action": "compact"}});
        let codex = &capabilities("codex").unwrap()["commands"];
        assert_eq!(
            render(&status, codex),
            "Context 85/100 tokens (85%) | Next: /compact"
        );
        let agents = &capabilities("agents").unwrap()["commands"];
        assert_eq!(
            render(&status, agents),
            "Context 85/100 tokens (85%) | Next: compact"
        );
        // `Math.round` rounds halves up.
        let half = json!({"usage": {"status": "observed", "currentTokens": 1, "capacityTokens": 200, "usedRatio": 0.005}});
        assert_eq!(render(&half, codex), "Context 1/200 tokens (1%)");
    }
}
