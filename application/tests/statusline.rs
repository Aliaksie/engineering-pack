//! Replays `tests/fixtures/runtime/statusline-contracts.json`, captured from the JavaScript
//! status-line entry, through `craft statusline`.
use craft_application::statusline;
use serde_json::{Value, json};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

const CORPUS: &str = include_str!("../../tests/fixtures/runtime/statusline-contracts.json");

fn corpus() -> Value {
    serde_json::from_str(CORPUS).unwrap()
}

/// One scratch directory per test binary; the bundle root is an empty installed package.
fn scratch() -> &'static Path {
    static SCRATCH: OnceLock<PathBuf> = OnceLock::new();
    SCRATCH.get_or_init(|| {
        let dir = std::env::temp_dir().join(format!("craft-statusline-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("bundle")).unwrap();
        // Physical, but without Windows' verbatim prefix: a user's `<root>/../file` keeps
        // ordinary path resolution, which a `\\?\` path would turn off.
        let physical = dir.canonicalize().unwrap();
        match physical.to_str().and_then(|p| p.strip_prefix(r"\\?\")) {
            Some(plain) => PathBuf::from(plain),
            None => physical,
        }
    })
}

/// Intentional differences from the pinned entry: what the native command must do instead.
enum Correction {
    /// A Claude read without a payload reports what an empty declaration reports.
    SameAs(&'static str),
    /// The input is refused with this message.
    Refused(&'static str),
}

fn correction(id: &str) -> Option<Correction> {
    let suffix = id.rsplit(':').next().unwrap();
    match id {
        // JavaScript parsed the empty payload of an explicit read and failed.
        "claude:empty-stdin:json" | "claude:whitespace:json" => {
            Some(Correction::SameAs("claude:data-empty:json"))
        }
        "claude:empty-stdin:text" | "claude:whitespace:text" => {
            Some(Correction::SameAs("claude:data-empty:text"))
        }
        // JavaScript accepted arrays and scalars as payloads and threw a TypeError on null.
        _ if id.starts_with("claude:payload-") => Some(Correction::Refused(
            "statusline payload must be a JSON object",
        )),
        // `craft session --data` already refuses a non-object declaration.
        _ if suffix == "data-array" => {
            Some(Correction::Refused("session input must be a JSON object"))
        }
        _ => None,
    }
}

fn write_project(root: &Path, files: &Value) {
    fs::create_dir_all(root).unwrap();
    for (file, text) in files.as_object().unwrap() {
        let path = root.join(file);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text.as_str().unwrap()).unwrap();
    }
}

/// The corpus normalization: project root, run instants and notice keys.
fn normalize(text: &str, root: &Path) -> String {
    let text = text.replace(&root.to_string_lossy().replace('\\', "\\\\"), "<ROOT>");
    let bytes = text.as_bytes();
    let instant = |at: usize| {
        let shape = b"dddd-dd-ddTdd:dd:dd.dddZ";
        bytes.len() >= at + shape.len()
            && shape.iter().enumerate().all(|(i, c)| match c {
                b'd' => bytes[at + i].is_ascii_digit(),
                c => bytes[at + i] == *c,
            })
    };
    let (mut output, mut index) = (String::new(), 0);
    while index < bytes.len() {
        if instant(index) {
            output.push_str("<TIME>");
            index += 24;
        } else if text[index..].starts_with("\"key\": \"")
            && text[index + 8..].len() > 64
            && text[index + 8..index + 72]
                .bytes()
                .all(|b| b.is_ascii_hexdigit())
        {
            output.push_str("\"key\": \"<KEY>");
            index += 72;
        } else {
            let c = text[index..].chars().next().unwrap();
            output.push(c);
            index += c.len_utf8();
        }
    }
    output
}

/// Recorded events as the corpus lists them, without the JS-only nested command envelope.
fn events(root: &Path) -> Vec<Value> {
    fn walk(dir: &Path, found: &mut Vec<PathBuf>) {
        for entry in fs::read_dir(dir).into_iter().flatten().flatten() {
            let path = entry.path();
            if path.is_dir() {
                walk(&path, found);
            } else if path.ends_with("events.jsonl") {
                found.push(path);
            }
        }
    }
    let mut files = Vec::new();
    walk(&root.join(".craft"), &mut files);
    let mut events: Vec<Value> = files
        .iter()
        .flat_map(|file| {
            fs::read_to_string(file)
                .unwrap()
                .lines()
                .map(|line| serde_json::from_str::<Value>(line).unwrap())
                .collect::<Vec<_>>()
        })
        .map(|event| {
            let mut entry =
                json!({"name": event["name"], "kind": event["kind"], "outcome": event["outcome"]});
            for (field, value) in [
                ("reasonCode", &event["reasonCode"]),
                ("host", &event["host"]),
                ("package", &event["attributes"]["package"]),
            ] {
                if !value.is_null() {
                    entry[field] = value.clone();
                }
            }
            entry
        })
        .filter(|event| event["name"] != "cli.command")
        .collect();
    events.sort_by_key(Value::to_string);
    events
}

struct Run {
    exit_code: i32,
    stdout: String,
    stderr: String,
    events: Vec<Value>,
}

fn replay(case: &Value, projects: &Value) -> Run {
    let id = case["id"].as_str().unwrap();
    let base = scratch().join(id.replace(':', "_"));
    let root = base.join("project");
    write_project(&root, &projects[case["project"].as_str().unwrap()]);
    let data = base.join("data.json");
    if let Some(declared) = case.get("data") {
        fs::write(&data, declared.to_string()).unwrap();
    }
    let mut argv: Vec<String> = [
        case["host"].as_str().unwrap(),
        "--package",
        "craft-delivery",
        "--bundle",
        &scratch().join("bundle").to_string_lossy(),
        "--root",
        &root.to_string_lossy(),
    ]
    .map(String::from)
    .to_vec();
    argv.extend(case["args"].as_array().unwrap().iter().map(|arg| {
        arg.as_str()
            .unwrap()
            .replace("<DATA>", &data.to_string_lossy())
            .replace("<ROOT>", &root.to_string_lossy())
    }));
    let stdin = case["stdin"].as_str().unwrap_or_default();
    let outcome = statusline::run(&argv, &mut stdin.as_bytes(), false);
    Run {
        exit_code: outcome.exit_code,
        stdout: normalize(&outcome.stdout, &root),
        stderr: normalize(&outcome.stderr, &root),
        events: events(&root),
    }
}

/// Expected events with the order the native trace records them in being irrelevant.
fn expected_events(case: &Value) -> Vec<Value> {
    let mut events: Vec<Value> = case["telemetry"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|event| event["name"] != "cli.command")
        .cloned()
        .collect();
    events.sort_by_key(Value::to_string);
    events
}

fn assert_stderr(id: &str, actual: &str, expected: &str) {
    match expected {
        "<JSON-PARSE-ERROR>" => assert!(
            actual.starts_with("invalid statusline payload: "),
            "{id}: {actual}"
        ),
        "<NOT-FOUND>" => assert!(actual.contains("(os error 2)"), "{id}: {actual}"),
        usage if usage.starts_with("usage: session.mjs ") => assert!(
            actual.starts_with("usage: craft statusline <host> "),
            "{id}: {actual}"
        ),
        message => assert_eq!(actual, message, "{id}"),
    }
}

#[test]
fn statusline_matches_the_frozen_corpus() {
    let corpus = corpus();
    let cases = corpus["cases"].as_array().unwrap();
    let find = |id: &str| cases.iter().find(|case| case["id"] == id).unwrap();
    for case in cases {
        let id = case["id"].as_str().unwrap();
        let run = replay(case, &corpus["projects"]);
        match correction(id) {
            Some(Correction::SameAs(reference)) => {
                let reference = find(reference);
                assert_eq!(run.exit_code, 0, "{id}: {}", run.stderr);
                assert_eq!(run.stdout, reference["stdout"].as_str().unwrap(), "{id}");
                let declared: Vec<Value> = expected_events(reference)
                    .into_iter()
                    .filter(|event| event["name"] != "input.loaded")
                    .collect();
                assert_eq!(run.events, declared, "{id}");
            }
            Some(Correction::Refused(message)) => {
                assert_eq!(run.exit_code, 2, "{id}");
                assert_eq!(run.stdout, "", "{id}");
                assert_eq!(run.stderr, format!("{message}\n"), "{id}");
                let failed = |name: &str, kind: &str| json!({"name": name, "kind": kind, "outcome": "failure", "reasonCode": "INVALID_INPUT", "host": case["host"], "package": "craft-delivery"});
                let mut expected = vec![
                    failed("diagnostic", "diagnostic"),
                    failed("host.session", "operation"),
                ];
                if case.get("data").is_some() {
                    expected.push(json!({"name": "input.loaded", "kind": "operation", "outcome": "success", "host": case["host"], "package": "craft-delivery"}));
                }
                expected.sort_by_key(Value::to_string);
                assert_eq!(run.events, expected, "{id}");
            }
            None => {
                assert_eq!(
                    run.exit_code as i64,
                    case["exitCode"].as_i64().unwrap(),
                    "{id}: {}",
                    run.stderr
                );
                if id.ends_with(":help") {
                    assert!(
                        run.stdout.starts_with("usage: craft statusline <host> "),
                        "{id}"
                    );
                    continue;
                }
                assert_eq!(run.stdout, case["stdout"].as_str().unwrap(), "{id}");
                assert_stderr(id, &run.stderr, case["stderr"].as_str().unwrap());
                assert_eq!(run.events, expected_events(case), "{id}");
            }
        }
    }
}

fn status(args: &[&str], stdin: &str, interactive: bool) -> craft_application::Outcome {
    static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let next = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let root = scratch().join(format!("direct-{next}"));
    fs::create_dir_all(&root).unwrap();
    let mut argv: Vec<String> = args.iter().map(|arg| arg.to_string()).collect();
    argv.extend(["--root".into(), root.to_string_lossy().into_owned()]);
    statusline::run(&argv, &mut stdin.as_bytes(), interactive)
}

#[test]
fn a_terminal_is_not_read_as_a_payload() {
    let bundle = scratch().join("bundle");
    let bundle = bundle.to_str().unwrap();
    // A pending payload on a terminal would block; the read reports unknown usage instead.
    let outcome = status(
        &["claude", "--package", "p", "--bundle", bundle],
        "{not json",
        true,
    );
    assert_eq!(outcome.exit_code, 0, "{}", outcome.stderr);
    assert_eq!(outcome.stdout, "Context usage unknown\n");
    // An explicit request for the host payload still reads it and requires one.
    let outcome = status(
        &[
            "claude",
            "--package",
            "p",
            "--bundle",
            bundle,
            "--native-stdin",
        ],
        "",
        true,
    );
    assert_eq!(
        (outcome.exit_code, outcome.stderr.as_str()),
        (2, "statusline payload is empty\n")
    );
}

#[test]
fn arguments_name_a_known_host_package_and_installed_bundle() {
    for args in [
        &["unknown-host", "--package", "p"][..],
        &["claude"][..],
        &["claude", "--package", "p", "--data"][..],
        &["claude", "--package", "p", "positional"][..],
    ] {
        let outcome = status(args, "", false);
        assert_eq!(outcome.exit_code, 2, "{args:?}");
        assert!(outcome.stdout.is_empty(), "{args:?}");
    }
    // Without the launcher's package root a status cannot account installed guidance.
    if std::env::var_os("CRAFT_PACKAGE_ROOT").is_none() {
        let outcome = status(&["codex", "--package", "p"], "", false);
        assert_eq!(outcome.exit_code, 2);
        assert!(
            outcome.stderr.contains("installed package root"),
            "{}",
            outcome.stderr
        );
    }
}

#[test]
fn every_host_reports_its_session_capabilities() {
    let table: Value = serde_json::from_str(include_str!("../../hosts/session.json")).unwrap();
    let bundle = scratch().join("bundle");
    for (host, capabilities) in table.as_object().unwrap() {
        assert_eq!(statusline::capabilities(host).as_ref(), Some(capabilities));
        let outcome = status(
            &[
                host,
                "--package",
                "p",
                "--bundle",
                bundle.to_str().unwrap(),
                "--json",
            ],
            "",
            false,
        );
        assert_eq!(outcome.exit_code, 0, "{host}: {}", outcome.stderr);
        let output: Value = serde_json::from_str(&outcome.stdout).unwrap();
        let mut reported = capabilities.clone();
        reported
            .as_object_mut()
            .unwrap()
            .shift_remove("contextLimits");
        assert_eq!(output["host"], reported, "{host}");
        assert_eq!(output["sessionHost"]["name"], json!(host));
        assert_eq!(
            output["sessionHost"]["reliableCompaction"],
            json!(capabilities["commands"].get("compact").is_some()),
            "{host}"
        );
    }
}

#[test]
fn usage_counts_current_input_and_cache_tokens_only() {
    let bundle = scratch().join("bundle");
    let payload = json!({"session_id": "s1", "context_window": {"context_window_size": 100, "total_input_tokens": 999_999,
        "current_usage": {"input_tokens": 30, "cache_creation_input_tokens": 10, "cache_read_input_tokens": 5, "output_tokens": 1000}}});
    let outcome = status(
        &[
            "claude",
            "--package",
            "p",
            "--bundle",
            bundle.to_str().unwrap(),
            "--json",
        ],
        &payload.to_string(),
        false,
    );
    assert_eq!(outcome.exit_code, 0, "{}", outcome.stderr);
    let usage = &serde_json::from_str::<Value>(&outcome.stdout).unwrap()["usage"];
    assert_eq!(
        (&usage["currentTokens"], &usage["usedRatio"]),
        (&json!(45), &json!(0.45))
    );
}

#[test]
fn a_declared_host_is_an_object_or_absent() {
    let bundle = scratch().join("bundle");
    let payload = json!({"session_id": "s1", "model": {"id": "m"}}).to_string();
    for (host, refused) in [
        (json!(null), false),
        (json!({"model": "declared"}), false),
        (json!("claude"), true),
        (json!([]), true),
    ] {
        let data = scratch().join(format!(
            "host-{}.json",
            host.to_string().replace(['"', '{', '}', ':', '[', ']'], "")
        ));
        fs::write(&data, json!({"sessionId": "s1", "host": host}).to_string()).unwrap();
        let args = [
            "claude",
            "--package",
            "p",
            "--bundle",
            bundle.to_str().unwrap(),
            "--data",
            data.to_str().unwrap(),
            "--native-stdin",
            "--json",
        ];
        let outcome = status(&args, &payload, false);
        if refused {
            assert_eq!(
                (outcome.exit_code, outcome.stderr.as_str()),
                (2, "session input host must be a JSON object\n"),
                "{host}"
            );
        } else {
            assert_eq!(outcome.exit_code, 0, "{host}: {}", outcome.stderr);
            let output: Value = serde_json::from_str(&outcome.stdout).unwrap();
            assert_eq!(output["sessionHost"]["model"], "m", "{host}");
        }
    }
}

#[test]
fn a_project_budget_on_a_listed_claude_model_names_the_restart() {
    let root = scratch().join("budget");
    write_project(
        &root,
        &json!({".craft/project.json": json!({"formatVersion": 1, "execution": {"hosts": {"claude": {
            "contextLimits": {"claude-opus-5-5": {"budgetTokens": 200_000}}}}}}).to_string()}),
    );
    let bundle = scratch().join("bundle");
    // No window in the payload: the bundled table supplies the listed model's window.
    let payload = json!({"session_id": "s1", "model": {"id": "claude-opus-5-5"},
        "context_window": {"current_usage": {"input_tokens": 250_000, "cache_creation_input_tokens": 0, "cache_read_input_tokens": 0}}});
    let run = |json: bool| {
        let mut argv: Vec<String> = [
            "claude",
            "--package",
            "p",
            "--bundle",
            bundle.to_str().unwrap(),
            "--root",
            root.to_str().unwrap(),
        ]
        .iter()
        .map(|arg| arg.to_string())
        .collect();
        if json {
            argv.push("--json".into());
        }
        statusline::run(&argv, &mut payload.to_string().as_bytes(), false)
    };
    let outcome = run(true);
    assert_eq!(outcome.exit_code, 0, "{}", outcome.stderr);
    let output: Value = serde_json::from_str(&outcome.stdout).unwrap();
    assert_eq!(output["usage"]["capacityTokens"], 1_000_000);
    assert_eq!(output["usage"]["capacitySource"], "model-context-limit");
    assert_eq!(
        output["budget"],
        json!({"budgetTokens": 200_000, "exceeded": true})
    );
    let text = run(false);
    assert!(
        text.stdout
            .starts_with("Context 250000/1000000 tokens (25%) | Past 200000-token budget"),
        "{}",
        text.stdout
    );
}
