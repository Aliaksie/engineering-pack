//! Conventions a project already states: the scan reads the project's own files, writes
//! nothing, and what it finds is valid configuration. While the project declares no
//! standards those are in effect; standards it declares replace them.
mod support;
use craft_application::cli;
use serde_json::{Value, json};
use support::Repo;

const AGENTS: &str = "# Rules\n\nCommit messages follow Conventional Commits.\n";
const PACKAGE: &str = r#"{"scripts":{"lint":"eslint ."}}"#;

fn propose(repo: &Repo) -> Value {
    let argv: Vec<String> = ["standards", "--root", repo.path(), "--propose"]
        .iter()
        .map(|arg| arg.to_string())
        .collect();
    let outcome = cli::run_structured(&argv);
    assert_eq!(outcome.exit_code, 0, "{}", outcome.stderr);
    serde_json::from_str(&outcome.stdout).expect("JSON result")
}

fn standards(result: &Value) -> Vec<Value> {
    result["proposals"]
        .as_array()
        .expect("proposals")
        .iter()
        .map(|proposal| proposal["standard"].clone())
        .collect()
}

#[test]
fn an_unconfigured_project_gets_proposals_from_its_own_files_and_nothing_is_written() {
    let repo = Repo::new(
        "conventions",
        &[
            ("AGENTS.md", AGENTS),
            ("package.json", PACKAGE),
            ("eslint.config.js", "export default [];\n"),
            ("docs/CONTRIBUTING.md", "# Contributing\n"),
            (".editorconfig", "[*]\nindent_style = tab\n"),
        ],
    );
    let result = propose(&repo);
    let ids: Vec<Value> = standards(&result)
        .iter()
        .map(|standard| standard["id"].clone())
        .collect();
    assert_eq!(
        ids,
        json!(["commits", "lint", "contributing", "editorconfig"])
            .as_array()
            .unwrap()
            .clone()
    );
    assert_eq!(
        result["scanned"],
        json!([
            "AGENTS.md",
            "docs/CONTRIBUTING.md",
            ".editorconfig",
            "package.json",
            "eslint.config.js"
        ])
    );
    assert!(
        !repo.root.join(".craft/project.json").exists(),
        "the scan writes no configuration"
    );

    // Accepting every proposal gives a configuration the project loads, with nothing left to propose.
    repo.write(
        ".craft/project.json",
        &json!({"formatVersion": 1, "standards": standards(&result)}).to_string(),
    );
    let again = propose(&repo);
    assert_eq!(again["proposals"], json!([]));
    assert_eq!(again["declared"].as_array().map(Vec::len), Some(4));
}

#[test]
fn a_linked_or_oversized_file_is_not_read() {
    let repo = Repo::new("conventions-bounds", &[("README.md", "x\n")]);
    repo.write("CONTRIBUTING.md", &"Conventional Commits\n".repeat(20_000));
    #[cfg(unix)]
    std::os::unix::fs::symlink("/etc/hosts", repo.root.join("AGENTS.md")).expect("link");
    let result = propose(&repo);
    // The oversized contributor document is present, so it is still a file standard; its
    // text was not read, so it names no convention.
    let ids: Vec<Value> = standards(&result).iter().map(|s| s["id"].clone()).collect();
    assert_eq!(ids, vec![json!("contributing")]);
    assert_eq!(result["scanned"], json!(["CONTRIBUTING.md"]));
    assert_eq!(result["unread"][0]["file"], "CONTRIBUTING.md");
}

#[test]
fn propose_takes_no_files_and_belongs_to_standards() {
    let run = |args: &[&str]| cli::run(&args.iter().map(|a| a.to_string()).collect::<Vec<_>>());
    let mixed = run(&["standards", "--propose", "--file", "CHANGELOG.md"]);
    assert_ne!(mixed.exit_code, 0);
    assert!(
        mixed.stderr.contains("--propose takes no --file"),
        "{}",
        mixed.stderr
    );
    let other = run(&["anchors", "--propose"]);
    assert_ne!(other.exit_code, 0);
    assert!(
        other.stderr.contains("--propose is supported by standards"),
        "{}",
        other.stderr
    );
}

