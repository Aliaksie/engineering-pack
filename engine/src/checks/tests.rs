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
        json!({"run": 3, "passed": 3, "notPassed": 0, "skipped": 0, "left": 1})
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

#[test]
fn changed_files_stand_only_as_a_whole_argument() {
    let check = |args: Value| validate(json!({"args": args}).as_object().unwrap(), "check");
    assert!(check(json!(["--fix", CHANGED_FILES])).is_ok());
    assert!(check(json!(["--files={changedFiles}"])).is_err());
}

#[test]
fn a_check_takes_the_changed_files_it_covers_named_from_its_directory() {
    let mut settings = project();
    settings["modules"][1]["checks"][0]["args"] = json!(["lint", CHANGED_FILES]);
    settings["standards"][0]["run"]["args"] = json!(["prettier", CHANGED_FILES]);
    let changed = [
        json!("ui/a.ts"),
        json!("ui/deep/b.ts"),
        json!("services/api/src/c.rs"),
        json!("README.md"),
    ];
    let scope = routing::scope(&settings, &changed).unwrap();
    let plan = plan(&settings, &scope, "pr", false).unwrap();
    let find = |id: &str| -> Value {
        plan["run"]
            .as_array()
            .unwrap()
            .iter()
            .find(|check| check["id"] == id)
            .cloned()
            .unwrap()
    };
    let (module, standard, other) = (find("ui-1"), find("lint"), find("api-1"));
    assert!(takes_files(&module) && takes_files(&standard) && !takes_files(&other));
    let files = files_for(&settings, &module, &changed).unwrap();
    assert_eq!(files, ["a.ts", "deep/b.ts"]);
    assert_eq!(files_for(&settings, &standard, &changed).unwrap(), files);
    assert_eq!(
        arguments(&module, &files).unwrap(),
        ["lint", "a.ts", "deep/b.ts"]
    );
    assert_eq!(arguments(&other, &[]).unwrap(), ["test"]);
    assert!(
        files_for(&settings, &module, &[json!("README.md")])
            .unwrap()
            .is_empty()
    );

    let many: Vec<String> = (0..5000).map(|n| format!("src/file-{n:05}.ts")).collect();
    assert!(arguments(&module, &many).unwrap_err().contains("--file"));
}

#[test]
fn a_skipped_check_is_reported_and_never_passed() {
    let check =
        json!({"id": "lint", "stage": "commit", "command": "pnpm", "args": [CHANGED_FILES]});
    let plan = json!({"stage": "commit", "run": [check], "left": []});
    let report = report(&plan, vec![skipped(&check)], &json!("d"), &json!("d"), "t");
    assert_eq!(report["result"], "nothing-run");
    assert_eq!(report["summary"]["skipped"], 1);
    assert_eq!(report["skipped"][0]["reason"], "no-changed-file");
    assert!(text(&report).contains("1 skipped with no changed file"));
    let history = recorded(&json!({}), &report);
    assert_eq!(history["runs"][0]["skipped"][0]["id"], "lint");
}

#[test]
fn the_latest_run_is_counted_and_current_only_on_its_own_tree() {
    assert!(latest(&json!({}), &json!("A")).is_none());
    let plan = json!({"stage": "commit", "run": [], "left": [{"id": "slow", "stage": "ci"}]});
    let passed = verdict(&json!({"id": "fmt"}), &ended(0, "ok"));
    let older = recorded(
        &json!({}),
        &report(&plan, vec![], &json!("A"), &json!("A"), "t1"),
    );
    let history = recorded(
        &older,
        &report(&plan, vec![passed], &json!("B"), &json!("B"), "t2"),
    );
    let last = latest(&history, &json!("B")).unwrap();
    assert_eq!(
        last,
        json!({"at": "t2", "stage": "commit", "result": "passed", "run": 1, "passed": 1,
            "skipped": 0, "left": 1, "current": true})
    );
    assert_eq!(latest(&history, &json!("C")).unwrap()["current"], false);
    assert_eq!(
        latest(&history, &Value::Null).unwrap()["current"],
        Value::Null
    );
}

