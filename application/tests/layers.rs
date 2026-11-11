//! Configuration layers as a team meets them: one organization file and one user file shared
//! by two repositories, on each deep host. A repository with no configuration of its own
//! inherits the baseline in its hooks and commands; the other overrides only what differs,
//! and `craft config` names the layer that won and the one it hid. A layer file that cannot
//! be used is an error naming the file, never a silent fallback.
mod support;
use serde_json::{Value, json};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use support::Repo;

/// Git stands in for the organization's own tools: every test host has it.
const ORGANIZATION: &str = r#"{"formatVersion": 1,
  "routing": {"debtMarkers": true, "sessionSummary": false},
  "standards": [
    {"id": "commits", "anchor": "conventional-commits"},
    {"id": "review", "checklist": ["Tests cover the change"]},
    {"id": "licenses", "run": {"command": "git", "args": ["--version"]}},
    {"id": "audit", "run": {"command": "git", "args": ["--version"], "stage": "ci"}}],
  "safety": {"enabled": true, "deny": [
    {"id": "infra-destroy", "command": ["terraform", "destroy"], "reason": "tearing down shared infrastructure"}]}
}"#;
const USER: &str =
    r#"{"routing": {"sessionSummary": true}, "efficiency": {"review": {"mode": "manual"}}}"#;
/// The one repository that differs: its own review checklist, no license check, markers off.
const PROJECT: &str = r#"{"formatVersion": 1,
  "routing": {"debtMarkers": false},
  "standards": [
    {"id": "review", "checklist": ["Tests and a changelog line"]},
    {"id": "licenses", "disabled": true},
    {"id": "legacy", "disabled": true}]
}"#;

/// The machine both repositories live on: the organization file and a home directory
/// holding the user's file.
struct Machine {
    dir: PathBuf,
}

impl Drop for Machine {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.dir);
    }
}

fn private(path: &Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = if path.is_dir() { 0o700 } else { 0o600 };
        fs::set_permissions(path, fs::Permissions::from_mode(mode)).unwrap();
    }
    #[cfg(not(unix))]
    let _ = path;
}

impl Machine {
    fn new(name: &str, organization: &str, user: Option<&str>) -> Self {
        let dir = std::env::temp_dir().join(format!("craft-layers-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let settings = dir.join("home/.config/craft");
        fs::create_dir_all(&settings).unwrap();
        fs::create_dir_all(dir.join("managed")).unwrap();
        let machine = Self { dir };
        fs::write(machine.organization(), organization).unwrap();
        if let Some(user) = user {
            fs::write(settings.join("preferences.json"), user).unwrap();
            private(&settings.join("preferences.json"));
        }
        private(&settings);
        private(&machine.dir.join("managed"));
        private(&machine.organization());
        machine
    }

    fn organization(&self) -> PathBuf {
        self.dir.join("managed/organization.json")
    }

    /// `craft <args>` on this machine with `input` on stdin.
    fn craft(&self, args: &[&str], input: &str) -> Output {
        let home = self.dir.join("home");
        let mut child = Command::new(env!("CARGO_BIN_EXE_craft"))
            .args(args)
            .env("CRAFT_ORGANIZATION_CONFIG", self.organization())
            .env("HOME", &home)
            .env("USERPROFILE", &home)
            .env("CRAFT_EVIDENCE_WARM", "0")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        use std::io::Write;
        child
            .stdin
            .take()
            .unwrap()
            .write_all(input.as_bytes())
            .unwrap();
        child.wait_with_output().unwrap()
    }

    /// The JSON result of a command that succeeds.
    fn json(&self, repo: &Repo, args: &[&str]) -> Value {
        let mut argv = vec![args[0], "--root", repo.path(), "--json"];
        argv.extend(&args[1..]);
        let output = self.craft(&argv, "");
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(output.status.success(), "{args:?}: {stderr}");
        serde_json::from_slice(&output.stdout).unwrap_or_else(|_| panic!("{args:?}: {stderr}"))
    }

    /// What the host is told by a hook: `deny <reason>` for a refused tool call, else the
    /// context the hook adds.
    fn hook(&self, host: &str, repo: &Repo, lifecycle: &str, extra: Value) -> String {
        let mut payload = json!({"session_id": format!("S-{host}"), "cwd": repo.path()});
        for (key, value) in extra.as_object().unwrap() {
            payload[key] = value.clone();
        }
        let args = [
            "hook",
            host,
            lifecycle,
            "--package",
            "craft-delivery",
            "--handler",
            "deliver",
        ];
        let output = self.craft(&args, &payload.to_string());
        let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        match output.status.code() {
            Some(2) => format!("deny {stderr}"),
            Some(0) if stdout.contains(r#""deny""#) => format!("deny {stdout}"),
            Some(0) => {
                let wire: Value = serde_json::from_str(&stdout).unwrap_or(Value::Null);
                wire["hookSpecificOutput"]["additionalContext"]
                    .as_str()
                    .map_or(stdout, str::to_string)
            }
            code => panic!("{lifecycle} exit {code:?}: {stderr}"),
        }
    }
}

fn shell(host: &str, command: &str, output: &str) -> Value {
    let response = match host {
        "codex" => json!(output),
        _ => json!({"stdout": output, "stderr": ""}),
    };
    json!({"hook_event_name": "PostToolUse", "tool_name": "Bash",
        "tool_input": {"command": command}, "tool_response": response})
}

fn row<'a>(report: &'a Value, list: &str, field: &str) -> &'a Value {
    report[list]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["field"] == field)
        .unwrap_or_else(|| panic!("{field} in {report}"))
}

