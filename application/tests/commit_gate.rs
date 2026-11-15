//! An agent's `git commit` waits for the declared commit-stage checks: the hook refuses it
//! while a check covering a changed file has not passed on the current files in a
//! `craft check` run of the session, and names that command. The hook runs no check itself.
mod support;
use serde_json::json;
use support::Repo;

/// `git diff --check` fails on trailing whitespace in a changed line: a check whose result
/// the test sets by what it writes, on every platform CI runs.
const PROJECT: &str = r#"{"formatVersion":1,
  "modules":[{"name":"api","path":"api","checks":[{"command":"git","args":["diff","--check"],"id":"api-whitespace"}]}]}"#;

/// The Claude hook's answer to a shell command: exit code and what it said.
fn before(repo: &Repo, command: &str) -> (i32, String) {
    let payload = json!({"session_id": "S1", "cwd": repo.path(), "tool_name": "Bash",
        "tool_input": {"command": command}});
    let args: Vec<String> = [
        "claude",
        "before-tool",
        "--package",
        "craft-delivery",
        "--handler",
        "deliver",
    ]
    .iter()
    .map(|a| a.to_string())
    .collect();
    let (wire, _) = craft_application::hook::run(&args, &mut payload.to_string().as_bytes());
    (wire.exit_code, format!("{}{}", wire.stdout, wire.stderr))
}

/// `craft check` for the session: its result (a failed check exits non-zero).
fn check(repo: &Repo) -> &'static str {
    match repo.cli(&["check", "--session", "S1", "--json"]) {
        Ok(report) => {
            assert_eq!(report["result"], "passed", "{report}");
            "passed"
        }
        Err(_) => "failed",
    }
}

#[test]
fn a_commit_waits_until_the_covering_check_passed_on_the_current_files() {
    let repo = Repo::new(
        "commit-gate",
        &[
            (".craft/project.json", PROJECT),
            ("api/a.txt", "a\n"),
            ("docs/a.md", "a\n"),
        ],
    );
    let commit = "git commit -am 'feat: x'";
    assert_eq!(before(&repo, commit).0, 0, "nothing changed");

    repo.write("docs/a.md", "b\n");
    assert_eq!(before(&repo, commit).0, 0, "no check covers docs");

    repo.write("api/a.txt", "b \n");
    let (code, said) = before(&repo, commit);
    assert_eq!(code, 2, "{said}");
    assert!(said.contains("api-whitespace"), "{said}");
    assert!(
        said.contains("craft check --stage commit --session S1"),
        "{said}"
    );
    // Other commands are not held.
    assert_eq!(before(&repo, "git status").0, 0);

    assert_eq!(check(&repo), "failed");
    let (code, said) = before(&repo, commit);
    assert_eq!(code, 2);
    assert!(said.contains("api-whitespace did not pass"), "{said}");

    // Fixed but not run again: the passed run must be on these files.
    repo.write("api/a.txt", "b\n");
    assert_eq!(before(&repo, commit).0, 2);
    assert_eq!(check(&repo), "passed");
    let (code, said) = before(&repo, commit);
    assert_eq!(code, 0, "{said}");

    // Changed again after the passing run.
    repo.write("api/a.txt", "c\n");
    assert_eq!(before(&repo, commit).0, 2);
}

#[test]
fn the_gate_is_off_with_its_switch_or_without_running_checks() {
    for routing in [r#"{"commitGate":false}"#, r#"{"projectChecks":false}"#] {
        let project = PROJECT.replacen(
            r#"{"formatVersion":1,"#,
            &format!(r#"{{"formatVersion":1,"routing":{routing},"#),
            1,
        );
        let repo = Repo::new(
            "commit-gate-off",
            &[(".craft/project.json", &project), ("api/a.txt", "a\n")],
        );
        repo.write("api/a.txt", "b \n");
        let (code, said) = before(&repo, "git commit -am 'feat: x'");
        assert_eq!(code, 0, "{routing}: {said}");
    }
}
