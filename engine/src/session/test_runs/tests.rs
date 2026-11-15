use super::*;

#[test]
fn test_commands_are_recognised_through_wrappers_and_chains() {
    for command in [
        "cargo test -q -p craft-engine --lib standards",
        "cd app && npm test",
        "npm run test:unit",
        "pnpm test",
        "CI=1 npx --no-install vitest run",
        "uv run pytest -k orders",
        "python3 -m pytest tests/",
        "python -m unittest discover",
        "node --test tests/*.test.mjs 2>&1 | tail -5",
        "./mvnw -B verify",
        "./gradlew test",
        "go test ./...",
        ".\\gradlew.bat test",
        "gradlew test",
        "mvnw.cmd -B verify",
        "npm.cmd test",
        "C:\\Users\\dev\\.cargo\\bin\\cargo.exe test",
        "py -m pytest",
        "py -3.12 -m pytest -q",
    ] {
        assert!(is_test_command(command), "{command}");
    }
    for command in [
        "cargo build",
        "git commit -m test",
        "grep -rn test src",
        "npm install",
        "node scripts/build.mjs",
        "ls tests",
        "py -c test",
        "cargo.exe build",
    ] {
        assert!(!is_test_command(command), "{command}");
    }
}

#[test]
fn a_success_text_from_an_unknown_runner_is_unknown_and_its_failure_failed() {
    let ok = classify(Some(0), "All good! 12 checks fine.\n");
    assert_eq!(ok["result"], "unknown");
    assert_eq!(ok["runner"], Value::Null);
    assert_eq!(classify(Some(1), "boom\n")["result"], "failed");
}

#[test]
fn a_summary_with_failures_fails_even_when_the_exit_code_was_lost_in_a_pipe() {
    let output = "test result: FAILED. 2 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s\n";
    let verdict = classify(Some(0), output);
    assert_eq!(verdict["result"], "failed");
    assert_eq!(verdict["reason"], "failures");
    // No exit code from the host: the summary alone decides.
    assert_eq!(classify(None, output)["result"], "failed");
}

#[test]
fn output_a_pipe_cut_says_nothing_about_tests_it_does_not_show() {
    // Cargo ran a library with 10 tests; `tail` kept only the last binary's summary.
    let last = "running 0 tests\n\ntest result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 1 filtered out; finished in 0.00s\n";
    let cut = "cargo test -q -p craft-engine layers 2>&1 | tail -4";
    assert_eq!(classify(Some(0), last)["reason"], "zero-tests");
    let verdict = classify_command(Some(0), last, cut);
    assert_eq!(
        (verdict["result"].as_str(), verdict["reason"].as_str()),
        (Some("unknown"), Some("cut-output"))
    );
    let passed = "test result: ok. 3 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s\n";
    assert_eq!(classify_command(Some(0), passed, cut)["result"], "unknown");
    let failed = "test result: FAILED. 9 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s\n";
    assert_eq!(classify_command(Some(0), failed, cut)["reason"], "failures");
    // Whole output: kept by `tee` and `cat`, or a pipe that comes before the runner.
    for whole in [
        "cargo test 2>&1 | tee test.log",
        "cargo test | cat",
        "printf 'y' | cargo test",
        "cargo test && git status | head -3",
        "cargo test || echo failed | tail -1",
    ] {
        assert_eq!(
            classify_command(Some(0), last, whole)["reason"],
            "zero-tests",
            "{whole}"
        );
    }
    for cut in [
        "npm test |& tail -5",
        "pytest -q | grep -v PASSED",
        "cargo test 2>&1 | LC_ALL=C sed -n '1,40p'",
    ] {
        assert_eq!(
            classify_command(Some(0), last, cut)["reason"],
            "cut-output",
            "{cut}"
        );
    }
}

#[test]
fn output_cut_before_the_summary_is_unknown() {
    let verdict = classify(Some(0), "running 3 tests\ntest a ... ok\n");
    assert_eq!(verdict["result"], "unknown");
}

#[test]
fn only_a_run_on_the_current_tree_speaks_for_it() {
    let pass = classify(
        Some(0),
        "test result: ok. 3 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s\n",
    );
    let zero = classify(
        Some(0),
        "test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 3 filtered out; finished in 0.00s\n",
    );
    let mut history = Value::Null;
    history = observed(
        &history,
        observation(&pass, "cargo test", "claude", "t1", &json!("A")),
    );
    history = observed(
        &history,
        observation(&zero, "cargo test nomatch", "claude", "t2", &json!("B")),
    );
    assert_eq!(status(&history, &json!("A"))["status"], "passed");
    assert_eq!(status(&history, &json!("B"))["status"], "failed");
    assert_eq!(status(&history, &json!("B"))["reason"], "zero-tests");
    // An edit since the run: the tree has no run yet.
    assert_eq!(status(&history, &json!("C"))["status"], "not-run");
    // An unfingerprinted tree is never matched.
    let blind = observed(
        &Value::Null,
        observation(&pass, "cargo test", "codex", "t3", &Value::Null),
    );
    assert_eq!(status(&blind, &Value::Null)["status"], "not-run");
}

