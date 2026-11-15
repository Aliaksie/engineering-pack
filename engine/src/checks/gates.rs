//! The declared checks at Git's and CI's gates, from the same declaration `craft check` runs:
//! the pre-commit hook `craft check --install-hook` writes, the line to add where a hook
//! manager or another hook already owns pre-commit, and the CI job `--ci-snippet` prints.
use serde_json::{Value, json};

type Result<T = ()> = std::result::Result<T, String>;

/// The line that marks a pre-commit hook as Craft's own, so it is replaced and never another.
pub const HOOK_MARK: &str = "# craft: pre-commit hook written by `craft check --install-hook`.";
/// Where Craft's releases are published.
pub const RELEASE_REPOSITORY: &str = "Aliaksie/engineering-pack";
/// The runtime a CI job downloads: the hosted Linux runners of both providers.
const CI_TARGET: &str = "x86_64-unknown-linux-gnu";
/// The pinned checkout action of the GitHub job.
const CHECKOUT: &str = "actions/checkout@3d3c42e5aac5ba805825da76410c181273ba90b1 # v7.0.1";
pub const CI_PROVIDERS: [&str; 2] = ["github", "gitlab"];

/// A hook manager that writes Git's hooks itself, found from its configuration file.
pub const MANAGERS: [(&str, &[&str]); 3] = [
    ("husky", &[".husky"]),
    (
        "lefthook",
        &[
            "lefthook.yml",
            ".lefthook.yml",
            "lefthook.yaml",
            ".lefthook.yaml",
            "lefthook.json",
            ".lefthook.json",
            "lefthook.toml",
            ".lefthook.toml",
        ],
    ),
    (
        "pre-commit",
        &[".pre-commit-config.yaml", ".pre-commit-config.yml"],
    ),
];

/// `value` as one single-quoted POSIX shell word.
pub fn quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\"'\"'"))
}

/// A path a gate may name: one line of text without control characters.
fn plain(value: &str, what: &str) -> Result {
    if value.is_empty() || value.len() > 4096 || value.chars().any(char::is_control) {
        return Err(format!("{what} must be a single line of text"));
    }
    Ok(())
}

/// The `craft check` arguments of a gate for a project `root` named from the top of its
/// checkout (`""` at the top).
fn check_arguments(stage: &str, root: &str) -> Result<String> {
    let root = root.trim_end_matches('/');
    if root.is_empty() {
        return Ok(format!("check --stage {stage}"));
    }
    plain(root, "the project folder")?;
    Ok(format!("check --stage {stage} --root {}", quote(root)))
}

/// The command a pre-commit gate runs: `launcher` at the commit stage for the project at
/// `root` below the checkout top.
pub fn hook_command(launcher: &str, root: &str) -> Result<String> {
    plain(launcher, "the craft launcher path")?;
    Ok(format!(
        "{} {}",
        quote(launcher),
        check_arguments("commit", root)?
    ))
}

/// The pre-commit hook Craft writes. A launcher that is gone (Craft moved or updated) lets
/// the commit through and says so: the gate is the project's, a stale path must not lock
/// every commit.
pub fn hook_script(launcher: &str, root: &str) -> Result<String> {
    plain(launcher, "the craft launcher path")?;
    let arguments = check_arguments("commit", root)?;
    Ok(format!(
        "#!/bin/sh\n{HOOK_MARK}\n\
         # Runs the project's declared commit-stage checks for the changed files; a check that\n\
         # does not pass stops the commit. `git commit --no-verify` skips it.\n\
         # Git's own locations would point the checks' Git calls at the wrong place below the top.\n\
         unset GIT_DIR GIT_WORK_TREE GIT_INDEX_FILE\n\
         craft={launcher}\n\
         if [ ! -f \"$craft\" ]; then\n  \
           printf '%s\\n' \"craft: $craft is gone; the commit-stage checks did not run. Run craft check --install-hook --apply again.\" >&2\n  \
           exit 0\n\
         fi\n\
         exec \"$craft\" {arguments}\n",
        launcher = quote(launcher),
    ))
}

