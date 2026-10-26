//! `craft review-followup`: argument rules and exit statuses over observation files.
use craft_application::cli;
use serde_json::{Value, json};
use std::fs;
use std::path::PathBuf;

const HEAD: &str = "1111111111111111111111111111111111111111";

fn args(values: &[&str]) -> Vec<String> {
    values.iter().map(|value| value.to_string()).collect()
}

/// A scratch working directory, so command telemetry never lands in this repository.
fn scratch() -> PathBuf {
    let dir = std::env::temp_dir().join(format!("craft-review-followup-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let dir = dir.canonicalize().unwrap();
    std::env::set_current_dir(&dir).unwrap();
    dir
}

fn observation(more_comments: bool) -> Value {
    let page =
        |nodes: Value, more: bool| json!({"pageInfo": {"hasNextPage": more}, "nodes": nodes});
    json!({"data": {"repository": {"pullRequest": {
        "number": 7, "state": "OPEN", "headRefOid": HEAD, "author": {"login": "developer"},
        "commits": {"nodes": [{"commit": {"oid": HEAD, "committedDate": "2026-08-01T00:00:00Z"}}]},
        "comments": page(json!([
            {"author": {"login": "auditor"}, "createdAt": "2026-08-10T00:00:00Z", "body": "Why?"},
            {"author": {"login": "developer"}, "createdAt": "2026-08-11T00:00:00Z", "body": "Because."}
        ]), more_comments),
        "reviews": page(json!([]), false),
        "reviewThreads": page(json!([]), false),
    }}}})
}

#[test]
fn followup_reports_status_and_completeness_through_its_exit_code() {
    let dir = scratch();
    let complete = dir.join("complete.json");
    let truncated = dir.join("truncated λ.json");
    fs::write(&complete, observation(false).to_string()).unwrap();
    fs::write(&truncated, observation(true).to_string()).unwrap();
    let base = |file: &PathBuf| {
        args(&[
            "review-followup",
            "--observation",
            file.to_str().unwrap(),
            "--reviewer",
            "auditor",
        ])
    };

    let mut argv = base(&complete);
    argv.extend(args(&["--now", "2026-08-12T00:00:00Z"]));
    let outcome = cli::run(&argv);
    assert_eq!(outcome.exit_code, 0, "{}", outcome.stderr);
    let result: Value = serde_json::from_str(&outcome.stdout).unwrap();
    assert_eq!(result["complete"], true);
    assert_eq!(result["now"], "2026-08-12T00:00:00Z");
    assert_eq!(result["pullRequests"][0]["status"], "answered");

    // Without --now the check runs for the current time, long after this contribution.
    let mut argv = base(&complete);
    argv.extend(args(&["--stale-days", "0"]));
    let result: Value = serde_json::from_str(&cli::run(&argv).stdout).unwrap();
    assert_eq!(
        result["pullRequests"][0]["signals"]["replyAfterReviewer"],
        true
    );
    assert!(
        result["pullRequests"][0]["signals"]["daysSinceReviewer"]
            .as_i64()
            .unwrap()
            > 0
    );

    let outcome = cli::run(&base(&truncated));
    assert_eq!(outcome.exit_code, 1);
    let result: Value = serde_json::from_str(&outcome.stdout).unwrap();
    assert_eq!(result["complete"], false);
    assert_eq!(
        result["pullRequests"][0]["incomplete"],
        "comments truncated; fetch the rest before deciding"
    );

    let help = cli::run(&args(&["review-followup", "--help"]));
    assert_eq!(help.exit_code, 0);
    assert!(help.stdout.contains("review-followup --observation"));

    let file = complete.to_str().unwrap();
    let missing = dir.join("missing.json");
    let invalid = dir.join("invalid.json");
    fs::write(&invalid, "{").unwrap();
    for (argv, expected) in [
        (
            args(&["review-followup", "--observation", file]),
            "usage: craft review-followup ",
        ),
        (
            args(&["review-followup", "--reviewer", "auditor"]),
            "usage: craft review-followup ",
        ),
        (
            args(&[
                "review-followup",
                "--observation",
                file,
                "--reviewer",
                "a",
                "--reviewer",
                "b",
            ]),
            "usage: craft review-followup ",
        ),
        (
            args(&[
                "review-followup",
                "--observation",
                file,
                "--reviewer",
                "a",
                "--authorized",
            ]),
            "usage: craft review-followup ",
        ),
        (
            args(&[
                "review-followup",
                "--observation",
                file,
                "--reviewer",
                "a",
                "--now",
                "today",
            ]),
            "--now must be an ISO 8601 time",
        ),
        (
            args(&[
                "review-followup",
                "--observation",
                file,
                "--reviewer",
                "a",
                "--stale-days",
                "-1",
            ]),
            "--stale-days must be",
        ),
        (
            args(&[
                "review-followup",
                "--observation",
                missing.to_str().unwrap(),
                "--reviewer",
                "a",
            ]),
            "cannot read observation",
        ),
        (
            args(&[
                "review-followup",
                "--observation",
                invalid.to_str().unwrap(),
                "--reviewer",
                "a",
            ]),
            "invalid observation JSON",
        ),
    ] {
        let outcome = cli::run(&argv);
        assert_eq!(outcome.exit_code, 2, "{argv:?}");
        assert!(outcome.stdout.is_empty(), "{argv:?}");
        assert!(
            outcome.stderr.starts_with(expected),
            "{argv:?}: {}",
            outcome.stderr
        );
    }
}
