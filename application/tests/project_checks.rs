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
    check_at(repo.path(), args)
}

fn check_at(root: &str, args: &[&str]) -> (i32, Value) {
    let mut argv: Vec<String> = ["check", "--root", root, "--json"]
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

/// Git in `directory`, for a checkout the fixture builds by hand.
fn git_in(directory: &std::path::Path, args: &[&str]) {
    let output = std::process::Command::new("git")
        .arg("-C")
        .arg(directory)
        .args([
            "-c",
            "core.hooksPath=/dev/null",
            "-c",
            "commit.gpgsign=false",
        ])
        .args([
            "-c",
            "user.name=Fixture",
            "-c",
            "user.email=fixture@example.test",
        ])
        .args(args)
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
}

/// Ids of the checks a report ran.
fn ran(report: &Value) -> Vec<String> {
    results(report, "checks")
        .into_iter()
        .map(|(id, _)| id)
        .collect()
}

#[test]
fn a_project_below_the_top_of_its_checkout_reads_its_own_changes() {
    let repo = Repo::new(
        "checks-below",
        &[
            ("web/.craft/project.json", PROJECT),
            ("web/api/a.txt", "a\n"),
            ("web/ui/a.txt", "a\n"),
            ("api/a.txt", "outside the project\n"),
        ],
    );
    let web = repo.root.join("web");
    let web = web.to_str().unwrap();
    // A change outside the project's folder is no change of the project, though its path
    // from the top would name the project's `api` module.
    repo.write("api/a.txt", "changed\n");
    let (_, outside) = check_at(web, &[]);
    assert_eq!(outside["result"], "nothing-run", "{outside}");

    repo.write("web/api/a.txt", "changed\n");
    let (exit, report) = check_at(web, &[]);
    assert_eq!(exit, 0, "{report}");
    assert_eq!(ran(&report), ["api-build", "api-format"]);
}

#[test]
fn a_workspace_root_reads_the_changes_of_each_member_repository() {
    let workspace = Repo::bare("checks-workspace");
    workspace.write(
        ".craft/project.json",
        r#"{"formatVersion":1,"modules":[
          {"name":"api","path":"api","checks":[{"command":"git","args":["--version"],"id":"api-build"}]},
          {"name":"web","path":"web","checks":[{"command":"git","args":["--version"],"id":"web-build"}]}]}"#,
    );
    for member in ["api", "web"] {
        workspace.write(&format!("{member}/a.txt"), "a\n");
        let dir = workspace.root.join(member);
        git_in(&dir, &["init", "-q"]);
        git_in(&dir, &["add", "."]);
        git_in(&dir, &["commit", "-qm", "fixture"]);
    }
    let (_, clean) = check_at(workspace.path(), &[]);
    assert_eq!(clean["result"], "nothing-run", "{clean}");

    workspace.write("web/a.txt", "changed\n");
    let (exit, report) = check_at(workspace.path(), &[]);
    assert_eq!(exit, 0, "{report}");
    assert_eq!(ran(&report), ["web-build"]);

    let plain = Repo::bare("checks-plain");
    plain.write(".craft/project.json", PROJECT);
    let (_, none) = check_at(plain.path(), &[]);
    assert!(
        none["note"].as_str().unwrap().contains("no Git checkout"),
        "{none}"
    );
}

#[test]
fn a_base_adds_the_files_committed_since_it() {
    let repo = repo();
    repo.git(&["branch", "start"]);
    repo.write("docs/a.md", "committed\n");
    repo.git(&["commit", "-qam", "docs"]);
    let (_, clean) = check(&repo, &["--stage", "pr"]);
    assert_eq!(
        clean["result"], "nothing-run",
        "a clean tree alone: {clean}"
    );

    repo.write("api/a.txt", "uncommitted\n");
    let (exit, report) = check(&repo, &["--stage", "pr", "--base", "start"]);
    assert_eq!(exit, 0, "{report}");
    assert_eq!(ran(&report), ["api-build", "api-format", "docs-links"]);

    let unknown = repo.cli(&["check", "--base", "no-such-ref"]).unwrap_err();
    assert!(unknown.contains("no-such-ref"), "{unknown}");
    assert!(repo.cli(&["check", "--base", "start", "--all"]).is_err());
    assert!(
        repo.cli(&["check", "--base", "start", "--file", "api/a.txt"])
            .is_err()
    );
    assert!(repo.cli(&["standards", "--base", "start"]).is_err());
}

/// `git diff --exit-code` fails only for a file it is given that changed, and prints its path
/// from the top: a check that fails names the files the argument passed, from its directory.
#[test]
fn a_changed_files_argument_passes_the_checks_own_files_and_skips_a_check_without_any() {
    let repo = Repo::new(
        "checks-files",
        &[
            (
                ".craft/project.json",
                r#"{"formatVersion":1,"modules":[
                  {"name":"api","path":"api","checks":[{"command":"git","args":["diff","--exit-code","--","{changedFiles}"],"id":"api-diff"}]},
                  {"name":"ui","path":"ui","checks":[{"command":"git","args":["diff","--exit-code","--","{changedFiles}"],"id":"ui-diff"}]}]}"#,
            ),
            ("api/a.txt", "a\n"),
            ("api/b.txt", "b\n"),
            ("ui/a.txt", "a\n"),
        ],
    );
    repo.write("api/a.txt", "changed\n");
    std::fs::remove_file(repo.root.join("api/b.txt")).unwrap();
    let (exit, report) = check(&repo, &[]);
    assert_eq!(exit, 1, "{report}");
    // The deleted file is a change of the module, but no file to pass.
    let diff = &report["checks"][0];
    assert_eq!(
        (diff["id"].as_str(), diff["files"].as_u64()),
        (Some("api-diff"), Some(1))
    );
    assert!(
        diff["output"].as_str().unwrap().contains("api/a.txt"),
        "{diff}"
    );

    let (exit, all) = check(&repo, &["--all"]);
    assert_eq!(exit, 0, "no changed file is named, so nothing ran: {all}");
    assert_eq!(all["result"], "nothing-run");
    assert_eq!(all["summary"]["skipped"], 2);
    assert_eq!(all["skipped"][0]["reason"], "no-changed-file");
}

#[test]
fn the_session_shows_its_last_check_run_and_whether_the_tree_moved_since() {
    let repo = repo();
    let bundle = support::bundle();
    let session = |repo: &Repo| {
        repo.cli(&["session", "--session", "S5", "--bundle", &bundle])
            .unwrap()
    };
    assert!(session(&repo)["lastCheck"].is_null(), "no run, no line");

    repo.write("api/a.txt", "changed\n");
    let (_, report) = check(&repo, &["--session", "S5"]);
    assert_eq!(report["recorded"], true);
    let last = &session(&repo)["lastCheck"];
    assert_eq!(
        (
            &last["result"],
            &last["stage"],
            &last["run"],
            &last["current"]
        ),
        (&json!("passed"), &json!("commit"), &json!(2), &json!(true)),
        "{last}"
    );
    repo.write("api/a.txt", "changed again\n");
    assert_eq!(session(&repo)["lastCheck"]["current"], false);
}
