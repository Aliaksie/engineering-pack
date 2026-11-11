use super::*;
use crate::routing;

fn project() -> Value {
    json!({
        "formatVersion": 1,
        "modules": [
            {"name": "api", "path": "services/api", "checks": [
                {"command": "cargo", "args": ["test"]},
                {"command": "cargo", "args": ["fmt", "--check"], "id": "api-format", "stage": "step", "timeoutSeconds": 30},
                {"command": "cargo", "args": ["test", "--features", "slow"], "stage": "ci"}
            ]},
            {"name": "ui", "path": "ui", "checks": [{"command": "pnpm", "args": ["test"], "stage": "pr"}]}
        ],
        "standards": [
            {"id": "lint", "run": {"command": "pnpm", "args": ["lint"], "cwd": "ui", "stage": "step"},
             "scope": {"paths": ["ui/"]}},
            {"id": "licenses", "run": {"command": "./tools/licenses"}},
            {"id": "notes", "file": "docs/notes.md"}
        ]
    })
}

fn planned_ids(
    settings: &Value,
    files: &[&str],
    stage: &str,
    all: bool,
) -> (Vec<String>, Vec<String>) {
    let files: Vec<Value> = files.iter().map(|file| json!(file)).collect();
    let scope = routing::scope(settings, &files).unwrap();
    let plan = plan(settings, &scope, stage, all).unwrap();
    let ids = |field: &str| {
        plan[field]
            .as_array()
            .unwrap()
            .iter()
            .map(|check| check["id"].as_str().unwrap().to_string())
            .collect()
    };
    (ids("run"), ids("left"))
}

#[test]
fn a_stage_runs_its_checks_and_earlier_ones_and_leaves_the_later() {
    let settings = project();
    let (run, left) = planned_ids(&settings, &["services/api/src/a.rs"], "commit", false);
    assert_eq!(run, ["api-1", "api-format", "licenses"]);
    assert_eq!(left, ["api-3"]);
    let (run, left) = planned_ids(&settings, &["services/api/src/a.rs"], "step", false);
    assert_eq!(run, ["api-format"]);
    assert_eq!(left, ["api-1", "api-3", "licenses"]);
    let (run, left) = planned_ids(&settings, &["services/api/src/a.rs"], "ci", false);
    assert_eq!(run.len(), 4);
    assert!(left.is_empty());
}

#[test]
fn only_checks_of_the_changed_scope_are_planned_unless_all_are_asked_for() {
    let settings = project();
    let (run, left) = planned_ids(&settings, &["ui/src/App.tsx"], "commit", false);
    assert_eq!(run, ["lint", "licenses"]);
    assert_eq!(left, ["ui-1"]);
    // No changed file: nothing is in scope, also no unscoped standard.
    let (run, left) = planned_ids(&settings, &[], "ci", false);
    assert!(run.is_empty() && left.is_empty());
    let (run, _) = planned_ids(&settings, &[], "ci", true);
    assert_eq!(
        run,
        ["api-1", "api-format", "api-3", "ui-1", "lint", "licenses"]
    );
}

#[test]
fn a_planned_check_carries_its_place_stage_and_bound() {
    let settings = project();
    let scope = routing::scope(&settings, &[json!("ui/a.ts")]).unwrap();
    let plan = plan(&settings, &scope, "pr", false).unwrap();
    assert_eq!(
        plan["run"][0],
        json!({"id": "ui-1", "kind": "module", "module": "ui", "command": "pnpm", "args": ["test"],
            "cwd": "ui", "stage": "pr", "timeoutSeconds": 600})
    );
    assert_eq!(plan["run"][1]["standard"], "lint");
    assert_eq!(plan["run"][1]["cwd"], "ui");
    assert_eq!(plan["run"][2]["cwd"], ".");
    assert_eq!(plan["run"][2]["stage"], "commit");
    assert!(super::plan(&settings, &scope, "nightly", false).is_err());
}

#[test]
fn two_checks_with_one_id_are_refused() {
    let mut settings = project();
    settings["modules"][0]["checks"][1]["id"] = json!("lint");
    let scope = routing::scope(&settings, &[]).unwrap();
    let error = plan(&settings, &scope, "ci", true).unwrap_err();
    assert!(error.contains("share the id lint"), "{error}");
}

#[test]
fn declared_stage_bound_and_id_are_validated() {
    let check = |value: Value| validate(value.as_object().unwrap(), "check");
    assert!(check(json!({"stage": "pr", "timeoutSeconds": 3600, "id": "a-1"})).is_ok());
    assert!(check(json!({"stage": "nightly"})).is_err());
    assert!(check(json!({"timeoutSeconds": 0})).is_err());
    assert!(check(json!({"timeoutSeconds": 3601})).is_err());
    assert!(check(json!({"timeoutSeconds": 1.5})).is_err());
    assert!(check(json!({"id": "Not Kebab"})).is_err());
}

