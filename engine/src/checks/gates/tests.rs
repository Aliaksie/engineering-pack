use super::*;

const LAUNCHER: &str = "/opt/craft it's/bin/craft";

#[test]
fn hook_runs_the_commit_stage_from_the_project_folder() {
    let command = hook_command(LAUNCHER, "").unwrap();
    assert_eq!(
        command,
        "'/opt/craft it'\"'\"'s/bin/craft' check --stage commit"
    );
    let below = hook_command(LAUNCHER, "services/api/").unwrap();
    assert!(below.ends_with("check --stage commit --root 'services/api'"));
    let script = hook_script(LAUNCHER, "services/api/").unwrap();
    assert_eq!(script.lines().nth(1), Some(HOOK_MARK));
    assert!(script.starts_with("#!/bin/sh\n"));
    assert!(script.contains("\nunset GIT_DIR GIT_WORK_TREE GIT_INDEX_FILE\n"));
    assert!(script.contains("\n  exit 0\n"));
    assert!(script.ends_with("exec \"$craft\" check --stage commit --root 'services/api'\n"));
    assert!(hook_command("bad\nline", "").is_err());
    assert!(hook_script(LAUNCHER, "a\tb").is_err());
}

#[test]
fn hook_install_never_takes_a_hook_it_does_not_own() {
    let script = hook_script(LAUNCHER, "").unwrap();
    let command = hook_command(LAUNCHER, "").unwrap();
    let plan = |managers: &[&str], path: Option<&str>, existing: Option<&str>| {
        hook_plan(managers, path, existing, &script, &command)
    };
    assert_eq!(plan(&[], None, None)["action"], "write");
    assert_eq!(plan(&[], None, Some(&script))["action"], "current");
    let older = script.replace("/opt", "/usr/local");
    assert_eq!(plan(&[], None, Some(&older))["action"], "update");

    let foreign = plan(&[], None, Some("#!/bin/sh\nnpm test\n"));
    assert_eq!(foreign["action"], "print");
    assert_eq!(foreign["manager"], "hook");
    assert_eq!(foreign["add"], format!("{command}\n"));

    let hooks_path = plan(&[], Some(".githooks"), None);
    assert_eq!(hooks_path["manager"], "core.hooksPath");
    assert!(hooks_path["reason"].as_str().unwrap().contains(".githooks"));

    // A manager rewrites Git's hook itself, so its configuration wins over every other fact.
    let husky = plan(&["husky"], Some(".husky/_"), Some(&script));
    assert_eq!(husky["action"], "print");
    assert_eq!(husky["add"], format!("{command}\n"));
    let lefthook = plan(&["lefthook"], None, None);
    let add = lefthook["add"].as_str().unwrap();
    assert!(add.starts_with("pre-commit:\n  commands:\n    craft-check:\n      run: \""));
    let framework = plan(&["pre-commit"], None, None);
    let add = framework["add"].as_str().unwrap();
    assert!(add.contains("language: system\n"));
    assert!(add.contains("pass_filenames: false\n"));
    // The quoted launcher survives YAML: the entry is a double-quoted scalar of the command.
    let entry = add.lines().find(|line| line.contains("entry:")).unwrap();
    let scalar: String = serde_json::from_str(entry.trim().trim_start_matches("entry: ")).unwrap();
    assert_eq!(scalar, command);
}

#[test]
fn ci_snippets_pin_the_release_and_check_since_the_target_branch() {
    let github = ci_snippet("github", "0.0.7", "").unwrap();
    assert!(github.contains("gh release download v0.0.7 --repo Aliaksie/engineering-pack"));
    assert!(
        github.contains(
            "CRAFT_ASSET: engineering-pack-0.0.7-native-x86_64-unknown-linux-gnu.tar.gz\n"
        )
    );
    assert!(github.contains("sha256sum --check --ignore-missing SHA256SUMS\n"));
    assert!(github.contains("fetch-depth: 0\n"));
    // The base branch reaches the shell through the environment, never inline.
    assert!(github.contains("BASE_REF: ${{ github.base_ref }}\n"));
    assert!(github.contains(
        "\"$RUNNER_TEMP/craft/native-bin/craft\" check --stage ci --base \"origin/$BASE_REF\"\n"
    ));
    assert!(!github.contains("run: \"") && !github.contains("${{ github.base_ref }}\""));
    for line in github.lines().filter(|line| !line.is_empty()) {
        let indent = line.len() - line.trim_start().len();
        assert!(indent % 2 == 0, "odd indentation: {line}");
    }

    let gitlab = ci_snippet("gitlab", "0.0.7", "services/api").unwrap();
    assert!(gitlab.contains(
        "CRAFT_RELEASE: https://github.com/Aliaksie/engineering-pack/releases/download/v0.0.7\n"
    ));
    assert!(gitlab.contains("GIT_DEPTH: \"0\"\n"));
    assert!(gitlab.contains("check --stage ci --root 'services/api' --base \"origin/$target\"\n"));
    assert!(gitlab.starts_with("# Craft:"));

    assert!(ci_snippet("jenkins", "0.0.7", "").is_err());
    assert!(ci_snippet("github", "", "").is_err());
    assert!(ci_snippet("github", "0.0", "").is_err());
}

#[test]
fn receipts_read_as_one_answer() {
    let snippet = json!({"ciSnippet": "name: x\n"});
    assert_eq!(text(&snippet), "name: x");
    let hook = |action: &str, applied: bool| {
        text(&json!({"applied": applied, "hook": {"action": action,
            "path": ".git/hooks/pre-commit", "command": "craft check --stage commit",
            "reason": "husky manages this repository's Git hooks; add this to .husky/pre-commit",
            "add": "craft check --stage commit\n"}}))
    };
    assert!(hook("write", false).ends_with("run again with --apply to write it."));
    assert!(hook("write", true).starts_with("Wrote .git/hooks/pre-commit"));
    assert!(hook("update", true).starts_with("Updated "));
    assert!(hook("current", false).contains("already runs"));
    assert_eq!(
        hook("print", false),
        "husky manages this repository's Git hooks; add this to .husky/pre-commit; nothing was written:\ncraft check --stage commit"
    );
}
