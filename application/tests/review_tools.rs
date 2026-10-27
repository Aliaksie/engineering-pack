//! Replays `tests/fixtures/runtime/review-tool-contracts.json`, captured from the JS review
//! tools, through `review-prepare`, `review-check` and `review-publication`.
use craft_application::cli;
use serde_json::{Value, json};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;

const CORPUS: &str = include_str!("../../tests/fixtures/runtime/review-tool-contracts.json");

fn corpus() -> Value {
    serde_json::from_str(CORPUS).unwrap()
}

/// One scratch directory per test binary, also the working directory: `review-check`
/// records telemetry there, never in this repository.
fn scratch() -> &'static Path {
    static SCRATCH: OnceLock<PathBuf> = OnceLock::new();
    SCRATCH.get_or_init(|| {
        let dir = std::env::temp_dir().join(format!("craft-review-tools-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let dir = dir.canonicalize().unwrap();
        std::env::set_current_dir(&dir).unwrap();
        dir
    })
}

fn run(args: &[String]) -> craft_application::Outcome {
    cli::run(args)
}

fn args(values: &[&str]) -> Vec<String> {
    values.iter().map(|value| value.to_string()).collect()
}

fn pretty(value: &Value) -> String {
    format!("{}\n", serde_json::to_string_pretty(value).unwrap())
}

fn build(root: &Path, steps: &Value) {
    fs::create_dir_all(root).unwrap();
    for step in steps.as_array().unwrap() {
        if let Some(file) = step["write"].as_str() {
            let path = root.join(file);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            let bytes: Vec<u8> = match step.get("bytes") {
                Some(bytes) => bytes
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|b| b.as_u64().unwrap() as u8)
                    .collect(),
                None => step["text"].as_str().unwrap().as_bytes().to_vec(),
            };
            fs::write(path, bytes).unwrap();
        } else if let Some(file) = step["remove"].as_str() {
            fs::remove_file(root.join(file)).unwrap();
        } else {
            let date = step["date"].as_str().unwrap_or("2026-01-01T00:00:00Z");
            let git: Vec<&str> = step["git"]
                .as_array()
                .unwrap()
                .iter()
                .map(|a| a.as_str().unwrap())
                .collect();
            let output = Command::new("git")
                .args(&git)
                .current_dir(root)
                .env("GIT_CONFIG_GLOBAL", root.join("..").join("no-global"))
                .env("GIT_CONFIG_NOSYSTEM", "1")
                .env("GIT_AUTHOR_DATE", date)
                .env("GIT_COMMITTER_DATE", date)
                .env("GIT_AUTHOR_NAME", "Craft")
                .env("GIT_AUTHOR_EMAIL", "craft@example.test")
                .env("GIT_COMMITTER_NAME", "Craft")
                .env("GIT_COMMITTER_EMAIL", "craft@example.test")
                .output()
                .unwrap();
            assert!(output.status.success(), "git {git:?}: {output:?}");
        }
    }
}

fn assert_diagnostic(id: &str, stderr: &str, expected: &str, usage: &str) {
    match expected {
        "<USAGE>" => assert!(
            stderr.starts_with(&format!("usage: craft {usage} ")),
            "{id}: {stderr}"
        ),
        "<GIT>" => assert!(
            stderr.starts_with("Git snapshot failed: "),
            "{id}: {stderr}"
        ),
        "<JSON>" => assert!(
            stderr.starts_with("invalid ") && stderr.contains(" JSON: "),
            "{id}: {stderr}"
        ),
        "<EXISTS>" => assert!(!stderr.trim().is_empty(), "{id}"),
        message => assert_eq!(stderr, format!("{message}\n"), "{id}"),
    }
}

