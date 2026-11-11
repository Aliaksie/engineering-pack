//! One task through every standards and evidence feature, as each deep host reaches them: the
//! `craft` executable answers each hook from the host's own payload, on a machine with an
//! organization baseline and a user file shared by two repositories. The first repository
//! accepts the conventions its own files support, adds modules, a review checklist, an
//! advised file and its own review skill, and is routed, edited, checked, committed, tested,
//! compacted, delegated and stopped; a destructive command is refused on the way, `craft
//! config` names the layer behind each value, `craft doctor` reads the hook events the
//! session left and `craft check` runs the declared checks at their stage. The second
//! repository has no configuration and acts on the same baseline, with its own records. A
//! feature that stays silent, contradicts another or cannot be reached from the hooks fails
//! here.
mod support;
use serde_json::{Value, json};
use std::cell::Cell;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use support::{Repo, doctor, homes};

/// Git stands in for the organization's own tools: every test host has it.
const ORGANIZATION: &str = r#"{"formatVersion": 1,
  "routing": {"sessionSummary": false},
  "standards": [
    {"id": "review", "checklist": ["Tests cover the change"]},
    {"id": "licenses", "run": {"command": "git", "args": ["--version"]}},
    {"id": "audit", "run": {"command": "git", "args": ["--version"], "stage": "ci"}}],
  "safety": {"enabled": true, "deny": [
    {"id": "infra-destroy", "command": ["terraform", "destroy"], "reason": "tearing down shared infrastructure"}]}
}"#;
/// The user wants the line the organization turned off.
const USER: &str = r#"{"routing": {"sessionSummary": true}}"#;
const AGENTS: &str = "# Rules\n\nCommit messages follow Conventional Commits.\n";
const CHANGELOG: &str = "# Changelog\n\nThe format is based on [Keep a Changelog](https://keepachangelog.com).\n\n## [Unreleased]\n";
const SKILL: &str =
    "---\nname: team-review\ndescription: Review with the team checklist.\n---\nReview.\n";
const ZERO: &str = "ℹ tests 0\nℹ pass 0\nℹ fail 0\nℹ skipped 0\n";
const PASS: &str = "✔ refunds the order (1ms)\n✔ rejects a second refund (1ms)\nℹ tests 2\nℹ pass 2\nℹ fail 0\nℹ skipped 0\n";
/// The most one hook may add to a deep host's context (`hosts/hooks.json`).
const CEILING: usize = 9000;

/// The project file with `standards`: modules with their checks and the project's own
/// review skill are there from the start.
fn project(standards: Value) -> String {
    json!({"formatVersion": 1,
        "routing": {"debtMarkers": true},
        "modules": [
            {"name": "domain", "path": "src/domain", "dependsOn": [], "checks": [
                {"command": "git", "args": ["--version"], "id": "domain-build"},
                {"command": "git", "args": ["--version"], "id": "domain-slow", "stage": "ci"}]},
            {"name": "web", "path": "src/web", "dependsOn": ["domain"]}],
        "standards": standards,
        "skills": {"declared": [
            {"name": "team-review", "path": ".agents/skills/team-review",
             "replaces": ["craft-review:review-change"]}]}})
    .to_string()
}

/// The standards no file states: the module rings, a review checklist and an advised file.
fn own() -> Vec<Value> {
    vec![
        json!({"id": "rings", "anchor": "hexagonal-architecture"}),
        json!({"id": "api-review", "checklist": ["Errors map to status codes"], "scope": {"paths": ["src/web"]}}),
        json!({"id": "api-guide", "file": "docs/api.md", "scope": {"paths": ["src/web"]}}),
    ]
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

/// The host's view of the scenario: its payload shapes for edits and test results, on a
/// machine of its own with the layer files the scenario gives it.
struct Host {
    name: &'static str,
    session: &'static str,
    machine: PathBuf,
    /// The largest context one hook added, and all of it for this host's own session.
    largest: Cell<usize>,
    injected: Cell<usize>,
}

impl Drop for Host {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.machine);
    }
}