fn settings() -> Value {
    let mut settings = crate::contracts::default_settings();
    let project = project();
    settings["modules"] = project["modules"].clone();
    settings["standards"] = project["standards"].clone();
    settings
}

fn run(stage: &str, source: &str, checks: Value, skipped: Value) -> Value {
    json!({"stage": stage, "result": "x", "source": source, "checks": checks, "skipped": skipped, "left": []})
}

#[test]
fn a_commit_is_held_until_a_run_on_the_current_files_passed_every_covering_check() {
    let settings = settings();
    let covering = covering(&settings, &[json!("services/api/src/lib.rs")]).unwrap();
    let ids: Vec<&str> = covering.iter().filter_map(|c| c["id"].as_str()).collect();
    assert_eq!(
        ids.len(),
        3,
        "two api checks and the repository-wide standard: {ids:?}"
    );
    let passed = |id: &str| json!({"id": id, "result": "passed"});
    let all_passed = json!(ids.iter().map(|id| passed(id)).collect::<Vec<_>>());
    let held = |history: Value| commit_hold(&covering, &history, &json!("tree-2"), "S1");

    // Never run.
    let hold = held(json!(null)).unwrap();
    assert_eq!(hold["decision"], "deny");
    assert!(
        hold["reason"]
            .as_str()
            .unwrap()
            .contains("craft check --stage commit --session S1")
    );
    // Passed, but on other files.
    assert!(
        held(json!({"runs": [run("commit", "tree-1", all_passed.clone(), json!([]))]})).is_some()
    );
    // Passed on these files, at the commit stage or later.
    assert!(
        held(json!({"runs": [run("commit", "tree-2", all_passed.clone(), json!([]))]})).is_none()
    );
    assert!(held(json!({"runs": [run("ci", "tree-2", all_passed.clone(), json!([]))]})).is_none());
    // A step-stage run leaves the commit-stage checks out.
    assert!(
        held(json!({"runs": [run("step", "tree-2", all_passed.clone(), json!([]))]})).is_some()
    );
    // The newest run on these files decides: a failure there names the check.
    let mut failed = all_passed.clone();
    failed[0]["result"] = json!("failed");
    let hold = held(json!({"runs": [
        run("commit", "tree-2", all_passed.clone(), json!([])),
        run("commit", "tree-2", failed, json!([]))
    ]}))
    .unwrap();
    let reason = hold["reason"].as_str().unwrap();
    assert!(
        reason.contains(&format!("{} did not pass", ids[0])),
        "{reason}"
    );
    // A check skipped for want of a file counts as covered.
    let mut some = all_passed.clone();
    let skipped = json!([some.as_array_mut().unwrap().remove(0)]);
    assert!(held(json!({"runs": [run("commit", "tree-2", some, skipped)]})).is_none());
}

#[test]
fn no_hold_without_a_covering_check_or_with_the_gate_off() {
    let settings = settings();
    // The ui module's own check runs at the pr stage: it never holds a commit.
    let ui = covering(&settings, &[json!("ui/app.ts")]).unwrap();
    assert!(ui.iter().all(|c| c["stage"] != "pr"), "{ui:?}");
    assert!(covering(&settings, &[]).unwrap().is_empty());
    assert!(commit_hold(&[], &json!(null), &json!("t"), "S1").is_none());
    // No tree digest (no checkout): nothing to bind a run to, so nothing is held.
    assert!(commit_hold(&ui, &json!(null), &json!(null), "S1").is_none());
    let mut off = settings.clone();
    off["routing"]["commitGate"] = json!(false);
    assert!(!gates_commits(&off));
    off["routing"]["commitGate"] = json!(true);
    off["routing"]["projectChecks"] = json!(false);
    assert!(!gates_commits(&off));
    assert!(gates_commits(&settings));
}