fn ids(list: &Value) -> Vec<&str> {
    list.as_array()
        .unwrap_or_else(|| panic!("list: {list}"))
        .iter()
        .map(|entry| entry["id"].as_str().unwrap())
        .collect()
}

fn layer<'a>(report: &'a Value, name: &str) -> &'a Value {
    report["layers"]
        .as_array()
        .unwrap()
        .iter()
        .find(|layer| layer["name"] == name)
        .unwrap()
}

fn shared_baseline(host: &str) {
    let machine = Machine::new(host, ORGANIZATION, Some(USER));
    let files = [("src/a.txt", "a\n")];
    let inherits = Repo::new(&format!("layers-inherit-{host}"), &files);
    let differs = Repo::new(
        &format!("layers-differ-{host}"),
        &[(".craft/project.json", PROJECT), ("src/a.txt", "a\n")],
    );

    // A repository with no configuration of its own has the whole baseline.
    let config = machine.json(&inherits, &["config", "--explain"]);
    assert_eq!(
        config["order"],
        json!(["bundled", "organization", "user", "project"])
    );
    assert_eq!(layer(&config, "organization")["state"], "loaded");
    assert_eq!(layer(&config, "user")["state"], "loaded");
    assert_eq!(layer(&config, "project")["state"], "absent");
    assert_eq!(
        row(&config, "values", "standards[commits]")["origin"],
        "organization"
    );
    assert_eq!(
        row(&config, "values", "routing.debtMarkers")["origin"],
        "organization"
    );
    // The user's own switch wins over the organization's and says what it hid.
    let summary = row(&config, "overrides", "routing.sessionSummary");
    assert_eq!(
        (&summary["origin"], &summary["value"]),
        (&json!("user"), &json!(true))
    );
    assert_eq!(
        summary["overrides"][0],
        json!({"origin": "organization", "value": false})
    );
    let settings = machine.json(&inherits, &["settings"]);
    assert_eq!(
        ids(&settings["settings"]["standards"]),
        ["commits", "review", "licenses", "audit"]
    );
    assert_eq!(settings["origins"]["efficiency.review.mode"], "user");
    assert_eq!(settings["safety"]["origin"], "organization");

    // The other repository overrides only what differs; each override names the layer it hid,
    // and the one that changes nothing is said.
    let config = machine.json(&differs, &["config"]);
    assert_eq!(layer(&config, "project")["state"], "loaded");
    let markers = row(&config, "overrides", "routing.debtMarkers");
    assert_eq!(
        (&markers["origin"], &markers["value"]),
        (&json!("project"), &json!(false))
    );
    assert_eq!(markers["overrides"][0]["origin"], "organization");
    let review = row(&config, "overrides", "standards[review]");
    assert_eq!(
        review["value"]["checklist"][0],
        "Tests and a changelog line"
    );
    assert_eq!(
        review["overrides"][0]["value"]["checklist"][0],
        "Tests cover the change"
    );
    let licenses = row(&config, "overrides", "standards[licenses]");
    assert_eq!(licenses["disabled"], true);
    assert_eq!(licenses["overrides"][0]["origin"], "organization");
    assert_eq!(
        row(&config, "overrides", "standards[legacy]")["disablesNothing"],
        true
    );
    let settings = machine.json(&differs, &["settings"]);
    assert_eq!(
        ids(&settings["settings"]["standards"]),
        ["commits", "review", "audit"]
    );
    assert_eq!(
        settings["entries"]["standards"],
        json!({"commits": "organization", "review": "project", "audit": "organization"})
    );
    let only = machine.json(&differs, &["config", "--explain", "--field", "safety"]);
    assert!(
        only["values"]
            .as_array()
            .unwrap()
            .iter()
            .all(|row| row["field"].as_str().unwrap().starts_with("safety")),
        "{only}"
    );

    // The organization's check runs by `craft check` at its stage where it is inherited, and
    // is not run where the project turned it off; the CI check is left to CI in both.
    let checked = machine.json(&inherits, &["check", "--file", "src/a.txt"]);
    assert_eq!(checked["result"], "passed", "{checked}");
    assert_eq!(ids(&checked["checks"]), ["licenses"]);
    assert_eq!(ids(&checked["left"]), ["audit"]);
    let checked = machine.json(&differs, &["check", "--file", "src/a.txt"]);
    assert!(ids(&checked["checks"]).is_empty(), "{checked}");
    assert_eq!(ids(&checked["left"]), ["audit"]);

    // The hooks of both repositories act on the same baseline.
    for repo in [&inherits, &differs] {
        machine.hook(host, repo, "session-start", json!({"source": "startup"}));
        let refused = machine.hook(
            host,
            repo,
            "before-tool",
            json!({"hook_event_name": "PreToolUse", "tool_name": "Bash",
                "tool_input": {"command": "terraform destroy -auto-approve"}}),
        );
        assert!(refused.starts_with("deny "), "{refused}");
        assert!(
            refused.contains("rule `infra-destroy`: tearing down shared infrastructure"),
            "{refused}"
        );
        repo.write("src/a.txt", "b\n");
        repo.git(&["commit", "-qam", "Changed a"]);
        let committed = machine.hook(
            host,
            repo,
            "after-tool",
            shell(
                host,
                "git commit -am 'Changed a'",
                "[main 1a2b3c] Changed a",
            ),
        );
        assert!(committed.contains("conventional-commits"), "{committed}");
    }
}

