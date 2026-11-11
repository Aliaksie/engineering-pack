//! `craft check` runs the checks a project declares for the changed files, at the stage it
//! is asked for: a check of a later stage is left to it, a check that did not run or did not
//! pass is never a pass, and the result binds to the tree the commands ran on.
mod support;
use serde_json::{Value, json};
use support::Repo;

/// Git stands in for the project's own tools: it is the one program every test host has.
const PROJECT: &str = r#"{"formatVersion":1,
  "modules":[
    {"name":"api","path":"api","checks":[
      {"command":"git","args":["--version"],"id":"api-build"},
      {"command":"git","args":["--version"],"id":"api-format","stage":"step"},
      {"command":"git","args":["--version"],"id":"api-slow","stage":"ci"}]},
    {"name":"ui","path":"ui","checks":[{"command":"git","args":["no-such-subcommand"],"id":"ui-broken"}]}],
  "standards":[
    {"id":"docs-links","run":{"command":"git","args":["--version"],"stage":"pr"},"scope":{"paths":["docs/"]}},
    {"id":"missing-tool","run":{"command":"craft-no-such-program"},"scope":{"paths":["tools/"]}}]}"#;

fn repo() -> Repo {
    Repo::new(
        "checks",
        &[
            (".craft/project.json", PROJECT),
            ("api/a.txt", "a\n"),
            ("ui/a.txt", "a\n"),
            ("docs/a.md", "a\n"),
            ("tools/a.txt", "a\n"),
        ],
    )
}

/// The exit code and JSON result of `craft check` with `args`.
fn check(repo: &Repo, args: &[&str]) -> (i32, Value) {
    let mut argv: Vec<String> = ["check", "--root", repo.path(), "--json"]
        .iter()
        .map(|arg| arg.to_string())
        .collect();
    argv.extend(args.iter().map(|arg| arg.to_string()));
    let outcome = craft_application::cli::run_structured(&argv);
    let value = serde_json::from_str(&outcome.stdout)
        .unwrap_or_else(|_| panic!("exit {}: {}", outcome.exit_code, outcome.stderr));
    (outcome.exit_code, value)
}

fn results(report: &Value, field: &str) -> Vec<(String, String)> {
    report[field]
        .as_array()
        .unwrap()
        .iter()
        .map(|entry| {
            (
                entry["id"].as_str().unwrap().to_string(),
                entry["result"].as_str().unwrap().to_string(),
            )
        })
        .collect()
}

fn pairs(expected: &[(&str, &str)]) -> Vec<(String, String)> {
    expected
        .iter()
        .map(|(id, result)| (id.to_string(), result.to_string()))
        .collect()
}

#[test]
fn the_changed_modules_checks_run_at_the_stage_and_later_ones_are_left() {
    let repo = repo();
    repo.write("api/a.txt", "changed\n");
    let (exit, report) = check(&repo, &[]);
    assert_eq!(exit, 0, "{report}");
    assert_eq!(report["stage"], "commit");
    assert_eq!(report["result"], "passed");
    assert_eq!(
        results(&report, "checks"),
        pairs(&[("api-build", "passed"), ("api-format", "passed")])
    );
    assert_eq!(
        results(&report, "left"),
        pairs(&[("api-slow", "left-to-ci")])
    );
    assert!(report["source"].is_string(), "bound to the tree: {report}");
    assert_eq!(report["treeChanged"], false);
    assert_eq!(report["recorded"], false, "no session, nothing kept");

    let (_, step) = check(&repo, &["--stage", "step"]);
    assert_eq!(results(&step, "checks"), pairs(&[("api-format", "passed")]));
    let (_, ci) = check(&repo, &["--stage", "ci"]);
    assert_eq!(ci["summary"]["run"], 3);
    assert!(results(&ci, "left").is_empty());
}

#[test]
fn a_failing_or_unavailable_command_fails_the_run_and_shows_why() {
    let repo = repo();
    let (exit, report) = check(&repo, &["--file", "ui/a.txt", "--file", "tools/a.txt"]);
    assert_eq!(exit, 1);
    assert_eq!(report["result"], "failed");
    assert_eq!(
        results(&report, "checks"),
        pairs(&[("ui-broken", "failed"), ("missing-tool", "not-started")])
    );
    let broken = &report["checks"][0];
    assert_eq!(broken["reason"], "exit-code");
    assert!(
        broken["output"]
            .as_str()
            .unwrap()
            .contains("no-such-subcommand"),
        "{broken}"
    );
    assert!(
        report["checks"][1]["error"]
            .as_str()
            .unwrap()
            .contains("craft-no-such-program")
    );
}

#[test]
fn an_unchanged_tree_runs_nothing_unless_every_check_is_asked_for() {
    let repo = repo();
    let (exit, report) = check(&repo, &[]);
    assert_eq!(exit, 0);
    assert_eq!(report["result"], "nothing-run");
    assert!(report["note"].as_str().unwrap().contains("--all"));

    let (exit, all) = check(&repo, &["--all", "--stage", "pr"]);
    assert_eq!(exit, 1, "the broken and the unavailable check are run");
    assert_eq!(all["summary"]["run"], 5);
    assert_eq!(results(&all, "left"), pairs(&[("api-slow", "left-to-ci")]));
}

