//! Test commands the host runs are observed from the host's own tool result: the runner's
//! summary decides, a run with no executed test is never a pass, and each run is kept with
//! the tree it ran on.
mod support;
use craft_platform::state;
use serde_json::{Value, json};
use support::Repo;

const CARGO_ZERO: &str = "running 0 tests\n\ntest result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 3 filtered out; finished in 0.00s\n";
const CARGO_PASS: &str = "running 3 tests\n\ntest result: ok. 3 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s\n";
const CARGO_FAIL: &str = "test result: FAILED. 2 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s\n";

/// `craft hook <host> <lifecycle>` for the delivery package, with a host payload.
fn hook(host: &str, lifecycle: &str, payload: Value) -> String {
    let args: Vec<String> = [
        host,
        lifecycle,
        "--package",
        "craft-delivery",
        "--handler",
        "deliver",
    ]
    .iter()
    .map(|a| a.to_string())
    .collect();
    let (wire, _) = craft_application::hook::run(&args, &mut payload.to_string().as_bytes());
    assert_eq!(wire.exit_code, 0, "{}", wire.stderr);
    wire.stdout
}

fn shell(repo: &Repo, command: &str, extra: Value) -> Value {
    let mut payload = json!({
        "session_id": "S1", "cwd": repo.path(), "tool_name": "Bash",
        "tool_input": {"command": command},
    });
    for (key, value) in extra.as_object().unwrap() {
        payload[key] = value.clone();
    }
    payload
}

fn runs(repo: &Repo) -> Vec<Value> {
    let file = state::file(
        std::path::Path::new(repo.path()),
        "test-runs.json",
        false,
        Some("S1"),
    )
    .unwrap();
    let history: Value = serde_json::from_slice(&std::fs::read(file).unwrap()).unwrap();
    history["runs"].as_array().unwrap().clone()
}

#[test]
fn each_host_result_shape_gives_the_runner_verdict_with_the_tree_it_ran_on() {
    let repo = Repo::new("test-runs", &[("src/lib.rs", "pub fn f() {}\n")]);
    // Claude: a clean exit that ran nothing is said at once, in the tool result.
    let said = hook(
        "claude",
        "after-tool",
        shell(
            &repo,
            "cargo test nomatch",
            json!({"tool_response": {"stdout": CARGO_ZERO, "stderr": ""}}),
        ),
    );
    assert!(said.contains("executed no test"), "{said}");
    assert!(said.contains("zero-tests"), "{said}");

    // Claude reports a failing command as its own event; its `error` carries the exit code.
    let failed = hook(
        "claude",
        "after-tool-failure",
        shell(
            &repo,
            "cargo test",
            json!({"error": format!("Exit code 101\n{CARGO_FAIL}")}),
        ),
    );
    assert_eq!(failed, "");

    // A long output: the host keeps the head in `stdout` and the whole in a file.
    let full = std::path::Path::new(repo.path()).join("persisted-output.txt");
    std::fs::write(&full, format!("{}{CARGO_PASS}", "noise\n".repeat(20_000))).unwrap();
    let quiet = hook(
        "claude",
        "after-tool",
        shell(
            &repo,
            "cargo test",
            json!({"tool_response": {
                "stdout": "noise\n".repeat(100), "stderr": "", "persistedOutputPath": full.to_str().unwrap()
            }}),
        ),
    );
    assert!(
        !quiet.contains("Craft test evidence"),
        "a passing run stays quiet: {quiet}"
    );

    // Codex passes the output text with no exit code: the summary decides.
    hook(
        "codex",
        "after-tool",
        shell(&repo, "cargo test -q", json!({"tool_response": CARGO_PASS})),
    );
    // Not a test command: nothing observed.
    hook(
        "codex",
        "after-tool",
        shell(&repo, "cargo build", json!({"tool_response": CARGO_PASS})),
    );

    let runs = runs(&repo);
    let seen: Vec<(&str, &str, &str, Value)> = runs
        .iter()
        .map(|r| {
            (
                r["host"].as_str().unwrap(),
                r["result"].as_str().unwrap(),
                r["reason"].as_str().unwrap(),
                r["exitCode"].clone(),
            )
        })
        .collect();
    assert_eq!(
        seen,
        [
            ("claude", "failed", "zero-tests", json!(0)),
            ("claude", "failed", "exit-code", json!(101)),
            ("claude", "passed", "summary", json!(0)),
            ("codex", "passed", "summary", Value::Null),
        ]
    );
    assert_eq!(runs[2]["executed"], 3);
    let source = &runs[0]["source"];
    assert!(source.is_string(), "a Git tree is fingerprinted: {source}");
    // The persisted file is untracked: the tree changed between the first two runs and it.
    assert_eq!(runs[1]["source"], runs[0]["source"]);
    assert_ne!(runs[2]["source"], runs[0]["source"]);
}

#[test]
fn a_resumed_task_hears_whether_its_last_test_run_still_holds() {
    let repo = Repo::new(
        "test-runs-resume",
        &[("value.mjs", "export const value = 1;\n")],
    );
    repo.cli(&[
        "task-start",
        "--session",
        "S1",
        "--task",
        "fix",
        "--goal",
        "Fix it",
        "--file",
        "value.mjs",
    ])
    .unwrap();
    let pass = "ℹ tests 2\nℹ pass 2\nℹ fail 0\nℹ skipped 0\n";
    hook(
        "claude",
        "after-tool",
        shell(
            &repo,
            "node --test",
            json!({"tool_response": {"stdout": pass}}),
        ),
    );
    let resume = |repo: &Repo| {
        let facts = json!({
            "handler": "deliver", "host": "claude", "package": "craft-delivery", "lifecycle": "after-compact",
            "scope": "any", "inputStatus": "complete", "tool": "other", "contextSupported": true,
            "cwd": repo.path(), "session": "S1", "file": null, "prompt": "", "pattern": null,
        });
        craft_application::host::run(&facts).unwrap()["context"]["data"]["tests"].clone()
    };
    let held = resume(&repo);
    assert_eq!(held["status"], "passed");
    assert_eq!(held["executed"], 2);
    // An edit since the run: the run no longer speaks for the tree.
    repo.write("value.mjs", "export const value = 2;\n");
    assert_eq!(resume(&repo)["status"], "not-run");
}
