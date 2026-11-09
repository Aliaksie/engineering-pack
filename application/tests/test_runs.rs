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

const CONTRACT: &str = include_str!("../../tests/fixtures/acceptance/contract.json");

/// A verify step that replaces the ledger with `records` (format `version`).
fn submit(
    repo: &Repo,
    version: u64,
    records: Value,
    outcome: &str,
    complete: bool,
) -> Result<Value, String> {
    let bundle = support::bundle();
    let expected = repo
        .cli(&["session", "--session", "S1", "--bundle", &bundle])
        .unwrap();
    let ledger = repo.input(
        "ledger.json",
        &json!({"formatVersion": version, "records": records}),
    );
    let mut args = vec![
        "task-step",
        "--session",
        "S1",
        "--stage",
        "verify",
        "--target",
        "acceptance:value",
        "--approach",
        "unit-test",
        "--evidence",
        "observed run",
        "--outcome",
        outcome,
        "--result",
        "Checked",
        "--next",
        "Finish",
        "--acceptance-evidence",
        &ledger,
        "--expected-task-contract",
        expected["taskContract"].as_str().unwrap(),
        "--expected-evidence-digest",
        expected["evidenceDigest"].as_str().unwrap(),
    ];
    args.extend(["--bundle", &bundle]);
    if complete {
        args.push("--complete");
    }
    repo.cli(&args)
}

fn observed_record(contract_digest: &str) -> Value {
    json!({"id": "run", "criterionIds": ["value", "error"], "scope": ["src"], "contract": contract_digest,
        "checkId": "value-check", "kind": "check", "tests": "observed"})
}

#[test]
fn a_ledger_record_takes_the_run_craft_observed_and_a_run_of_no_tests_cannot_complete() {
    let repo = Repo::new(
        "test-runs-ledger",
        &[("src/value.mjs", "export const value = 1;\n")],
    );
    let contract = repo.input(
        "contract.json",
        &serde_json::from_str::<Value>(CONTRACT).unwrap(),
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
        "src",
        "--contract",
        &contract,
    ])
    .unwrap();
    let digest = craft_engine::session::acceptance::contract_digest(
        &serde_json::from_str(CONTRACT).unwrap(),
    )
    .unwrap();

    // Nothing observed yet: the request is refused, not filled with a guess.
    let none = submit(&repo, 2, json!([observed_record(&digest)]), "passed", false).unwrap_err();
    assert!(none.contains("no test run observed"), "{none}");

    // A filter that matched nothing exits 0; the criterion fails and completion is refused.
    hook(
        "claude",
        "after-tool",
        shell(
            &repo,
            "node --test --test-name-pattern=nomatch",
            json!({"tool_response": {"stdout": "✔ value.test.mjs (5ms)\nℹ tests 1\nℹ pass 1\nℹ fail 0\n"}}),
        ),
    );
    let zero = submit(
        &repo,
        2,
        json!([observed_record(&digest)]),
        "quality-failure",
        false,
    )
    .unwrap();
    assert_eq!(
        zero["acceptance"]["criteria"][0]["status"], "failed",
        "{zero}"
    );
    assert_eq!(zero["acceptance"]["criteria"][0]["reason"], "zero-tests");
    // Declaring the work passed and complete over that run is refused.
    let refused = submit(&repo, 2, json!([observed_record(&digest)]), "passed", true).unwrap_err();
    assert!(
        refused.contains("contradicts the latest test run"),
        "{refused}"
    );

    // A run with executed tests on this tree verifies both criteria, bound by Craft.
    hook(
        "claude",
        "after-tool",
        shell(
            &repo,
            "node --test",
            json!({"tool_response": {"stdout": "✔ returns value (1ms)\n✔ keeps error (1ms)\nℹ tests 2\nℹ pass 2\nℹ fail 0\n"}}),
        ),
    );
    let done = submit(&repo, 2, json!([observed_record(&digest)]), "passed", true).unwrap();
    assert_eq!(done["complete"], true, "{done}");
    let saved = &done["acceptance"]["records"][0];
    assert_eq!(
        saved["tests"],
        json!({"runner": "node", "executed": 2, "failed": 0, "skipped": 0, "source": "observed"})
    );
    assert_eq!(saved["freshness"], "current");

    // An observed record needs the format that carries test counts.
    let old = submit(&repo, 1, json!([observed_record(&digest)]), "passed", false).unwrap_err();
    assert!(old.contains("formatVersion 2"), "{old}");
}

#[test]
fn a_declared_pass_is_refused_when_the_latest_run_on_the_tree_failed() {
    let repo = Repo::new(
        "test-runs-verify",
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
    let bundle = support::bundle();
    let verify = |repo: &Repo| {
        repo.cli(&[
            "task-step",
            "--session",
            "S1",
            "--bundle",
            &bundle,
            "--stage",
            "verify",
            "--target",
            "acceptance:value",
            "--approach",
            "unit-test",
            "--evidence",
            "tests",
            "--outcome",
            "passed",
            "--result",
            "Passed",
            "--next",
            "Finish",
        ])
    };
    hook(
        "claude",
        "after-tool",
        shell(
            &repo,
            "cargo test nomatch",
            json!({"tool_response": {"stdout": CARGO_ZERO}}),
        ),
    );
    let refused = verify(&repo).unwrap_err();
    assert!(
        refused.contains("contradicts the latest test run"),
        "{refused}"
    );
    assert!(refused.contains("zero-tests"), "{refused}");
    // The tree changed since: no run speaks for it, and the declaration stands as before.
    repo.write("value.mjs", "export const value = 2;\n");
    assert!(verify(&repo).is_ok());
}

#[test]
fn a_result_without_output_or_exit_code_records_nothing() {
    let repo = Repo::new("test-runs-empty", &[("src/lib.rs", "pub fn f() {}\n")]);
    hook(
        "codex",
        "after-tool",
        shell(&repo, "cargo test", json!({"tool_response": CARGO_PASS})),
    );
    // Codex while the command still runs: no output, no exit code.
    hook(
        "codex",
        "after-tool",
        shell(&repo, "cargo test", json!({"tool_response": null})),
    );
    hook("codex", "after-tool", shell(&repo, "cargo test", json!({})));
    let runs = runs(&repo);
    assert_eq!(runs.len(), 1);
    assert_eq!(runs[0]["result"], "passed");
}