#[test]
fn a_check_left_to_a_later_stage_alone_is_not_a_pass() {
    let repo = repo();
    let (exit, report) = check(&repo, &["--file", "docs/a.md"]);
    assert_eq!(exit, 0);
    assert_eq!(report["result"], "nothing-run");
    assert_eq!(
        results(&report, "left"),
        pairs(&[("docs-links", "left-to-pr")])
    );
}

#[test]
fn a_session_keeps_the_run_and_a_switched_off_project_runs_nothing() {
    let repo = repo();
    repo.write("api/a.txt", "changed\n");
    let (_, report) = check(&repo, &["--session", "S1"]);
    assert_eq!(report["recorded"], true);
    let file =
        craft_platform::state::file(&repo.root, "check-runs.json", false, Some("S1")).unwrap();
    let history: Value = serde_json::from_str(&std::fs::read_to_string(file).unwrap()).unwrap();
    assert_eq!(history["runs"][0]["source"], report["source"]);
    assert_eq!(history["runs"][0]["checks"][0]["id"], "api-build");

    repo.write(
        ".craft/project.json",
        &PROJECT.replacen(
            r#""formatVersion":1,"#,
            r#""formatVersion":1,"routing":{"projectChecks":false},"#,
            1,
        ),
    );
    let (exit, off) = check(&repo, &["--all"]);
    assert_eq!(exit, 0);
    assert_eq!(off["result"], "nothing-run");
    assert!(off["note"].as_str().unwrap().contains("projectChecks"));
}

#[test]
fn a_command_that_changes_the_tree_is_reported_and_bad_requests_are_refused() {
    let repo = Repo::new(
        "checks-tree",
        &[
            (
                ".craft/project.json",
                r#"{"formatVersion":1,"modules":[{"name":"api","path":"api","checks":[
                    {"command":"git","args":["rm","-q","a.txt"]}]}]}"#,
            ),
            ("api/a.txt", "a\n"),
        ],
    );
    let (exit, report) = check(&repo, &["--file", "api/a.txt"]);
    assert_eq!(exit, 0, "{report}");
    assert_eq!(report["checks"][0]["id"], "api-1");
    assert_eq!(report["treeChanged"], true);

    assert!(repo.cli(&["check", "--stage", "nightly"]).is_err());
    assert!(
        repo.cli(&["check", "--all", "--file", "api/a.txt"])
            .is_err()
    );
    let bad = json!({"formatVersion":1,"modules":[{"name":"api","path":"api","checks":[
        {"command":"git","stage":"nightly"}]}]});
    repo.write(".craft/project.json", &bad.to_string());
    assert!(repo.cli(&["check", "--all"]).is_err());
}

#[cfg(unix)]
#[test]
fn a_command_past_its_time_bound_is_ended_and_not_passed() {
    let repo = Repo::new(
        "checks-slow",
        &[
            (
                ".craft/project.json",
                r#"{"formatVersion":1,"modules":[{"name":"api","path":"api","checks":[
                    {"command":"sleep","args":["30"],"timeoutSeconds":1}]}]}"#,
            ),
            ("api/a.txt", "a\n"),
        ],
    );
    let (exit, report) = check(&repo, &["--all"]);
    assert_eq!(exit, 1);
    assert_eq!(report["checks"][0]["result"], "timed-out");
}

#[test]
fn a_history_that_cannot_be_kept_does_not_discard_the_report() {
    let repo = repo();
    repo.write("api/a.txt", "changed\n");
    // A directory where the history file belongs: reading and writing it fail.
    let history =
        craft_platform::state::file(&repo.root, "check-runs.json", true, Some("S4")).unwrap();
    std::fs::create_dir_all(&history).unwrap();
    let (exit, report) = check(&repo, &["--session", "S4"]);
    assert_eq!(exit, 0, "{report}");
    assert_eq!(report["recorded"], false);
    assert_eq!(report["result"], "passed");
    assert!(
        report["recordErrors"]
            .as_array()
            .is_some_and(|e| !e.is_empty())
    );
}

#[cfg(unix)]
#[test]
fn an_observed_run_binds_to_the_tree_the_command_started_on() {
    let repo = Repo::new(
        "checks-observed",
        &[
            (
                ".craft/project.json",
                r#"{"formatVersion":1,"modules":[{"name":"api","path":"api","checks":[
                    {"command":"sh","args":["-c","git rm -q a.txt; pytest"]}]}]}"#,
            ),
            ("api/a.txt", "a\n"),
        ],
    );
    let (_, report) = check(&repo, &["--file", "api/a.txt", "--session", "S3"]);
    assert_eq!(report["treeChanged"], true, "{report}");
    let file =
        craft_platform::state::file(&repo.root, "test-runs.json", false, Some("S3")).unwrap();
    let runs: Value = serde_json::from_str(&std::fs::read_to_string(file).unwrap()).unwrap();
    assert_ne!(report["source"], Value::Null);
    assert_eq!(runs["runs"][0]["source"], report["source"]);
}