impl Host {
    /// A machine with the `(organization, user)` files, or with neither.
    fn new(name: &'static str, session: &'static str, layers: Option<(&str, &str)>) -> Self {
        let machine =
            std::env::temp_dir().join(format!("craft-machine-{session}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&machine);
        let settings = machine.join("home/.config/craft");
        fs::create_dir_all(&settings).unwrap();
        fs::create_dir_all(machine.join("managed")).unwrap();
        private(&settings);
        private(&machine.join("managed"));
        if let Some((organization, user)) = layers {
            for (file, text) in [
                (machine.join("managed/organization.json"), organization),
                (settings.join("preferences.json"), user),
            ] {
                fs::write(&file, text).unwrap();
                private(&file);
            }
        }
        Self {
            name,
            session,
            machine,
            largest: Cell::new(0),
            injected: Cell::new(0),
        }
    }

    /// `craft` as it runs on this machine.
    fn craft(&self) -> Command {
        let home = self.machine.join("home");
        let mut command = Command::new(env!("CARGO_BIN_EXE_craft"));
        command
            .env(
                "CRAFT_ORGANIZATION_CONFIG",
                self.machine.join("managed/organization.json"),
            )
            .env("HOME", &home)
            .env("USERPROFILE", &home)
            .env("CRAFT_EVIDENCE_WARM", "0")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        command
    }

    /// One public CLI command with structured output; `Err` carries its diagnostic.
    fn cli(&self, repo: &Repo, args: &[&str]) -> Result<Value, String> {
        let output = self
            .craft()
            .args([args[0], "--root", repo.path(), "--json"])
            .args(&args[1..])
            .output()
            .unwrap();
        let stderr = String::from_utf8_lossy(&output.stderr)
            .trim_end()
            .to_string();
        if !output.status.success() {
            return Err(stderr);
        }
        Ok(serde_json::from_slice(&output.stdout).unwrap_or_else(|_| panic!("{args:?}: {stderr}")))
    }

    /// `craft hook <host> <lifecycle>` as the host runs it for this host's own session.
    fn hook(&self, repo: &Repo, lifecycle: &str, extra: Value) -> String {
        let before = self.injected.get();
        let said = self.hook_in(self.session, repo, lifecycle, extra);
        if !said.starts_with("deny ") && !said.starts_with('{') {
            self.injected.set(before + said.len());
        }
        said
    }

    /// The text the host is given in `session`: `deny <reason>` for a refused tool call, the
    /// context or the reason a hook adds, else its output as written.
    fn hook_in(&self, session: &str, repo: &Repo, lifecycle: &str, extra: Value) -> String {
        let mut payload = json!({"session_id": session, "cwd": repo.path()});
        for (key, value) in extra.as_object().unwrap() {
            payload[key] = value.clone();
        }
        let mut child = self
            .craft()
            .args(["hook", self.name, lifecycle])
            .args(["--package", "craft-delivery", "--handler", "deliver"])
            .spawn()
            .unwrap();
        use std::io::Write;
        child
            .stdin
            .take()
            .unwrap()
            .write_all(payload.to_string().as_bytes())
            .unwrap();
        let output = child.wait_with_output().unwrap();
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if output.status.code() == Some(2) {
            return format!("deny {stderr}");
        }
        assert!(output.status.success(), "{lifecycle}: {stderr}");
        assert!(stderr.is_empty(), "{lifecycle} warned: {stderr}");
        if stdout.is_empty() {
            return stdout;
        }
        let wire: Value = serde_json::from_str(&stdout).unwrap();
        if stdout.contains(r#""deny""#) {
            return format!("deny {stdout}");
        }
        let said = match wire["hookSpecificOutput"]["additionalContext"].as_str() {
            Some(context) => context.to_string(),
            None => wire["reason"].as_str().unwrap_or(&stdout).to_string(),
        };
        self.largest.set(self.largest.get().max(said.len()));
        said
    }

    /// An edit tool call writing `files`: Claude names one file per call, Codex sends one patch
    /// naming each file by its absolute path, as Codex writes them.
    fn edit(&self, repo: &Repo, files: &[&str]) -> Vec<Value> {
        match self.name {
            "codex" => {
                let headers: String = files
                    .iter()
                    .map(|f| format!("*** Update File: {}/{f}\n@@\n-old\n+new\n", repo.path()))
                    .collect();
                vec![json!({"tool_name": "apply_patch",
                    "tool_input": {"command": format!("*** Begin Patch\n{headers}*** End Patch")}})]
            }
            _ => files
                .iter()
                .map(|f| json!({"tool_name": "Edit", "tool_input": {"file_path": f}}))
                .collect(),
        }
    }

    /// A shell call before it runs.
    fn run(&self, command: &str) -> Value {
        json!({"tool_name": "Bash", "tool_input": {"command": command}})
    }

    /// The tool result of a shell command with `output`: Claude passes stdout, Codex the text.
    fn shell(&self, command: &str, output: &str) -> Value {
        let response = match self.name {
            "codex" => json!(output),
            _ => json!({"stdout": output, "stderr": ""}),
        };
        json!({"tool_name": "Bash", "tool_input": {"command": command}, "tool_response": response})
    }

    fn after_edit(&self, repo: &Repo, files: &[&str]) -> String {
        self.edit(repo, files)
            .into_iter()
            .map(|call| self.hook(repo, "after-tool", call))
            .filter(|said| !said.is_empty())
            .collect::<Vec<_>>()
            .join("\n")
    }
}

fn git(repo: &Repo, args: &[&str]) {
    let output = Command::new("git")
        .arg("-C")
        .arg(repo.path())
        .args(["-c", "user.name=t", "-c", "user.email=t@t"])
        .args([
            "-c",
            "commit.gpgsign=false",
            "-c",
            "core.hooksPath=/dev/null",
        ])
        .args(args)
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
}

fn verify(host: &Host, repo: &Repo) -> Result<Value, String> {
    let bundle = support::bundle();
    host.cli(
        repo,
        &[
            "task-step",
            "--session",
            host.session,
            "--bundle",
            &bundle,
            "--stage",
            "verify",
            "--target",
            "acceptance:refund",
            "--approach",
            "unit-test",
            "--evidence",
            "node --test",
            "--outcome",
            "passed",
            "--result",
            "Refund tests pass",
            "--next",
            "Finish",
        ],
    )
}

/// The JSON object that follows `marker` in a hook's context text.
fn data_after(context: &str, marker: &str) -> Value {
    let start = context
        .find(marker)
        .unwrap_or_else(|| panic!("{marker} in {context}"));
    let rest = &context[start + marker.len()..];
    let open = rest.find(['{', '[']).unwrap();
    serde_json::Deserializer::from_str(&rest[open..])
        .into_iter::<Value>()
        .next()
        .unwrap()
        .unwrap()
}

fn ids(list: &Value) -> Vec<&str> {
    list.as_array()
        .unwrap_or_else(|| panic!("list: {list}"))
        .iter()
        .map(|s| s["id"].as_str().unwrap())
        .collect()
}

/// The row of `field` in a `craft config` list.
fn row<'a>(report: &'a Value, list: &str, field: &str) -> &'a Value {
    report[list]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["field"] == field)
        .unwrap_or_else(|| panic!("{field} in {report}"))
}

