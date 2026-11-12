//! The project's safety policy at the host boundary: a destructive shell command is refused
//! in each host's own format before it runs, other commands pass with no permission
//! decision. The policy is on until a project or a baseline layer turns it off.
mod support;
use serde_json::{Value, json};
use support::Repo;

const ON: &str = r#"{"formatVersion":1,"safety":{"enabled":true,
  "deny":[{"id":"infra-destroy","command":["terraform","destroy"],"reason":"tearing down shared infrastructure"}]}}"#;

/// What the host is told: `deny <reason>` for a refusal (Claude Code's blocking exit with
/// the reason, or a deny in another host's JSON), else the hook's output.
fn shell(host: &str, cwd: &str, command: &str) -> String {
    let args: Vec<String> = [
        host,
        "before-tool",
        "--package",
        "craft-delivery",
        "--handler",
        "deliver",
    ]
    .iter()
    .map(|arg| arg.to_string())
    .collect();
    let payload = json!({"session_id": "S1", "cwd": cwd, "hook_event_name": "PreToolUse",
        "tool_name": "Bash", "tool_input": {"command": command}});
    let (wire, _) = craft_application::hook::run(&args, &mut payload.to_string().as_bytes());
    match wire.exit_code {
        2 => format!("deny {}", wire.stderr),
        0 if wire.stdout.contains(r#""deny""#) => format!("deny {}", wire.stdout),
        0 => wire.stdout,
        code => panic!("exit {code}: {}", wire.stderr),
    }
}

#[test]
fn claude_code_and_codex_refuse_a_destructive_command_with_the_rule_named() {
    let repo = Repo::new(
        "safety",
        &[(".craft/project.json", ON), ("src/a.txt", "a\n")],
    );
    let claude = shell("claude", repo.path(), "git push --force origin main");
    assert!(claude.starts_with("deny "), "{claude}");
    assert!(claude.contains("rule `force-push`"), "{claude}");
    let codex = shell(
        "codex",
        repo.path(),
        "cd infra && terraform destroy -auto-approve",
    );
    assert!(codex.starts_with("deny "), "{codex}");
    assert!(
        codex.contains("rule `infra-destroy`: tearing down shared infrastructure"),
        "{codex}"
    );
}

#[test]
fn a_delete_is_judged_from_the_directory_the_command_runs_in() {
    let repo = Repo::new(
        "safety-cwd",
        &[(".craft/project.json", ON), ("api/a.txt", "a\n")],
    );
    let nested = format!("{}/api", repo.path());
    assert!(shell("claude", &nested, "rm -rf ..").contains("delete-project-root"));
    assert!(shell("claude", &nested, "rm -rf ../..").contains("delete-outside-project"));
    assert!(!shell("claude", &nested, "rm -rf build").starts_with("deny"));
}

#[test]
fn an_allowed_command_gets_no_permission_decision() {
    let repo = Repo::new("safety-allow", &[(".craft/project.json", ON)]);
    for host in ["claude", "codex"] {
        for command in ["git status", "rm -rf build", "git push origin main"] {
            let output = shell(host, repo.path(), command);
            assert!(
                !output.contains("permissionDecision"),
                "{host} {command}: {output}"
            );
        }
    }
    // A tool that is not the shell is never judged, whatever its input says.
    let args: Vec<String> = [
        "claude",
        "before-tool",
        "--package",
        "craft-delivery",
        "--handler",
        "deliver",
    ]
    .iter()
    .map(|arg| arg.to_string())
    .collect();
    let payload = json!({"session_id": "S1", "cwd": repo.path(), "hook_event_name": "PreToolUse",
        "tool_name": "Write", "tool_input": {"file_path": "notes.md", "content": "x", "command": "git push --force"}});
    let (wire, _) = craft_application::hook::run(&args, &mut payload.to_string().as_bytes());
    assert_eq!(wire.exit_code, 0, "{}", wire.stderr);
    assert!(!wire.stdout.contains("deny"), "{}", wire.stdout);
}

#[test]
fn a_project_that_declares_nothing_is_protected_and_one_that_turns_the_policy_off_is_not() {
    let command = "sudo rm -rf / && git push --force";
    let bare = Repo::new("safety-default", &[("README.md", "x\n")]);
    let refused = shell("claude", bare.path(), command);
    assert!(refused.starts_with("deny "), "{refused}");
    assert!(
        refused.contains("rule `system-administration`"),
        "{refused}"
    );
    let harmless = shell("claude", bare.path(), "git status && rm -rf target");
    assert!(!harmless.starts_with("deny"), "{harmless}");

    let off = r#"{"formatVersion":1,"safety":{"enabled":false}}"#;
    let repo = Repo::new("safety-off", &[(".craft/project.json", off)]);
    let output = shell("claude", repo.path(), command);
    assert!(!output.starts_with("deny"), "{output}");
}

#[test]
fn craft_settings_lists_the_rules_in_force() {
    let repo = Repo::new("safety-settings", &[(".craft/project.json", ON)]);
    let argv: Vec<String> = ["settings", "--root", repo.path()]
        .iter()
        .map(|a| a.to_string())
        .collect();
    let outcome = craft_application::cli::run_structured(&argv);
    assert_eq!(outcome.exit_code, 0, "{}", outcome.stderr);
    let report: Value = serde_json::from_str(&outcome.stdout).expect("report");
    assert_eq!(report["safety"]["enabled"], true);
    assert_eq!(report["safety"]["origin"], "project");
    let ids: Vec<&str> = report["safety"]["rules"]
        .as_array()
        .expect("rules")
        .iter()
        .filter_map(|rule| rule["id"].as_str())
        .collect();
    assert_eq!(ids.len(), 6);
    assert_eq!(ids.last(), Some(&"infra-destroy"));
}

#[test]
fn the_session_summary_counts_refused_commands() {
    let repo = Repo::new("safety-summary", &[(".craft/project.json", ON)]);
    assert!(shell("claude", repo.path(), "git reset --hard").starts_with("deny"));
    assert!(shell("claude", repo.path(), "sudo reboot").starts_with("deny"));
    let args: Vec<String> = [
        "claude",
        "turn-end",
        "--package",
        "craft-delivery",
        "--handler",
        "deliver",
    ]
    .iter()
    .map(|arg| arg.to_string())
    .collect();
    let payload = json!({"session_id": "S1", "cwd": repo.path(), "hook_event_name": "Stop"});
    let (wire, _) = craft_application::hook::run(&args, &mut payload.to_string().as_bytes());
    assert_eq!(
        serde_json::from_str::<Value>(&wire.stdout).expect("output")["systemMessage"],
        "Craft this session: 2 commands refused by the safety policy"
    );
}