#[test]
fn claude_code_shares_one_baseline_across_two_repositories() {
    shared_baseline("claude");
}

#[test]
fn codex_shares_one_baseline_across_two_repositories() {
    shared_baseline("codex");
}

#[test]
fn a_layer_that_cannot_be_used_is_an_error_naming_its_file() {
    let repo = Repo::new("layers-broken", &[("src/a.txt", "a\n")]);
    let failure = |machine: &Machine, args: &[&str]| {
        let mut argv = vec![args[0], "--root", repo.path()];
        argv.extend(&args[1..]);
        let output = machine.craft(&argv, "");
        assert!(!output.status.success(), "{args:?} succeeded");
        String::from_utf8_lossy(&output.stderr).into_owned()
    };
    let named = |machine: &Machine, error: &str, reason: &str| {
        let file = machine.organization();
        assert!(
            error.contains(&format!("organization layer {}", file.display()))
                && error.contains(reason),
            "{error}"
        );
    };
    let truncated = Machine::new("truncated", r#"{"routing": "#, None);
    for command in ["config", "settings", "check"] {
        named(
            &truncated,
            &failure(&truncated, &[command]),
            "EOF while parsing",
        );
    }
    // A hook says so and lets the host continue: no partial configuration is applied quietly.
    let said = truncated.hook(
        "claude",
        &repo,
        "session-start",
        json!({"source": "startup"}),
    );
    assert!(said.contains("unavailable: organization layer"), "{said}");
    // The review package's shell guard cannot evaluate its policy and refuses the command.
    let payload = json!({"session_id": "S-guard", "cwd": repo.path(),
        "hook_event_name": "PreToolUse", "tool_name": "Bash", "tool_input": {"command": "ls"}});
    let guard = [
        "hook",
        "claude",
        "before-tool",
        "--package",
        "craft-review",
        "--handler",
        "review-read-only",
        "--scope",
        "shell",
    ];
    let output = truncated.craft(&guard, &payload.to_string());
    let refused = String::from_utf8_lossy(&output.stderr);
    assert_eq!(output.status.code(), Some(2), "{refused}");
    assert!(
        refused.contains("Policy could not be evaluated: organization layer"),
        "{refused}"
    );
    let project_only = Machine::new("project-only", r#"{"modules": []}"#, None);
    let error = failure(&project_only, &["config"]);
    named(&project_only, &error, "unknown layer field: modules");
    let user = Machine::new(
        "user",
        "{}",
        Some(r#"{"standards": [{"id": "a", "file": "a.md"}]}"#),
    );
    let error = failure(&user, &["config"]);
    assert!(
        error.contains("user layer ")
            && error.contains("preferences.json")
            && error.contains("file names a project file"),
        "{error}"
    );
    // A project file that cannot be used is named the same way.
    repo.write(
        ".craft/project.json",
        r#"{"formatVersion": 1, "routing": {"other": true}}"#,
    );
    let fine = Machine::new("fine", "{}", None);
    let error = failure(&fine, &["config"]);
    assert!(
        error.contains("project.json: ") && error.contains("unknown routing field"),
        "{error}"
    );
}

#[cfg(unix)]
#[test]
fn an_organization_file_others_can_write_is_refused() {
    use std::os::unix::fs::PermissionsExt;
    let repo = Repo::new("layers-writable", &[("src/a.txt", "a\n")]);
    let machine = Machine::new("writable", ORGANIZATION, None);
    fs::set_permissions(machine.organization(), fs::Permissions::from_mode(0o666)).unwrap();
    let output = machine.craft(&["config", "--root", repo.path()], "");
    assert!(!output.status.success());
    let error = String::from_utf8_lossy(&output.stderr);
    assert!(error.contains("not writable by others"), "{error}");
}

#[test]
fn without_layer_files_a_project_is_configured_alone() {
    let repo = Repo::new(
        "layers-none",
        &[(".craft/project.json", PROJECT), ("src/a.txt", "a\n")],
    );
    let machine = Machine::new("none", "{}", None);
    fs::remove_file(machine.organization()).unwrap();
    let config = machine.json(&repo, &["config"]);
    assert_eq!(layer(&config, "organization")["state"], "absent");
    assert_eq!(layer(&config, "user")["state"], "absent");
    // Entries that would remove a baseline standard are reported, not errors: the same
    // project file works on a machine without the baseline.
    let fields: Vec<&str> = config["overrides"]
        .as_array()
        .unwrap()
        .iter()
        .map(|row| row["field"].as_str().unwrap())
        .collect();
    assert_eq!(fields, ["standards[licenses]", "standards[legacy]"]);
    let settings = machine.json(&repo, &["settings"]);
    assert_eq!(ids(&settings["settings"]["standards"]), ["review"]);
    assert!(settings.get("entries").is_none(), "{settings}");
}