/// What a pre-commit hook install does, from what the checkout holds:
/// `managers` are the hook managers whose configuration is present, `hooks_path` Git's
/// `core.hooksPath`, `existing` the current pre-commit hook. Never overwrites a hook that is
/// not Craft's or one a manager writes: it prints the line to add there instead.
pub fn hook_plan(
    managers: &[&str],
    hooks_path: Option<&str>,
    existing: Option<&str>,
    script: &str,
    command: &str,
) -> Value {
    let print = |manager: &str, reason: String, add: String| json!({"action": "print", "manager": manager, "reason": reason, "add": add});
    if let Some(manager) = managers.first() {
        let add = match *manager {
            "husky" => format!("{command}\n"),
            "lefthook" => format!(
                "pre-commit:\n  commands:\n    craft-check:\n      run: {}\n",
                yaml_string(command)
            ),
            _ => format!(
                "- repo: local\n  hooks:\n    - id: craft-check\n      name: craft check\n      entry: {}\n      language: system\n      pass_filenames: false\n      always_run: true\n",
                yaml_string(command)
            ),
        };
        let place = match *manager {
            "husky" => ".husky/pre-commit",
            "lefthook" => "the lefthook configuration",
            _ => ".pre-commit-config.yaml under `repos:`",
        };
        return print(
            manager,
            format!("{manager} manages this repository's Git hooks; add this to {place}"),
            add,
        );
    }
    if let Some(path) = hooks_path {
        return print(
            "core.hooksPath",
            format!(
                "Git runs hooks from core.hooksPath ({path}); add this line to its pre-commit hook"
            ),
            format!("{command}\n"),
        );
    }
    match existing {
        Some(current) if current == script => json!({"action": "current"}),
        Some(current) if current.lines().nth(1) == Some(HOOK_MARK) => {
            json!({"action": "update"})
        }
        Some(_) => print(
            "hook",
            "a pre-commit hook that is not Craft's is in place; add this line to it".into(),
            format!("{command}\n"),
        ),
        None => json!({"action": "write"}),
    }
}

/// A YAML double-quoted scalar holding `text`.
fn yaml_string(text: &str) -> String {
    serde_json::to_string(text).unwrap_or_default()
}