#[test]
fn a_run_that_checked_nothing_leaves_an_earlier_verdict_on_the_tree_standing() {
    let pass = classify(
        Some(0),
        "test result: ok. 3 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s\n",
    );
    let fail = classify(
        Some(101),
        "test result: FAILED. 2 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s\n",
    );
    let zero = classify(Some(0), "[INFO] Tests are skipped.\n");
    let unread = classify(Some(0), "");
    let empty_exit = classify(Some(1), "No tests found, exiting with code 1\n");
    assert_eq!(empty_exit["reason"], "zero-tests");
    let at = |history: &Value, verdict: &Value, command: &str| {
        observed(
            history,
            observation(verdict, command, "claude", "t", &json!("A")),
        )
    };
    let mut history = at(&Value::Null, &pass, "cargo test");
    for (verdict, command) in [
        (&zero, "mvn -DskipTests install"),
        (&unread, "cargo test --no-run"),
        (&empty_exit, "npx jest nomatch"),
    ] {
        history = at(&history, verdict, command);
        assert_eq!(
            status(&history, &json!("A"))["status"],
            "passed",
            "{command}"
        );
        assert_eq!(refutes_pass(&status(&history, &json!("A"))), None);
    }
    // A failure is decisive and stands over the pass before it, whatever follows it.
    history = at(&history, &fail, "cargo test");
    history = at(&history, &zero, "cargo test nomatch");
    assert_eq!(status(&history, &json!("A"))["reason"], "exit-code");
    assert!(refutes_pass(&status(&history, &json!("A"))).is_some());
    // With no decisive run, the latest run is what the tree has.
    let alone = at(&Value::Null, &zero, "cargo test nomatch");
    assert_eq!(status(&alone, &json!("A"))["reason"], "zero-tests");
}

#[test]
fn observed_counts_are_vouched_for_only_by_a_run_or_the_saved_ledger() {
    let fail = classify(Some(2), "boom\n");
    let pass = classify(Some(0), "Ran 2 tests in 0.0s\n\nOK\n");
    let mut history = Value::Null;
    for (verdict, at) in [(&fail, "t1"), (&pass, "t2")] {
        history = observed(
            &history,
            observation(verdict, "pytest", "codex", at, &json!("A")),
        );
    }
    let asked = json!({"id": "r", "tests": "observed"});
    let filled = evidence(&asked, current(&history, &json!("A")).unwrap()).unwrap();
    assert_eq!(filled["tests"]["executed"], 2);
    assert!(vouched(&filled, &history, &Value::Null));
    // A failure without a summary has no counts: unknown, not zero.
    let failed = evidence(&asked, &history["runs"][0]).unwrap();
    assert_eq!(
        failed["tests"],
        json!({"runner": null, "source": "observed"})
    );
    // Counts changed by hand, or a run Craft never saw, are not vouched for...
    let mut forged = filled.clone();
    forged["tests"]["executed"] = json!(9);
    assert!(!vouched(&forged, &history, &Value::Null));
    // ...unless the saved ledger already holds the record as it is.
    let saved = json!({"formatVersion": 2, "records": [forged.clone()]});
    assert!(vouched(&forged, &Value::Null, &saved));
    let declared = json!({"tests": {"source": "declared", "runner": "x", "executed": 1}});
    assert!(vouched(&declared, &Value::Null, &Value::Null));
}

#[test]
fn the_history_keeps_the_latest_runs_and_short_commands() {
    let pass = classify(Some(0), "Ran 1 test in 0.0s\n\nOK\n");
    let mut history = Value::Null;
    for n in 0..KEPT_RUNS + 5 {
        let run = observation(
            &pass,
            &format!("pytest {n} {}", "x".repeat(400)),
            "codex",
            "t",
            &json!(n),
        );
        history = observed(&history, run);
    }
    let runs = history["runs"].as_array().unwrap();
    assert_eq!(runs.len(), KEPT_RUNS);
    assert_eq!(runs[0]["source"], 5);
    assert_eq!(
        runs[0]["command"].as_str().unwrap().chars().count(),
        COMMAND_CHARS
    );
}
