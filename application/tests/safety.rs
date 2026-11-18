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
    hook(host, "craft-delivery", "deliver", cwd, command)
}

/// `shell` through the hook one package registers before a tool.
fn hook(host: &str, package: &str, handler: &str, cwd: &str, command: &str) -> String {
    let args: Vec<String> = [
        host,
        "before-tool",
        "--package",
        package,
        "--handler",
        handler,
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
    assert_eq!(ids.len(), 8);
    assert_eq!(ids.last(), Some(&"infra-destroy"));
}

#[test]
fn the_session_summary_counts_refused_commands() {
    let repo = Repo::new("safety-summary", &[(".craft/project.json", ON)]);
    assert!(shell("claude", repo.path(), "git reset --hard").starts_with("deny"));
    assert!(shell("claude", repo.path(), "sudo reboot").starts_with("deny"));
    // A secret read Claude Code hands to the user is not counted as refused.
    assert!(shell("claude", repo.path(), "cat .env").contains(r#""ask""#));
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

#[test]
fn every_package_refuses_what_the_policy_refuses() {
    let repo = Repo::new("safety-observe", &[("README.md", "x\n")]);
    for host in ["claude", "codex"] {
        for package in ["craft-engineering", "craft-review", "craft-stories"] {
            let refused = hook(host, package, "observe", repo.path(), "git push --force");
            assert!(refused.starts_with("deny "), "{host} {package}: {refused}");
            assert!(refused.contains("rule `force-push`"), "{refused}");
            let allowed = hook(
                host,
                package,
                "observe",
                repo.path(),
                "git push origin main",
            );
            assert!(allowed.is_empty(), "{host} {package}: {allowed}");
        }
    }
    let off = r#"{"formatVersion":1,"safety":{"enabled":false}}"#;
    let repo = Repo::new("safety-observe-off", &[(".craft/project.json", off)]);
    let output = hook(
        "claude",
        "craft-engineering",
        "observe",
        repo.path(),
        "sudo reboot",
    );
    assert!(output.is_empty(), "{output}");
}

#[test]
fn a_delete_below_the_temporary_directory_is_allowed_and_the_directory_itself_is_not() {
    let repo = Repo::new("safety-temp", &[("README.md", "x\n")]);
    let temp = std::env::temp_dir();
    let temp = temp.to_string_lossy();
    let temp = temp.trim_end_matches(['/', '\\']);
    let below = format!("rm -rf {temp}/craft-safety-scratch-{}", std::process::id());
    assert!(!shell("claude", repo.path(), &below).starts_with("deny"));
    let whole = shell("claude", repo.path(), &format!("rm -rf {temp}"));
    assert!(whole.contains("delete-outside-project"), "{whole}");
}

#[test]
fn a_delete_of_a_scratch_directory_passes_written_out_and_is_refused_through_a_variable() {
    let repo = Repo::new("safety-scratch", &[("README.md", "x\n")]);
    let temp = std::env::temp_dir();
    let temp = temp.to_string_lossy();
    let scratch = format!(
        "{}/craft-safety-session-{}/scratchpad",
        temp.trim_end_matches(['/', '\\']),
        std::process::id()
    );
    for host in ["claude", "codex"] {
        let written = shell(host, repo.path(), &format!("rm -rf {scratch}/clone"));
        assert!(!written.starts_with("deny"), "{host}: {written}");
        let variable = shell(
            host,
            repo.path(),
            &format!("SCRATCH={scratch}; rm -rf \"$SCRATCH/clone\""),
        );
        assert!(variable.starts_with("deny "), "{host}: {variable}");
        assert!(
            variable.contains("rule `delete-outside-project`"),
            "{variable}"
        );
        assert!(variable.contains("$SCRATCH/clone"), "{variable}");
        assert!(variable.contains("does not expand"), "{variable}");
    }
}

#[cfg(unix)]
#[test]
fn a_project_reached_through_a_link_is_the_same_project() {
    let repo = Repo::new("safety-link", &[("README.md", "x\n")]);
    let parent = Repo::bare("safety-link-parent");
    let link = format!("{}/shop", parent.path());
    std::os::unix::fs::symlink(repo.path(), &link).unwrap();
    for target in [format!("{}/build", repo.path()), format!("{link}/build")] {
        let output = shell("claude", &link, &format!("rm -rf {target}"));
        assert!(!output.starts_with("deny"), "{target}: {output}");
    }
    let root = shell("claude", &link, &format!("rm -rf {}", repo.path()));
    assert!(root.contains("delete-project-root"), "{root}");
}

/// What the host is told for one tool call: `ask <reason>` where the call waits for the
/// user, `deny <reason>` for a refusal, else the hook's output.
fn call(host: &str, package: &str, cwd: &str, tool: &str, input: Value) -> String {
    let handler = if package == "craft-delivery" {
        "deliver"
    } else {
        "observe"
    };
    let args: Vec<String> = [
        host,
        "before-tool",
        "--package",
        package,
        "--handler",
        handler,
    ]
    .iter()
    .map(|arg| arg.to_string())
    .collect();
    let payload = json!({"session_id": "S1", "cwd": cwd, "hook_event_name": "PreToolUse",
        "tool_name": tool, "tool_input": input});
    let (wire, _) = craft_application::hook::run(&args, &mut payload.to_string().as_bytes());
    match wire.exit_code {
        2 => format!("deny {}", wire.stderr),
        0 if wire.stdout.contains(r#""ask""#) => format!("ask {}", wire.stdout),
        0 if wire.stdout.contains(r#""deny""#) => format!("deny {}", wire.stdout),
        0 => wire.stdout,
        code => panic!("exit {code}: {}", wire.stderr),
    }
}

const SECRET_REPO: [(&str, &str); 4] = [
    ("README.md", "x\n"),
    (".env", "TOKEN=real\n"),
    (".env.example", "TOKEN=\n"),
    ("api/.env.local", "KEY=real\n"),
];

#[test]
fn claude_code_hands_a_secret_file_read_to_the_user_through_every_package() {
    let repo = Repo::new("secret-claude", &SECRET_REPO);
    for package in [
        "craft-delivery",
        "craft-engineering",
        "craft-review",
        "craft-stories",
    ] {
        let read = call(
            "claude",
            package,
            repo.path(),
            "Read",
            json!({"file_path": format!("{}/.env", repo.path())}),
        );
        assert!(read.starts_with("ask "), "{package}: {read}");
        assert!(read.contains("read-secret-file"), "{read}");
        let template = call(
            "claude",
            package,
            repo.path(),
            "Read",
            json!({"file_path": ".env.example"}),
        );
        assert!(
            template.is_empty() || !template.contains("permissionDecision"),
            "{package}: {template}"
        );
    }
    let search = call(
        "claude",
        "craft-engineering",
        repo.path(),
        "Grep",
        json!({"pattern": "KEY", "glob": ".env*"}),
    );
    assert!(search.starts_with("ask "), "{search}");
    let searched = call(
        "claude",
        "craft-engineering",
        repo.path(),
        "Grep",
        json!({"pattern": "KEY", "path": "api/.env.local"}),
    );
    assert!(searched.starts_with("ask "), "{searched}");
    // Listing names shows no content; a broad search is outside what a name shows.
    for (tool, input) in [
        ("Glob", json!({"pattern": "**/.env*"})),
        ("Grep", json!({"pattern": "KEY", "glob": "*"})),
        ("Read", json!({"file_path": "README.md"})),
    ] {
        let output = call("claude", "craft-engineering", repo.path(), tool, input);
        assert!(output.is_empty(), "{tool}: {output}");
    }
    // A file outside the workspace is judged the same way.
    let home = call(
        "claude",
        "craft-engineering",
        repo.path(),
        "Read",
        json!({"file_path": "/home/dev/.aws/credentials"}),
    );
    assert!(home.starts_with("ask "), "{home}");
    let shell = call(
        "claude",
        "craft-delivery",
        repo.path(),
        "Bash",
        json!({"command": "cat api/.env.local"}),
    );
    assert!(shell.starts_with("ask "), "{shell}");
}

#[test]
fn hosts_that_cannot_ask_refuse_a_secret_file_read() {
    let repo = Repo::new("secret-hosts", &SECRET_REPO);
    for (host, tool, input) in [
        ("codex", "Bash", json!({"command": "grep TOKEN .env"})),
        (
            "codex",
            "apply_patch",
            json!({"command": "*** Begin Patch\n*** Update File: .env\n@@\n-a\n+b\n*** End Patch"}),
        ),
        (
            "gemini",
            "read_many_files",
            json!({"paths": ["README.md", "api/.env.local"]}),
        ),
        (
            "gemini",
            "read_file",
            json!({"absolute_path": format!("{}/.env", repo.path())}),
        ),
        ("copilot", "view", json!({"path": ".env"})),
        ("cursor", "Read", json!({"file_path": ".env"})),
    ] {
        let output = call(host, "craft-engineering", repo.path(), tool, input);
        assert!(output.starts_with("deny "), "{host} {tool}: {output}");
        assert!(output.contains("read-secret-file"), "{output}");
    }
}

#[test]
fn writing_a_new_secret_file_is_not_a_read_and_the_rule_can_be_turned_off() {
    let repo = Repo::new("secret-write", &SECRET_REPO);
    let new = call(
        "claude",
        "craft-delivery",
        repo.path(),
        "Write",
        json!({"file_path": "web/.env", "content": "A=1\n"}),
    );
    assert!(!new.contains("permissionDecision"), "{new}");
    let existing = call(
        "claude",
        "craft-delivery",
        repo.path(),
        "Edit",
        json!({"file_path": ".env", "old_string": "a", "new_string": "b"}),
    );
    assert!(existing.starts_with("ask "), "{existing}");
    let off = r#"{"formatVersion":1,"safety":{"disables":["read-secret-file"]}}"#;
    let repo = Repo::new(
        "secret-off",
        &[(".craft/project.json", off), (".env", "T=1\n")],
    );
    for package in ["craft-delivery", "craft-engineering"] {
        let read = call(
            "claude",
            package,
            repo.path(),
            "Read",
            json!({"file_path": ".env"}),
        );
        assert!(!read.contains("permissionDecision"), "{package}: {read}");
        let shell = call(
            "codex",
            package,
            repo.path(),
            "Bash",
            json!({"command": "cat .env"}),
        );
        assert!(!shell.starts_with("deny"), "{package}: {shell}");
    }
}

/// A host that honours the hook: asks before the command and runs it only when the answer
/// is not a refusal. Returns whether the command was refused.
#[cfg(unix)]
fn host_runs(host: &str, repo: &Repo, command: &str) -> bool {
    let refused = shell(host, repo.path(), command).starts_with("deny ");
    if !refused {
        let status = std::process::Command::new("sh")
            .args(["-c", command])
            .current_dir(repo.path())
            .status()
            .unwrap();
        assert!(status.success(), "{command}");
    }
    refused
}

/// The attempt, the refusal and the effect are three facts. A command that discards local
/// changes and then puts the text back leaves a clean comparison behind, so only the file
/// itself tells whether it ran: Git writes a new file where it discards one. A second name
/// for the file, kept outside the project, still names the old one afterwards.
#[cfg(unix)]
#[test]
fn a_refused_command_has_no_effect_and_a_restored_file_does_not_hide_one() {
    use std::os::unix::fs::MetadataExt;
    let discard_and_restore =
        "cp src/a.txt kept.tmp && git reset --hard -q && cp kept.tmp src/a.txt && rm kept.tmp";
    let witness = |repo: &Repo| std::path::PathBuf::from(format!("{}.witness", repo.path()));
    // Whether the path still names the file the witness names, and its text.
    let observe = |repo: &Repo| {
        let file = std::path::Path::new(repo.path()).join("src/a.txt");
        let same = std::fs::metadata(&file).unwrap().ino()
            == std::fs::metadata(witness(repo)).unwrap().ino();
        (same, std::fs::read_to_string(&file).unwrap())
    };
    let edited = |name: &str, project: &str| {
        let repo = Repo::new(
            name,
            &[(".craft/project.json", project), ("src/a.txt", "a\n")],
        );
        repo.write("src/a.txt", "a\nlocal work\n");
        let _ = std::fs::remove_file(witness(&repo));
        std::fs::hard_link(format!("{}/src/a.txt", repo.path()), witness(&repo)).unwrap();
        repo
    };
    let local = (true, "a\nlocal work\n".to_string());
    for host in ["claude", "codex", "cursor", "copilot", "gemini"] {
        let repo = edited("safety-effect", ON);
        assert_eq!(observe(&repo), local);
        assert!(host_runs(host, &repo, discard_and_restore), "{host}");
        assert_eq!(observe(&repo), local, "{host}: the file was touched");
        std::fs::remove_file(witness(&repo)).unwrap();
    }
    // The same attempt where nothing refuses it: the text ends up as it was, the file does not.
    let off = r#"{"formatVersion":1,"safety":{"enabled":false}}"#;
    let repo = edited("safety-effect-off", off);
    assert!(!host_runs("claude", &repo, discard_and_restore));
    let (same, restored) = observe(&repo);
    assert_eq!(restored, local.1, "the restored text hides the discard");
    assert!(!same, "the discard replaced the file");
    std::fs::remove_file(witness(&repo)).unwrap();
}