fn layer<'a>(report: &'a Value, name: &str) -> &'a Value {
    report["layers"]
        .as_array()
        .unwrap()
        .iter()
        .find(|layer| layer["name"] == name)
        .unwrap()
}

fn scenario(host: Host) {
    let repo = Repo::new(
        &format!("scenario-{}", host.name),
        &[
            (".craft/project.json", &project(json!([]))),
            (".agents/skills/team-review/SKILL.md", SKILL),
            (".claude/skills/team-review/SKILL.md", SKILL),
            ("AGENTS.md", AGENTS),
            ("docs/api.md", "# API\n\nErrors map to status codes.\n"),
            ("src/domain/order.ts", "export class Order {}\n"),
            ("src/web/http.ts", "export const http = 1\n"),
            ("CHANGELOG.md", CHANGELOG),
        ],
    );
    // Craft installed for this project before the session, as doctor will read it.
    let installs = homes(&repo);
    let root = Path::new(repo.path());
    let session = host.session;

    // Session start names the session for task commands and, once, the conventions the
    // project's own files support: the organization's standards are not its declarations.
    let started = host.hook(&repo, "session-start", json!({"source": "startup"}));
    assert!(
        started.contains(&format!("\"id\":\"{session}\"")),
        "{started}"
    );
    let offer = data_after(&started, "Craft conventions scan");
    assert_eq!(
        offer["standards"],
        json!(["commits", "changelog"]),
        "{started}"
    );

    // The consumer accepts what was proposed; with the project's own standards it is the
    // configuration the rest of the session is held to.
    let proposed = host.cli(&repo, &["standards", "--propose"]).unwrap();
    let mut standards: Vec<Value> = proposed["proposals"]
        .as_array()
        .unwrap()
        .iter()
        .map(|proposal| proposal["standard"].clone())
        .collect();
    assert_eq!(ids(&json!(standards)), ["commits", "changelog"]);
    standards.extend(own());
    repo.write(".craft/project.json", &project(json!(standards)));
    let again = host.cli(&repo, &["standards", "--propose"]).unwrap();
    assert_eq!(again["proposals"], json!([]), "{again}");
    let restarted = host.hook(&repo, "session-start", json!({"source": "startup"}));
    assert!(!restarted.contains("conventions scan"), "{restarted}");

    // Every value has its layer: the accepted standards are the project's, the review
    // checklist the organization's, and the user's switch wins over the organization's.
    let config = host.cli(&repo, &["config", "--explain"]).unwrap();
    assert_eq!(
        config["order"],
        json!(["bundled", "organization", "user", "project"])
    );
    for name in ["organization", "user", "project"] {
        assert_eq!(layer(&config, name)["state"], "loaded", "{name}");
    }
    assert_eq!(
        row(&config, "values", "standards[commits]")["origin"],
        "project"
    );
    assert_eq!(
        row(&config, "values", "standards[review]")["origin"],
        "organization"
    );
    let summary = row(&config, "overrides", "routing.sessionSummary");
    assert_eq!(
        (&summary["origin"], &summary["value"]),
        (&json!("user"), &json!(true))
    );
    assert_eq!(
        summary["overrides"][0],
        json!({"origin": "organization", "value": false})
    );

    // A review request elsewhere routes the project's own review skill in place of Craft's.
    let hint = host.hook_in(
        "review",
        &repo,
        "before-prompt",
        json!({"prompt": "Review the change in src/web/http.ts"}),
    );
    let route = data_after(&hint, "Routing hints");
    assert_eq!(route["intent"], "review", "{hint}");
    assert_eq!(route["skills"][0], "team-review", "{hint}");
    assert!(!hint.contains("review-change"), "{hint}");
    assert_eq!(
        ids(&route["standards"]),
        [
            "review",
            "licenses",
            "audit",
            "commits",
            "changelog",
            "rings"
        ],
        "the baseline first, then the project's: {hint}"
    );

    // Work the host does alone gets nothing: a chore at the prompt, a change request, which
    // names no skills there, and a shell command no rule concerns.
    for prompt in [
        "Fix the typo in the README",
        "Add a refund endpoint to the web module",
    ] {
        let asked = host.hook(&repo, "before-prompt", json!({"prompt": prompt}));
        assert_eq!(asked, "", "{prompt}");
    }
    assert_eq!(host.hook(&repo, "before-tool", host.run("git status")), "");
    assert_eq!(
        host.hook(&repo, "after-tool", host.shell("git status", "clean")),
        ""
    );
    host.cli(
        &repo,
        &[
            "task-start",
            "--session",
            session,
            "--task",
            "refund",
            "--goal",
            "Refund endpoint",
            "--file",
            "src/web/refund.ts",
        ],
    )
    .unwrap();

    // The first edit in the web module carries every standard of its scope.
    let call = host.edit(&repo, &["src/web/refund.ts"]).remove(0);
    let hint = host.hook(&repo, "before-tool", call);
    let route = data_after(&hint, "Routing hints");
    assert_eq!(route["intent"], "implement", "{hint}");
    assert_eq!(route["modules"][0]["name"], "web", "{hint}");
    let scoped = [
        "review",
        "licenses",
        "audit",
        "commits",
        "changelog",
        "rings",
        "api-review",
        "api-guide",
    ];
    assert_eq!(ids(&route["standards"]), scoped);
    repo.write(
        "src/web/refund.ts",
        "import { Order } from '../domain/order'\nexport const refund = (o: Order) => o\n",
    );
    assert_eq!(
        host.after_edit(&repo, &["src/web/refund.ts"]),
        "",
        "an allowed import and no anchor in scope: quiet"
    );

    // Edits that break the rings and the accepted changelog format are reported in the tool
    // result.
    repo.write(
        "src/domain/order.ts",
        "import { refund } from '../web/refund'\nexport class Order {}\n",
    );
    repo.write("CHANGELOG.md", "# Changelog\n\n## Next\n- refunds\n");
    let found = host.after_edit(&repo, &["src/domain/order.ts", "CHANGELOG.md"]);
    assert!(found.contains("undeclared-dependency"), "{found}");
    assert!(found.contains("keep-a-changelog"), "{found}");
    repo.write("src/domain/order.ts", "export class Order {}\n");
    repo.write(
        "CHANGELOG.md",
        &format!("{CHANGELOG}\n### Added\n- Refunds.\n"),
    );
    assert_eq!(
        host.after_edit(&repo, &["src/domain/order.ts", "CHANGELOG.md"]),
        "",
        "fixed: quiet"
    );

    // The commit message is checked against the accepted commit convention.
    git(&repo, &["add", "-A"]);
    git(&repo, &["commit", "-qm", "Added refunds"]);
    let committed = host.hook(
        &repo,
        "after-tool",
        host.shell(
            "git commit -am 'Added refunds'",
            "[main 1a2b3c] Added refunds",
        ),
    );
    assert!(committed.contains("conventional-commits"), "{committed}");

    // The organization's safety policy refuses a destructive command before it runs.
    let refused = host.hook(
        &repo,
        "before-tool",
        host.run("git push --force origin main"),
    );
    assert!(refused.starts_with("deny "), "{refused}");
    assert!(refused.contains("rule `force-push`"), "{refused}");

    // A test run that executed nothing is said at once and cannot back a verified pass.
    let zero = host.hook(
        &repo,
        "after-tool",
        host.shell("node --test --test-name-pattern=refund", ZERO),
    );
    assert!(zero.contains("executed no test"), "{zero}");
    let refused = verify(&host, &repo).unwrap_err();
    assert!(refused.contains("contradicts the test run"), "{refused}");
    let passed = host.hook(&repo, "after-tool", host.shell("node --test", PASS));
    assert_eq!(passed, "", "a passing run is quiet");
    verify(&host, &repo).unwrap();

    // After compaction the task returns with its checked and reviewed standards and its tests.
    host.hook(&repo, "before-compact", json!({"trigger": "auto"}));
    let resumed = host.hook(&repo, "after-compact", json!({"source": "compact"}));
    let resume = data_after(&resumed, "Craft task records to resume from");
    assert_eq!(resume["task"], "refund", "{resumed}");
    assert_eq!(
        ids(&resume["standards"]),
        scoped[..7],
        "advise-level guidance returns with routing, not with the resume"
    );
    assert_eq!(resume["tests"]["status"], "passed", "{resumed}");
    assert_eq!(resume["tests"]["executed"], 2, "{resumed}");

    // A subagent the host starts hears the standards of the work it was delegated from.
    let delegated = host.hook(&repo, "agent-start", json!({"agent_type": "default"}));
    let delegation = data_after(&delegated, "Project standards of the work");
    assert_eq!(delegation["intent"], "implement", "{delegated}");
    assert_eq!(ids(&delegation["standards"]), scoped);

    // The turn ends with one review of what it changed, said once.
    repo.write(
        "src/web/refund.ts",
        "import { Order } from '../domain/order'\n// TODO: partial refunds\nexport const refund = (o: Order) => o\n",
    );
    host.after_edit(&repo, &["src/web/refund.ts"]);
    let stop = host.hook(&repo, "turn-end", json!({"stop_hook_active": false}));
    let summary = data_after(&stop, "fix it, or tell the user why it stays:");
    assert_eq!(summary["findings"][0]["rule"], "todo-marker", "{stop}");
    assert_eq!(
        summary["findings"][0]["file"], "src/web/refund.ts",
        "{stop}"
    );
    assert_eq!(ids(&summary["review"]), ["review", "api-review"], "{stop}");
    // The same findings never hold a stop twice: the turn ends, and the user reads one line
    // whose counts are what happened above (rings and changelog raised and fixed, the commit
    // message and the marker raised, one empty test run, one resume after compaction, one
    // command refused). The line is there because the user's file asked for it.
    let ended = host.hook(&repo, "turn-end", json!({"stop_hook_active": false}));
    let ended: Value = serde_json::from_str(&ended).unwrap_or_else(|_| panic!("{ended}"));
    assert_eq!(
        ended,
        json!({"systemMessage": "Craft this session: 4 standards findings raised, 2 fixed · 1 test run that ran no test caught · 1 task context restored · 1 command refused by the safety policy"})
    );
    assert_eq!(
        host.hook(&repo, "turn-end", json!({"stop_hook_active": false})),
        "",
        "an unchanged summary is not shown again"
    );

    // What the hooks added to the model's context is on record, byte for byte, and no hook
    // passed the host's ceiling.
    let status = host
        .cli(
            &repo,
            &["session", "--session", session, "--bundle", repo.path()],
        )
        .unwrap();
    let delivered = &status["delivered"];
    assert_eq!(
        delivered["contextBytes"],
        host.injected.get(),
        "{delivered}"
    );
    assert_eq!(delivered["droppedPieces"], 0, "{delivered}");
    assert!(host.largest.get() <= CEILING, "{}", host.largest.get());

    // Doctor reads the install and the last hook this session ran, and changes nothing.
    let (code, report) = doctor(&repo, &installs);
    assert_eq!(code, 0, "{report}");
    let seen = report["hosts"]
        .as_array()
        .unwrap()
        .iter()
        .find(|h| h["host"] == host.name)
        .unwrap();
    let hooks = seen.to_string().contains("last hook turn-end at");
    assert!(hooks, "{seen}");
    assert!(root.join(".craft/project.json").exists());

    // The module's declared checks and the organization's run at the commit stage; those
    // declared for CI are left to it, and the run joins the session's records.
    let checked = host
        .cli(
            &repo,
            &[
                "check",
                "--file",
                "src/domain/order.ts",
                "--session",
                session,
            ],
        )
        .unwrap();
    assert_eq!(checked["result"], "passed", "{checked}");
    assert_eq!(ids(&checked["checks"]), ["domain-build", "licenses"]);
    assert_eq!(ids(&checked["left"]), ["domain-slow", "audit"]);
    assert_eq!(checked["left"][0]["result"], "left-to-ci");
    assert_eq!(checked["recorded"], true);

    second_repository(&host);
}

