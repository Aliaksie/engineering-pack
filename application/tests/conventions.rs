//! Conventions a project already states become proposals the consumer can accept: the scan
//! reads the project's own files, writes nothing, and what it proposes is valid configuration.
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
    assert_eq!(
        first["systemMessage"],
        "Craft: this project's own files support 2 standards it has not declared (commits, rustfmt). `craft standards --propose` lists them with evidence; none is active until you add it."
    );
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
