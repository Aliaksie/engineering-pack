//! The declared checks at Git's pre-commit hook and in CI: `craft check --install-hook`
//! writes Craft's hook only on `--apply` and never takes a hook it does not own;
//! `--ci-snippet` prints a job pinned to this release.
mod support;
use std::fs;
use support::{Repo, assert_err};

/// A commit-stage check whose result the test sets: it fails while `api/bad` exists.
const PROJECT: &str = r#"{"formatVersion":1,
  "modules":[{"name":"api","path":"api","checks":[{"command":"git","args":["diff","--check"],"id":"api-whitespace"}]}]}"#;

fn hook(repo: &Repo) -> std::path::PathBuf {
    repo.root.join(".git/hooks/pre-commit")
}

#[test]
fn the_hook_is_written_on_apply_and_kept_current() {
    let repo = Repo::new(
        "check-gates",
        &[(".craft/project.json", PROJECT), ("api/a.txt", "a\n")],
    );
    let _ = fs::remove_file(hook(&repo));
    let preview = repo.cli(&["check", "--install-hook", "--json"]).unwrap();
    assert_eq!(preview["applied"], false);
    assert_eq!(preview["hook"]["action"], "write");
    assert_eq!(preview["hook"]["path"], ".git/hooks/pre-commit");
    assert!(!hook(&repo).exists(), "a preview writes nothing");

    let written = repo
        .cli(&["check", "--install-hook", "--apply", "--json"])
        .unwrap();
    assert_eq!(written["applied"], true);
    let script = fs::read_to_string(hook(&repo)).unwrap();
    assert!(script.contains("check --stage commit"));
    assert!(
        !script.contains("--root"),
        "the project is the checkout top"
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = fs::metadata(hook(&repo)).unwrap().permissions().mode();
        assert_eq!(mode & 0o111, 0o111, "the hook is executable");
    }
    let again = repo
        .cli(&["check", "--install-hook", "--apply", "--json"])
        .unwrap();
    assert_eq!(again["hook"]["action"], "current");
    assert_eq!(again["applied"], false);

    // Craft's own hook from another launcher is replaced.
    fs::write(
        hook(&repo),
        script.replace("craft=", "craft='/gone/craft' #"),
    )
    .unwrap();
    let updated = repo
        .cli(&["check", "--install-hook", "--apply", "--json"])
        .unwrap();
    assert_eq!(updated["hook"]["action"], "update");
    assert_eq!(fs::read_to_string(hook(&repo)).unwrap(), script);
}

#[test]
fn a_hook_or_manager_that_owns_pre_commit_gets_a_line_and_keeps_its_file() {
    let repo = Repo::new(
        "check-gates-owned",
        &[(".craft/project.json", PROJECT), ("api/a.txt", "a\n")],
    );
    repo.write(".git/hooks/pre-commit", "#!/bin/sh\nnpm test\n");
    let foreign = repo
        .cli(&["check", "--install-hook", "--apply", "--json"])
        .unwrap();
    assert_eq!(foreign["applied"], false);
    assert_eq!(foreign["hook"]["action"], "print");
    assert_eq!(foreign["hook"]["manager"], "hook");
    assert!(
        foreign["hook"]["add"]
            .as_str()
            .unwrap()
            .contains("check --stage commit")
    );
    assert_eq!(
        fs::read_to_string(hook(&repo)).unwrap(),
        "#!/bin/sh\nnpm test\n"
    );

    repo.write(".pre-commit-config.yaml", "repos: []\n");
    let framework = repo
        .cli(&["check", "--install-hook", "--apply", "--json"])
        .unwrap();
    assert_eq!(framework["hook"]["manager"], "pre-commit");
    assert!(
        framework["hook"]["add"]
            .as_str()
            .unwrap()
            .starts_with("- repo: local\n")
    );

    let hooks_path = Repo::new("check-gates-path", &[(".craft/project.json", PROJECT)]);
    hooks_path.git(&["config", "core.hooksPath", ".githooks"]);
    let configured = hooks_path
        .cli(&["check", "--install-hook", "--apply", "--json"])
        .unwrap();
    assert_eq!(configured["hook"]["manager"], "core.hooksPath");
    assert!(!hooks_path.root.join(".githooks/pre-commit").exists());
}