/// A repository with no configuration on the same machine: it has the whole baseline in its
/// commands and hooks, and its session's records are its own.
fn second_repository(host: &Host) {
    let repo = Repo::new(
        &format!("scenario-second-{}", host.name),
        &[("src/a.ts", "export const a = 1\n")],
    );
    let session = "second";
    let config = host.cli(&repo, &["config"]).unwrap();
    assert_eq!(layer(&config, "organization")["state"], "loaded");
    assert_eq!(layer(&config, "project")["state"], "absent");
    let settings = host.cli(&repo, &["settings"]).unwrap();
    assert_eq!(
        ids(&settings["settings"]["standards"]),
        ["review", "licenses", "audit"]
    );

    let started = host.hook_in(
        session,
        &repo,
        "session-start",
        json!({"source": "startup"}),
    );
    assert!(!started.contains("conventions scan"), "{started}");
    let call = host.edit(&repo, &["src/a.ts"]).remove(0);
    let hint = host.hook_in(session, &repo, "before-tool", call);
    let route = data_after(&hint, "Routing hints");
    assert_eq!(ids(&route["standards"]), ["review", "licenses", "audit"]);
    let refused = host.hook_in(
        session,
        &repo,
        "before-tool",
        host.run("terraform destroy -auto-approve"),
    );
    assert!(refused.starts_with("deny "), "{refused}");
    assert!(
        refused.contains("rule `infra-destroy`: tearing down shared infrastructure"),
        "{refused}"
    );

    let checked = host
        .cli(
            &repo,
            &["check", "--file", "src/a.ts", "--session", session],
        )
        .unwrap();
    assert_eq!(checked["result"], "passed", "{checked}");
    assert_eq!(ids(&checked["checks"]), ["licenses"]);
    assert_eq!(ids(&checked["left"]), ["audit"]);

    // Nothing of the first repository's session is counted here.
    let ended = host.hook_in(
        session,
        &repo,
        "turn-end",
        json!({"stop_hook_active": false}),
    );
    let ended: Value = serde_json::from_str(&ended).unwrap_or_else(|_| panic!("{ended}"));
    assert_eq!(
        ended,
        json!({"systemMessage": "Craft this session: 1 command refused by the safety policy"})
    );
}

