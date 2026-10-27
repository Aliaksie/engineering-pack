//! Replays `tests/fixtures/runtime/telemetry-contracts.json`, captured from the JS commands,
//! through `telemetry-report`, `telemetry-capture` and `telemetry-forget` over seeded stores.
use craft_application::cli;
use craft_platform::state;
use serde_json::{Value, json};
use std::fs;
use std::path::{Path, PathBuf};

const CORPUS: &str = include_str!("../../tests/fixtures/runtime/telemetry-contracts.json");

struct Scratch(PathBuf);
impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn lines(value: &Value) -> Vec<u8> {
    let mut text = value
        .as_array()
        .unwrap()
        .iter()
        .map(|line| line.as_str().unwrap())
        .collect::<Vec<_>>()
        .join("\n")
        .into_bytes();
    text.push(b'\n');
    text
}

fn seed(root: &Path, case: &Value) {
    fs::create_dir_all(root.join(".craft")).unwrap();
    fs::write(root.join(".craft/telemetry.json"), "{\"local\":false}\n").unwrap();
    let key = |create| state::file(root, "telemetry/key.json", create, None).unwrap();
    match &case["key"] {
        Value::String(text) if text.len() == 64 => {
            fs::write(key(true), format!("{}\n", json!({ "key": text }))).unwrap()
        }
        Value::String(raw) => fs::write(key(true), raw).unwrap(),
        _ => {}
    }
    let log = state::file(root, "telemetry/events.jsonl", true, None).unwrap();
    if case.get("log").is_some() {
        let mut bytes = lines(&case["log"]);
        for byte in case["trailingBytes"].as_array().into_iter().flatten() {
            bytes.push(byte.as_u64().unwrap() as u8);
        }
        fs::write(&log, bytes).unwrap();
    }
    if case.get("backup").is_some() {
        fs::write(format!("{}.1", log.display()), lines(&case["backup"])).unwrap();
    }
    if let Some(raw) = case.get("raw") {
        let text = raw["repeat"]
            .as_str()
            .unwrap()
            .repeat(raw["count"].as_u64().unwrap() as usize);
        fs::write(&log, text).unwrap();
    }
}

/// The receipt path relative to the root, with the worktree directory as `<W>`.
fn normalize(value: &mut Value, root: &Path) {
    if let Some(file) = value.get("file").and_then(Value::as_str) {
        let relative = Path::new(file).strip_prefix(root).unwrap();
        let mut parts: Vec<String> = relative
            .components()
            .map(|part| part.as_os_str().to_string_lossy().into_owned())
            .collect();
        parts[2] = "<W>".into();
        value["file"] = json!(format!("<ROOT>/{}", parts.join("/")));
    }
    if value.get("capturedAt").is_some() {
        value["capturedAt"] = json!("<T>");
    }
}

#[test]
fn telemetry_commands_match_the_frozen_corpus() {
    let corpus: Value = serde_json::from_str(CORPUS).unwrap();
    let mut steps = 0;
    for case in corpus["cases"].as_array().unwrap() {
        let id = case["id"].as_str().unwrap();
        let scratch = Scratch(std::env::temp_dir().join(format!(
            "craft-telemetry-contracts-{}-{id}",
            std::process::id()
        )));
        let _ = fs::remove_dir_all(&scratch.0);
        fs::create_dir_all(&scratch.0).unwrap();
        let root = state::canonical_root(&scratch.0).unwrap();
        seed(&root, case);
        let mut captures: Vec<String> = Vec::new();
        for (index, step) in case["steps"].as_array().unwrap().iter().enumerate() {
            let mut argv: Vec<String> = step["args"]
                .as_array()
                .unwrap()
                .iter()
                .map(|arg| {
                    let arg = arg.as_str().unwrap();
                    match arg.strip_prefix('@').and_then(|n| n.parse::<usize>().ok()) {
                        Some(n) => captures
                            .get(n)
                            .cloned()
                            .unwrap_or_else(|| "0123456789abcdef01234567".into()),
                        None => arg.to_string(),
                    }
                })
                .collect();
            argv.extend(["--root".into(), root.display().to_string()]);
            let outcome = cli::run(&argv);
            let label = format!("{id} #{index} {argv:?}: {}", outcome.stderr);
            assert_eq!(
                i64::from(outcome.exit_code),
                step["status"].as_i64().unwrap(),
                "{label}"
            );
            if outcome.exit_code != 0 {
                assert_eq!(outcome.stderr.trim_end(), step["stderr"], "{label}");
                continue;
            }
            let mut actual: Value = serde_json::from_str(&outcome.stdout).unwrap();
            if let Some(capture) = actual["capture"].as_str()
                && !captures.iter().any(|known| known == capture)
            {
                captures.push(capture.into());
            }
            normalize(&mut actual, &root);
            let mut expected: Value =
                serde_json::from_str(step["stdout"].as_str().unwrap()).unwrap();
            if case.get("correction").is_some() {
                // The impossible date is an invalid record rather than the earliest event.
                assert_eq!(expected["retainedEvents"], 2, "{label}");
                expected["retainedEvents"] = json!(1);
                expected["identity"]["sessionEvents"] = json!(1);
                expected["attribution"]["packages"]["unknown"] = json!(1);
                expected["window"]["first"] = json!("2026-10-20T10:00:00.000Z");
                expected["storage"]["invalidRecords"] = json!(1);
                expected["features"]["routing"]["events"] = json!(1);
            }
            assert_eq!(actual, expected, "{label}");
            steps += 1;
        }
    }
    assert_eq!(steps, 31, "successful steps replayed");
}