#[test]
fn a_project_below_the_top_runs_from_its_folder_and_a_workspace_root_is_refused() {
    let repo = Repo::new(
        "check-gates-below",
        &[
            ("svc/.craft/project.json", PROJECT),
            ("svc/api/a.txt", "a\n"),
        ],
    );
    let argv: Vec<String> = [
        "check",
        "--root",
        &format!("{}/svc", repo.path()),
        "--install-hook",
        "--json",
    ]
    .iter()
    .map(|arg| arg.to_string())
    .collect();
    let outcome = craft_application::cli::run_structured(&argv);
    assert_eq!(outcome.exit_code, 0, "{}", outcome.stderr);
    let preview: serde_json::Value = serde_json::from_str(&outcome.stdout).unwrap();
    assert!(
        preview["hook"]["command"]
            .as_str()
            .unwrap()
            .ends_with("check --stage commit --root 'svc'"),
        "{preview}"
    );

    let workspace = Repo::bare("check-gates-workspace");
    workspace.write(".craft/project.json", PROJECT);
    assert_err(
        workspace.cli(&["check", "--install-hook"]),
        &["no Git checkout at the project root"],
    );
}

#[test]
fn ci_snippets_print_a_pinned_job_and_take_no_run_options() {
    let repo = Repo::new("check-gates-ci", &[(".craft/project.json", PROJECT)]);
    let github = repo
        .cli(&["check", "--ci-snippet", "github", "--json"])
        .unwrap();
    let version = github["version"].as_str().unwrap();
    let snippet = github["ciSnippet"].as_str().unwrap();
    assert!(snippet.contains(&format!("gh release download v{version} ")));
    assert!(snippet.contains("check --stage ci --base \"origin/$BASE_REF\""));
    let gitlab = repo
        .cli(&["check", "--ci-snippet", "gitlab", "--json"])
        .unwrap();
    assert!(
        gitlab["ciSnippet"]
            .as_str()
            .unwrap()
            .contains("CI_MERGE_REQUEST_TARGET_BRANCH_NAME")
    );

    assert_err(
        repo.cli(&["check", "--ci-snippet", "jenkins"]),
        &["--ci-snippet takes one of: github, gitlab"],
    );
    assert_err(
        repo.cli(&["check", "--ci-snippet", "github", "--apply"]),
        &["takes no --apply"],
    );
    assert_err(
        repo.cli(&["check", "--install-hook", "--base", "main"]),
        &["take no --base"],
    );
    assert_err(
        repo.cli(&["check", "--apply"]),
        &["--apply only with --install-hook"],
    );
    assert_err(
        repo.cli(&["standards", "--install-hook"]),
        &["--install-hook is supported by check"],
    );
}

/// The written hook, run by Git itself: a commit whose check fails stops, one that passes goes
/// through. The hook is a POSIX shell script; Windows runs it through Git's own shell.
#[cfg(unix)]
#[test]
fn git_runs_the_declared_checks_before_each_commit() {
    let repo = Repo::new(
        "check-gates-commit",
        &[(".craft/project.json", PROJECT), ("api/a.txt", "a\n")],
    );
    let craft = env!("CARGO_BIN_EXE_craft");
    let installed = std::process::Command::new(craft)
        .args(["check", "--root", repo.path(), "--install-hook", "--apply"])
        .env_remove("CRAFT_PACKAGE_ROOT")
        .output()
        .unwrap();
    assert!(
        installed.status.success(),
        "{}",
        String::from_utf8_lossy(&installed.stderr)
    );
    let commit = |message: &str| {
        std::process::Command::new("git")
            .arg("-C")
            .arg(&repo.root)
            .args([
                "-c",
                "commit.gpgsign=false",
                "-c",
                "user.name=Fixture",
                "-c",
                "user.email=fixture@example.test",
                "commit",
                "-qam",
                message,
            ])
            .env_remove("CRAFT_PACKAGE_ROOT")
            .output()
            .unwrap()
    };
    repo.write("api/a.txt", "b \n");
    let refused = commit("feat: trailing space");
    assert!(!refused.status.success(), "the failing check stops it");
    // Git hands a hook's output to its own standard error.
    let said = format!(
        "{}{}",
        String::from_utf8_lossy(&refused.stdout),
        String::from_utf8_lossy(&refused.stderr)
    );
    assert!(said.contains("failed api-whitespace"), "{said}");

    repo.write("api/a.txt", "b\n");
    let passed = commit("feat: clean");
    assert!(
        passed.status.success(),
        "{}{}",
        String::from_utf8_lossy(&passed.stdout),
        String::from_utf8_lossy(&passed.stderr)
    );
}