/// The same work as a live session does it: no Craft task is ever bound. One edit writes a
/// file in each module; compaction must still restate the standards of what was edited and
/// the tests the session ran.
fn untasked(host: Host) {
    let mut declared = own();
    declared.splice(
        1..1,
        [
            json!({"id": "changelog", "anchor": "keep-a-changelog"}),
            json!({"id": "commits", "anchor": "conventional-commits"}),
        ],
    );
    let repo = Repo::new(
        &format!("untasked-{}", host.name),
        &[
            (".craft/project.json", &project(json!(declared))),
            ("docs/api.md", "# API\n\nErrors map to status codes.\n"),
            ("src/domain/order.ts", "export class Order {}\n"),
            ("src/web/http.ts", "export const http = 1\n"),
            ("CHANGELOG.md", "# Changelog\n\n## [Unreleased]\n"),
        ],
    );
    host.hook(&repo, "session-start", json!({"source": "startup"}));
    host.hook(
        &repo,
        "before-prompt",
        json!({"prompt": "Add order refunds with a refund handler in the web module"}),
    );

    // Every file the edit writes is routed: both modules, whichever file a patch names first.
    let files = ["src/domain/order.ts", "src/web/refund.ts"];
    let before = |repo: &Repo| -> Vec<String> {
        host.edit(repo, &files)
            .into_iter()
            .map(|call| host.hook(repo, "before-tool", call))
            .filter(|said| !said.is_empty())
            .collect()
    };
    let hints = before(&repo);
    let mut modules: Vec<String> = hints
        .iter()
        .flat_map(|hint| {
            let route = data_after(hint, "Routing hints");
            route["modules"]
                .as_array()
                .unwrap_or_else(|| panic!("modules in {hint}"))
                .iter()
                .map(|m| m["name"].as_str().unwrap().to_string())
                .collect::<Vec<_>>()
        })
        .collect();
    modules.sort();
    modules.dedup();
    assert_eq!(modules, ["domain", "web"], "{hints:?}");
    assert!(
        before(&repo).is_empty(),
        "routed files are not hinted again"
    );
    if host.name == "codex" {
        let patch = format!(
            "*** Begin Patch\n*** Update File: /elsewhere/notes.md\n@@\n-a\n+b\n*** Update File: {}/src/web/http.ts\n@@\n-a\n+b\n*** End Patch",
            repo.path()
        );
        let call = json!({"tool_name": "apply_patch", "tool_input": {"command": patch}});
        let hint = host.hook(&repo, "before-tool", call);
        let route = data_after(&hint, "Routing hints");
        assert_eq!(route["modules"][0]["name"], "web", "{hint}");
    }

    repo.write(
        "src/domain/order.ts",
        "export class Order { refunded = false }\n",
    );
    repo.write(
        "src/web/refund.ts",
        "import { Order } from '../domain/order'\nexport const refund = (o: Order) => o\n",
    );
    host.after_edit(&repo, &files);
    host.hook(&repo, "after-tool", host.shell("node --test", PASS));

    // Compaction without a task restores the checkpoint with the standards and the tests.
    host.hook(&repo, "before-compact", json!({"trigger": "auto"}));
    let resumed = host.hook(&repo, "after-compact", json!({"source": "compact"}));
    assert!(resumed.contains("still in force"), "{resumed}");
    let checkpoint = data_after(&resumed, "Checkpoint data");
    assert_eq!(
        ids(&checkpoint["standards"]),
        ["rings", "changelog", "commits", "api-review"],
        "the web file's review standard returns: the patch's second file joined the scope"
    );
    assert_eq!(checkpoint["tests"]["status"], "passed", "{resumed}");
    assert_eq!(checkpoint["tests"]["executed"], 2, "{resumed}");
}

#[test]
fn claude_code_keeps_standards_and_tests_without_a_task() {
    untasked(Host::new("claude", "claude-untasked", None));
}

#[test]
fn codex_keeps_standards_and_tests_without_a_task() {
    untasked(Host::new("codex", "codex-untasked", None));
}

#[test]
fn claude_code_reaches_every_feature_in_one_task() {
    scenario(Host::new(
        "claude",
        "claude-session",
        Some((ORGANIZATION, USER)),
    ));
}

#[test]
fn codex_reaches_every_feature_in_one_task() {
    scenario(Host::new(
        "codex",
        "codex-session",
        Some((ORGANIZATION, USER)),
    ));
}