fn start(host: &str, repo: &Repo, session: &str) -> String {
    let args: Vec<String> = [
        host,
        "session-start",
        "--package",
        "craft-delivery",
        "--handler",
        "deliver",
    ]
    .iter()
    .map(|arg| arg.to_string())
    .collect();
    let payload = json!({"session_id": session, "cwd": repo.path(), "source": "startup",
        "hook_event_name": "SessionStart"});
    let (wire, _) = craft_application::hook::run(&args, &mut payload.to_string().as_bytes());
    assert_eq!(wire.exit_code, 0, "{}", wire.stderr);
    wire.stdout
}

#[test]
fn a_session_start_names_undeclared_conventions_once_and_again_only_when_they_change() {
    let repo = Repo::new(
        "conventions-offer",
        &[("AGENTS.md", AGENTS), ("rustfmt.toml", "max_width = 100\n")],
    );
    let first: Value = serde_json::from_str(&start("claude", &repo, "S1")).expect("output");
    let line = first["systemMessage"].as_str().expect("line");
    for part in [
        "2 standards",
        "in effect",
        "(commits, rustfmt)",
        "--propose",
    ] {
        assert!(line.contains(part), "{line}");
    }
    let context = first["hookSpecificOutput"]["additionalContext"]
        .as_str()
        .expect("context");
    assert!(context.contains("Craft conventions scan"), "{context}");
    assert!(
        context.contains(r#""standards":["commits","rustfmt"]"#),
        "{context}"
    );
    assert!(context.contains(r#""--propose""#), "{context}");

    // The same set is not named again, in this session or the next, on any host.
    for (host, session) in [("claude", "S1"), ("claude", "S2"), ("codex", "S3")] {
        let again = start(host, &repo, session);
        assert!(
            !again.contains("conventions scan") && !again.contains("own files"),
            "{again}"
        );
    }
    // A new convention file changes the set.
    repo.write(".flake8", "[flake8]\n");
    assert!(start("codex", &repo, "S4").contains("3 standards"));
}

#[test]
fn a_project_with_standards_or_the_switch_off_hears_nothing() {
    for config in [
        r#"{"formatVersion":1,"standards":[{"id":"api","file":"docs/api.md"}]}"#,
        r#"{"formatVersion":1,"routing":{"conventionProposals":false}}"#,
    ] {
        let repo = Repo::new(
            "conventions-quiet",
            &[("AGENTS.md", AGENTS), (".craft/project.json", config)],
        );
        let output = start("claude", &repo, "S1");
        assert!(!output.contains("own files"), "{output}");
        assert!(
            !repo.root.join(".craft/state/notices").exists(),
            "no scan, no record"
        );
    }
}

/// `(exit code, stdout, stderr)` of one `craft` command at the repository.
fn craft(repo: &Repo, args: &[&str]) -> (i32, String, String) {
    let mut argv: Vec<String> = vec![args[0].to_string(), "--root".into(), repo.path().into()];
    argv.extend(args[1..].iter().map(|arg| arg.to_string()));
    let outcome = cli::run_structured(&argv);
    (outcome.exit_code, outcome.stdout, outcome.stderr)
}

fn declared_ids(repo: &Repo) -> Vec<String> {
    let (code, stdout, stderr) = craft(repo, &["standards"]);
    assert_eq!(code, 0, "{stderr}");
    let listing: Value = serde_json::from_str(&stdout).expect("listing");
    listing["standards"]
        .as_array()
        .expect("standards")
        .iter()
        .map(|standard| standard["id"].as_str().expect("id").to_string())
        .collect()
}

#[test]
fn a_project_that_declares_no_standards_is_held_to_what_its_files_state() {
    let repo = Repo::new(
        "conventions-effect",
        &[
            ("AGENTS.md", AGENTS),
            ("rustfmt.toml", "max_width = 100\n"),
            ("message.txt", "added stuff\n"),
        ],
    );
    assert_eq!(declared_ids(&repo), ["commits", "rustfmt"]);
    // The commit check the hooks run now speaks without any configuration.
    let message = repo.root.join("message.txt").to_string_lossy().into_owned();
    let (code, stdout, stderr) = craft(&repo, &["standards", "--commit-message", &message]);
    assert_ne!(code, 0);
    let said = format!("{stdout}{stderr}");
    assert!(said.contains(r#""standard":"commits""#), "{said}");
    // Still listed as proposals with their evidence, not as declared twice.
    let proposed = propose(&repo);
    assert_eq!(standards(&proposed).len(), 2);
    assert_eq!(proposed["declared"], json!([]));
    // `craft config` names the layer they came from.
    let report: Value =
        serde_json::from_str(&craft(&repo, &["config", "--json"]).1).expect("report");
    let detected = report["layers"]
        .as_array()
        .expect("layers")
        .iter()
        .find(|layer| layer["name"] == "detected")
        .expect("detected layer");
    assert_eq!(detected["values"], 2);
    let text = craft_engine::layers::text(&report);
    assert!(text.contains("bundled < detected < organization"), "{text}");
    assert!(text.contains("read from the project's own files"), "{text}");
    assert!(!repo.root.join(".craft/project.json").exists());
}

#[test]
fn declared_standards_replace_what_the_files_state_and_the_switch_turns_it_off() {
    for (config, expected) in [
        (
            r#"{"formatVersion":1,"standards":[{"id":"api","file":"docs/api.md"}]}"#,
            vec!["api"],
        ),
        (
            r#"{"formatVersion":1,"routing":{"conventionProposals":false}}"#,
            vec![],
        ),
    ] {
        let repo = Repo::new(
            "conventions-replaced",
            &[
                ("AGENTS.md", AGENTS),
                ("docs/api.md", "# API\n"),
                (".craft/project.json", config),
            ],
        );
        assert_eq!(declared_ids(&repo), expected, "{config}");
    }
}

#[test]
fn a_commit_format_the_history_keeps_is_in_effect_and_git_is_asked_once_per_commit() {
    let repo = Repo::new("conventions-practice", &[("README.md", "x\n")]);
    for number in 0..20 {
        let subject = format!("fix: correct case {number}");
        repo.git(&["commit", "-q", "--allow-empty", "-m", &subject]);
    }
    assert_eq!(declared_ids(&repo), ["commits"]);
    // A command a person runs keeps nothing in the project's state; a hook does.
    let kept = repo.root.join(".craft/state/notices/practice.json");
    assert!(!kept.exists());
    assert!(start("claude", &repo, "S1").contains("commits"));
    let proposed = propose(&repo);
    let evidence = proposed["proposals"][0]["evidence"][0]["observed"]
        .as_str()
        .expect("observed");
    assert!(evidence.contains("commit subjects follow it"), "{evidence}");
    // The answer is kept for the commit the checkout is at: the next command reads it
    // instead of starting Git, and a new commit asks again.
    let record: Value =
        serde_json::from_str(&std::fs::read_to_string(&kept).expect("kept")).expect("record");
    let mut edited = record.clone();
    edited["subjects"] = json!([]);
    std::fs::write(&kept, edited.to_string()).expect("write");
    assert!(declared_ids(&repo).is_empty(), "the kept answer is read");
    repo.git(&["commit", "-q", "--allow-empty", "-m", "fix: one more"]);
    assert_eq!(declared_ids(&repo), ["commits"]);
    assert_eq!(
        std::fs::read_to_string(&kept).expect("kept"),
        edited.to_string(),
        "only a hook writes"
    );
}

/// A command run from a folder of the project, without `--root`, is the project's: the same
/// configuration a hook reads, and no second `.craft` beside the folder. The host's own
/// project directory, where it names one to a hook, decides the same way for the hook.
#[test]
fn a_command_or_hook_started_below_the_project_root_belongs_to_the_project() {
    let repo = Repo::new(
        "conventions-subfolder",
        &[("AGENTS.md", AGENTS), ("src/deep/a.txt", "a\n")],
    );
    let deep = repo.root.join("src/deep");
    let home = repo.root.join(".home");
    std::fs::create_dir_all(&home).expect("home");
    let craft = |args: &[&str], input: &str, directory: Option<&std::path::Path>| {
        let mut command = std::process::Command::new(env!("CARGO_BIN_EXE_craft"));
        command
            .args(args)
            .current_dir(&deep)
            .env("HOME", &home)
            .env("USERPROFILE", &home)
            .env("CRAFT_ORGANIZATION_CONFIG", home.join("none.json"))
            .env("CRAFT_EVIDENCE_WARM", "0")
            .env_remove("CLAUDE_PROJECT_DIR")
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped());
        if let Some(directory) = directory {
            command.env("CLAUDE_PROJECT_DIR", directory);
        }
        let mut child = command.spawn().expect("craft");
        use std::io::Write;
        child
            .stdin
            .take()
            .expect("stdin")
            .write_all(input.as_bytes())
            .expect("input");
        let output = child.wait_with_output().expect("output");
        assert!(output.status.success(), "{output:?}");
        String::from_utf8_lossy(&output.stdout).into_owned()
    };
    let listing: Value = serde_json::from_str(&craft(&["standards"], "", None)).expect("listing");
    assert_eq!(listing["standards"][0]["id"], "commits", "{listing}");
    assert!(!deep.join(".craft").exists());

    // A hook whose shell is elsewhere, in a host that names the directory it was opened in.
    let elsewhere = Repo::new("conventions-elsewhere", &[("README.md", "x\n")]);
    let payload = json!({"session_id": "S1", "cwd": elsewhere.path(), "source": "startup",
        "hook_event_name": "SessionStart"});
    let hook = [
        "hook",
        "claude",
        "session-start",
        "--package",
        "craft-delivery",
        "--handler",
        "deliver",
    ];
    let said = craft(&hook, &payload.to_string(), Some(&deep));
    assert!(said.contains("commits"), "{said}");
    assert!(repo.root.join(".craft/state/sessions").exists());
    assert!(!elsewhere.root.join(".craft").exists());
}

/// A folder that is no repository and holds two: opened there, a session is held to each
/// repository's own standards within its folder, and the workspace keeps one `.craft`.
#[test]
fn a_workspace_root_is_held_to_what_each_member_repository_states() {
    let api = Repo::new(
        "workspace-api",
        &[("AGENTS.md", AGENTS), ("rustfmt.toml", "\n")],
    );
    let home = api
        .root
        .parent()
        .expect("parent")
        .join(format!("craft-workspace-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&home);
    std::fs::create_dir_all(home.join("notes")).expect("workspace");
    std::fs::rename(&api.root, home.join("api")).expect("member");
    let web = Repo::new("workspace-web", &[("biome.json", "{}\n")]);
    std::fs::rename(&web.root, home.join("web")).expect("member");
    let root = home.to_string_lossy().into_owned();
    let run = |args: &[&str]| -> Value {
        let mut argv: Vec<String> = vec![args[0].into(), "--root".into(), root.clone()];
        argv.extend(args[1..].iter().map(|arg| arg.to_string()));
        let outcome = cli::run_structured(&argv);
        assert_eq!(outcome.exit_code, 0, "{}", outcome.stderr);
        serde_json::from_str(&outcome.stdout).expect("JSON")
    };
    let listed = |value: &Value| -> Vec<String> {
        value["standards"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|standard| standard["id"].as_str().map(str::to_owned))
            .collect()
    };
    assert_eq!(listed(&run(&["standards"])), ["api-rustfmt", "web-biome"]);
    // A file is routed with its own repository's standards, not its neighbour's.
    let routed = run(&["route", "--intent", "implement", "--file", "web/src/app.ts"]);
    assert_eq!(listed(&routed), ["web-biome"], "{routed}");
    let proposed = run(&["standards", "--propose"]);
    assert_eq!(proposed["members"], json!(["api", "web"]));
    assert!(!home.join("api/.craft").exists() && !home.join("web/.craft").exists());
    let _ = std::fs::remove_dir_all(&home);
}