fn ended(exit: i64, output: &str) -> Value {
    json!({"started": true, "exitCode": exit, "timedOut": false, "durationMs": 12, "output": output})
}

#[test]
fn only_a_command_that_started_ended_in_time_and_exited_zero_passes() {
    let check = json!({"id": "a", "stage": "commit"});
    let passed = verdict(&check, &ended(0, "fine\n"));
    assert_eq!(passed["result"], "passed");
    assert!(
        passed.get("output").is_none(),
        "a passing check keeps no output"
    );
    let failed = verdict(&check, &ended(2, "boom\n"));
    assert_eq!(
        (&failed["result"], &failed["reason"]),
        (&json!("failed"), &json!("exit-code"))
    );
    assert_eq!(failed["output"], "boom");
    let missing = verdict(
        &check,
        &json!({"started": false, "error": "not found", "durationMs": 0}),
    );
    assert_eq!(missing["result"], "not-started");
    assert_eq!(missing["error"], "not found");
    let slow = verdict(
        &check,
        &json!({"started": true, "timedOut": true, "exitCode": null, "durationMs": 1000, "output": "…"}),
    );
    assert_eq!(slow["result"], "timed-out");
}

#[test]
fn a_test_command_that_executed_no_test_does_not_pass() {
    let check = json!({"id": "tests", "stage": "commit"});
    let empty = verdict(
        &check,
        &ended(
            0,
            "running 0 tests\n\ntest result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 3 filtered out; finished in 0.00s\n",
        ),
    );
    assert_eq!(
        (&empty["result"], &empty["reason"]),
        (&json!("failed"), &json!("zero-tests"))
    );
    let ran = verdict(
        &check,
        &ended(
            0,
            "running 2 tests\n\ntest result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s\n",
        ),
    );
    assert_eq!(ran["result"], "passed");
    assert_eq!(ran["tests"]["executed"], 2);
}

#[test]
fn long_output_is_shown_from_its_end() {
    let check = json!({"id": "a", "stage": "commit"});
    let output = format!("{}é\nlast line\n", "x".repeat(SHOWN_BYTES));
    let failed = verdict(&check, &ended(1, &output));
    let shown = failed["output"].as_str().unwrap();
    assert!(shown.len() <= SHOWN_BYTES && shown.ends_with("last line"));
    assert_eq!(failed["outputTruncated"], true);
}

#[test]
fn a_report_never_counts_a_left_or_unrun_check_as_passed() {
    let settings = project();
    let scope = routing::scope(&settings, &[json!("services/api/a.rs")]).unwrap();
    let plan = plan(&settings, &scope, "commit", false).unwrap();
    let results: Vec<Value> = plan["run"]
        .as_array()
        .unwrap()
        .iter()
        .map(|check| verdict(check, &ended(0, "")))
        .collect();
    let report = report(
        &plan,
        results,
        &json!("A"),
        &json!("A"),
        "2026-11-10T00:00:00.000Z",
    );
    assert_eq!(report["result"], "passed");
    assert_eq!(report["source"], "A");
    assert_eq!(report["treeChanged"], false);
    assert_eq!(report["left"][0]["result"], "left-to-ci");
    assert_eq!(
        report["summary"],
        json!({"run": 3, "passed": 3, "notPassed": 0, "left": 1})
    );
    assert!(text(&report).contains("left-to-ci api-3 (ci): cargo test --features slow"));

    let nothing = super::report(
        &json!({"stage": "step", "run": [], "left": plan["left"]}),
        Vec::new(),
        &json!("A"),
        &json!("B"),
        "t",
    );
    assert_eq!(nothing["result"], "nothing-run");
    assert_eq!(nothing["treeChanged"], true);
}

#[test]
fn a_session_history_keeps_the_newest_runs_without_output() {
    let mut history = Value::Null;
    for index in 0..HISTORY + 3 {
        let report = json!({"at": index, "stage": "commit", "result": "failed", "source": "A",
            "treeChanged": false,
            "checks": [{"id": "a", "stage": "commit", "result": "failed", "reason": "exit-code",
                "exitCode": 1, "durationMs": 5, "output": "secret-looking text"}],
            "left": []});
        history = recorded(&history, &report);
    }
    let runs = history["runs"].as_array().unwrap();
    assert_eq!(runs.len(), HISTORY);
    assert_eq!(runs[0]["at"], 3);
    assert!(!history.to_string().contains("secret-looking"));
}

#[test]
fn the_switch_is_on_unless_turned_off() {
    assert!(enabled(&json!({})));
    assert!(!enabled(&json!({"routing": {"projectChecks": false}})));
}