#[test]
fn prepare_matches_the_frozen_corpus() {
    let corpus = corpus();
    let base = scratch().join("prepare");
    let root = base.join("repo");
    build(&root, &corpus["repository"]);
    let existing = base.join("existing.json");
    fs::write(&existing, "keep\n").unwrap();
    for (index, case) in corpus["prepare"].as_array().unwrap().iter().enumerate() {
        let id = case["id"].as_str().unwrap();
        let output = base.join(format!("out-{index}.json"));
        let mut argv = vec!["review-prepare".to_string()];
        argv.extend(case["args"].as_array().unwrap().iter().map(|arg| {
            arg.as_str()
                .unwrap()
                .replace("<ROOT>", &root.to_string_lossy())
                .replace("<OUT>", &output.to_string_lossy())
                .replace("<EXISTING>", &existing.to_string_lossy())
        }));
        let outcome = run(&argv);
        assert_eq!(
            outcome.exit_code as i64,
            case["status"].as_i64().unwrap(),
            "{id}: {}",
            outcome.stderr
        );
        if outcome.exit_code == 0 {
            let mut receipt: Value = serde_json::from_str(&outcome.stdout).unwrap();
            assert_eq!(receipt["artifact"], json!(output.to_string_lossy()), "{id}");
            receipt["artifact"] = json!("<OUT>");
            assert_eq!(receipt, case["stdout"], "{id}");
            assert_eq!(
                outcome.stdout,
                format!("{receipt}\n")
                    .replace("<OUT>", &output.to_string_lossy().replace('\\', "\\\\")),
                "{id}"
            );
            assert_eq!(
                fs::read_to_string(&output).unwrap(),
                pretty(&case["artifact"]),
                "{id}"
            );
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let mode = fs::metadata(&output).unwrap().permissions().mode() & 0o777;
                assert_eq!(format!("{mode:o}"), case["mode"].as_str().unwrap(), "{id}");
            }
        } else {
            assert!(outcome.stdout.is_empty(), "{id}");
            assert_diagnostic(
                id,
                &outcome.stderr,
                case["stderr"].as_str().unwrap(),
                "review-prepare",
            );
            assert!(!output.exists(), "{id}: wrote an artifact");
        }
    }
    assert_eq!(fs::read_to_string(&existing).unwrap(), "keep\n");
}

fn artifact_file(dir: &Path, name: &str, value: &Value, text: &Value) -> PathBuf {
    let file = dir.join(name);
    let bytes = match text {
        Value::String(text) => text.as_bytes().to_vec(),
        Value::Object(raw) => raw["bytes"]
            .as_array()
            .unwrap()
            .iter()
            .map(|b| b.as_u64().unwrap() as u8)
            .collect(),
        _ => serde_json::to_vec(value).unwrap(),
    };
    fs::write(&file, bytes).unwrap();
    file
}

#[test]
fn check_matches_the_frozen_corpus() {
    let dir = scratch().join("check");
    fs::create_dir_all(&dir).unwrap();
    for case in corpus()["check"].as_array().unwrap() {
        let id = case["id"].as_str().unwrap();
        let snapshot = artifact_file(
            &dir,
            "snapshot.json",
            &case["snapshot"],
            &case["snapshotText"],
        );
        let review = artifact_file(&dir, "review.json", &case["review"], &case["reviewText"]);
        let outcome = run(&[
            "review-check".into(),
            "--snapshot".into(),
            snapshot.to_string_lossy().into(),
            "--review".into(),
            review.to_string_lossy().into(),
        ]);
        assert_eq!(
            outcome.exit_code as i64,
            case["status"].as_i64().unwrap(),
            "{id}: {}",
            outcome.stderr
        );
        match case["stdout"].as_str() {
            Some(stdout) => assert_eq!(outcome.stdout, stdout, "{id}"),
            None => assert_diagnostic(
                id,
                &outcome.stderr,
                case["stderr"].as_str().unwrap(),
                "review-check",
            ),
        }
    }
}

#[test]
fn publication_matches_the_frozen_corpus() {
    let dir = scratch().join("publication");
    fs::create_dir_all(&dir).unwrap();
    for case in corpus()["publication"].as_array().unwrap() {
        let id = case["id"].as_str().unwrap();
        let snapshot = artifact_file(&dir, "snapshot.json", &case["snapshot"], &Value::Null);
        let review = artifact_file(&dir, "review.json", &case["review"], &Value::Null);
        let target = &case["target"];
        let mut argv = args(&["review-publication", "--snapshot"]);
        argv.push(snapshot.to_string_lossy().into());
        argv.push("--review".into());
        argv.push(review.to_string_lossy().into());
        for (option, field) in [
            ("--current-base", "currentBase"),
            ("--current-head", "currentHead"),
        ] {
            argv.push(option.into());
            argv.push(target[field].as_str().unwrap().into());
        }
        if target["authorized"] == true {
            argv.push("--authorized".into());
        }
        let outcome = run(&argv);
        match case.get("plan") {
            Some(plan) => {
                assert_eq!(outcome.exit_code, 0, "{id}: {}", outcome.stderr);
                assert_eq!(outcome.stdout, pretty(plan), "{id}");
            }
            None => {
                assert_eq!(outcome.exit_code, 2, "{id}");
                assert_eq!(
                    outcome.stderr,
                    format!("{}\n", case["error"].as_str().unwrap()),
                    "{id}"
                );
            }
        }
    }
}