/// The CI job running the declared checks for a pull or merge request, with Craft `version`
/// fetched from its release and checked against the release's checksums. `root` is the project
/// folder below the repository top (`""` at the top).
pub fn ci_snippet(provider: &str, version: &str, root: &str) -> Result<String> {
    if !version
        .split('.')
        .all(|part| !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_digit()))
        || version.split('.').count() != 3
    {
        return Err("this craft carries no release version to pin".into());
    }
    let asset = format!("engineering-pack-{version}-native-{CI_TARGET}.tar.gz");
    let download = format!("https://github.com/{RELEASE_REPOSITORY}/releases/download/v{version}");
    let arguments = check_arguments("ci", root)?;
    let lines: Vec<String> = match provider {
        "github" => [
            "# Craft: the project's declared checks for each pull request, as `craft check` runs them.",
            &format!("# Save as .github/workflows/craft-check.yml (from `craft check --ci-snippet github`, Craft {version})."),
            "# Add the steps that install the tools the declared checks run before \"Declared checks\".",
            "name: Craft check",
            "on:",
            "  pull_request:",
            "permissions:",
            "  contents: read",
            "jobs:",
            "  craft-check:",
            "    runs-on: ubuntu-latest",
            "    steps:",
            &format!("      - uses: {CHECKOUT}"),
            "        with:",
            "          fetch-depth: 0",
            "          persist-credentials: false",
            &format!("      - name: Install Craft {version}"),
            "        env:",
            "          GH_TOKEN: ${{ github.token }}",
            &format!("          CRAFT_ASSET: {asset}"),
            "        run: |",
            "          mkdir -p \"$RUNNER_TEMP/craft\"",
            "          cd \"$RUNNER_TEMP/craft\"",
            &format!("          gh release download v{version} --repo {RELEASE_REPOSITORY} --pattern \"$CRAFT_ASSET\" --pattern SHA256SUMS"),
            "          sha256sum --check --ignore-missing SHA256SUMS",
            &format!("          gh attestation verify \"$CRAFT_ASSET\" --repo {RELEASE_REPOSITORY}"),
            "          tar -xzf \"$CRAFT_ASSET\"",
            "      - name: Declared checks",
            "        env:",
            "          BASE_REF: ${{ github.base_ref }}",
            "        run: |",
            &format!("          \"$RUNNER_TEMP/craft/native-bin/craft\" {arguments} --base \"origin/$BASE_REF\""),
        ]
        .map(str::to_owned)
        .to_vec(),
        "gitlab" => [
            "# Craft: the project's declared checks for each merge request, as `craft check` runs them.",
            &format!("# Add to .gitlab-ci.yml (from `craft check --ci-snippet gitlab`, Craft {version})."),
            "# The job's image needs curl, git, tar, sha256sum and the tools the declared checks run.",
            "craft-check:",
            "  stage: test",
            "  rules:",
            "    - if: $CI_PIPELINE_SOURCE == \"merge_request_event\"",
            "  variables:",
            "    GIT_DEPTH: \"0\"",
            &format!("    CRAFT_ASSET: {asset}"),
            &format!("    CRAFT_RELEASE: {download}"),
            "  script:",
            "    - |",
            "      craft_dir=\"$(mktemp -d)\"",
            "      curl -fsSL -o \"$craft_dir/$CRAFT_ASSET\" \"$CRAFT_RELEASE/$CRAFT_ASSET\"",
            "      curl -fsSL -o \"$craft_dir/SHA256SUMS\" \"$CRAFT_RELEASE/SHA256SUMS\"",
            "      (cd \"$craft_dir\" && sha256sum --check --ignore-missing SHA256SUMS)",
            "      tar -xzf \"$craft_dir/$CRAFT_ASSET\" -C \"$craft_dir\"",
            "      target=\"$CI_MERGE_REQUEST_TARGET_BRANCH_NAME\"",
            "      git fetch origin \"+refs/heads/$target:refs/remotes/origin/$target\"",
            &format!("      \"$craft_dir/native-bin/craft\" {arguments} --base \"origin/$target\""),
        ]
        .map(str::to_owned)
        .to_vec(),
        _ => {
            return Err(format!(
                "--ci-snippet takes one of: {}",
                CI_PROVIDERS.join(", ")
            ));
        }
    };
    Ok(lines.join("\n") + "\n")
}

/// The receipt of a hook install or a CI snippet.
pub fn text(report: &Value) -> String {
    if let Some(snippet) = report["ciSnippet"].as_str() {
        return snippet.trim_end().to_string();
    }
    let hook = &report["hook"];
    let path = hook["path"].as_str().unwrap_or("pre-commit");
    let command = hook["command"].as_str().unwrap_or_default();
    match hook["action"].as_str().unwrap_or_default() {
        "print" => format!(
            "{}; nothing was written:\n{}",
            hook["reason"].as_str().unwrap_or_default(),
            hook["add"].as_str().unwrap_or_default().trim_end()
        ),
        "current" => format!("{path} already runs: {command}"),
        action if report["applied"] == true => format!(
            "{} {path}: each commit runs {command}",
            if action == "update" {
                "Updated"
            } else {
                "Wrote"
            }
        ),
        action => format!(
            "Would {} {path} to run {command} on each commit; run again with --apply to write it.",
            if action == "update" {
                "update"
            } else {
                "write"
            }
        ),
    }
}

#[cfg(test)]
mod tests;