#[test]
fn usage_and_input_failures_exit_two_without_output() {
    let dir = scratch().join("usage");
    fs::create_dir_all(&dir).unwrap();
    let valid = dir.join("valid.json");
    fs::write(&valid, "{}").unwrap();
    let valid = valid.to_string_lossy().into_owned();
    let absent = dir.join("absent.json").to_string_lossy().into_owned();
    for (argv, expected) in [
        (args(&["review-check"]), "usage: craft review-check "),
        (
            args(&["review-check", "one.json", "two.json"]),
            "usage: craft review-check ",
        ),
        (
            args(&["review-check", "--snapshot", &valid]),
            "usage: craft review-check ",
        ),
        (
            args(&["review-check", "--snapshot", &valid, "--review", ""]),
            "usage: craft review-check ",
        ),
        (
            args(&[
                "review-check",
                "--snapshot",
                &valid,
                "--review",
                &valid,
                "--authorized",
            ]),
            "usage: craft review-check ",
        ),
        (
            args(&["review-check", "--snapshot", &absent, "--review", &valid]),
            "",
        ),
        (
            args(&[
                "review-publication",
                "--snapshot",
                &valid,
                "--review",
                &valid,
                "--current-base",
                "a",
            ]),
            "usage: craft review-publication ",
        ),
        (
            args(&["review-publication", "--authorized", "--authorized"]),
            "usage: craft review-publication ",
        ),
        (
            args(&["review-prepare", "--root"]),
            "usage: craft review-prepare ",
        ),
    ] {
        let outcome = run(&argv);
        assert_eq!(outcome.exit_code, 2, "{argv:?}");
        assert!(outcome.stdout.is_empty(), "{argv:?}");
        assert!(
            outcome.stderr.starts_with(expected) && outcome.stderr.len() > 1,
            "{argv:?}: {}",
            outcome.stderr
        );
    }
    let missing = run(&args(&[
        "review-check",
        "--snapshot",
        &absent,
        "--review",
        &valid,
    ]));
    assert_eq!(missing.exit_code, 2);
    assert!(!missing.stderr.is_empty() && missing.stdout.is_empty());
    let help = run(&args(&["review-check", "--help"]));
    assert_eq!(help.exit_code, 0);
    assert!(
        help.stdout
            .contains("review-check --snapshot snapshot.json --review review.json")
    );
}

#[test]
fn commands_record_a_review_trace() {
    let base = scratch().join("telemetry");
    let root = base.join("repo");
    build(&root, &corpus()["repository"]);
    let output = base.join("snapshot.json").to_string_lossy().into_owned();
    let root_arg = root.to_string_lossy().into_owned();
    let prepared = run(&args(&[
        "review-prepare",
        "--root",
        &root_arg,
        "--base",
        "main",
        "--head",
        "feature",
        "--output",
        &output,
    ]));
    assert_eq!(prepared.exit_code, 0, "{}", prepared.stderr);
    let failed = run(&args(&[
        "review-prepare",
        "--root",
        &root_arg,
        "--base",
        "main",
        "--head",
        "feature",
        "--output",
        &output,
    ]));
    assert_eq!(failed.exit_code, 2);
    let state = root.join(".craft").join("state");
    let mut events = Vec::new();
    let mut pending = vec![state];
    while let Some(dir) = pending.pop() {
        for entry in fs::read_dir(dir).into_iter().flatten().flatten() {
            let path = entry.path();
            if path.is_dir() {
                pending.push(path);
            } else if path.file_name().is_some_and(|name| name == "events.jsonl") {
                let text = fs::read_to_string(path).unwrap();
                events.extend(
                    text.lines()
                        .map(|line| serde_json::from_str::<Value>(line).unwrap()),
                );
            }
        }
    }
    let named = |name: &str| {
        events
            .iter()
            .filter(|e| e["name"] == name)
            .collect::<Vec<_>>()
    };
    let commands = named("review.command");
    assert_eq!(commands.len(), 2, "{events:?}");
    assert_eq!(commands[0]["attributes"]["phase"], "prepared");
    assert_eq!(commands[1]["outcome"], "failure");
    assert_eq!(commands[1]["reasonCode"], "STATE_BUSY");
    let completed = named("review.completed");
    assert!(
        completed
            .iter()
            .all(|e| e["attributes"]["phase"] == "prepared")
    );
    assert!(completed.iter().all(|e| e["parentOperationId"].is_string()));
}
