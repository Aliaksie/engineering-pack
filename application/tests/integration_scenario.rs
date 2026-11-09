//! One task through every standards and evidence feature, as each deep host reaches them: the
//! `craft` executable answers each hook from the host's own payload. A project with declared
//! modules, anchors, a review checklist, an advised file and its own review skill is routed,
//! edited, checked, committed, tested, compacted, delegated and stopped; `craft doctor` then
//! reads the hook events the session left. A feature that stays silent, contradicts another or
//! cannot be reached from the hooks fails here.
mod support;
use serde_json::{Value, json};
use std::path::Path;
use std::process::Command;
use support::{Repo, doctor, homes};

const PROJECT: &str = r#"{"formatVersion": 1,
  "routing": {"debtMarkers": true},
  "modules": [
    {"name": "domain", "path": "src/domain", "dependsOn": []},
    {"name": "web", "path": "src/web", "dependsOn": ["domain"]}],
  "standards": [
    {"id": "rings", "anchor": "hexagonal-architecture"},
    {"id": "changelog", "anchor": "keep-a-changelog"},
    {"id": "commits", "anchor": "conventional-commits"},
    {"id": "api-review", "checklist": ["Errors map to status codes"], "scope": {"paths": ["src/web"]}},
    {"id": "api-guide", "file": "docs/api.md", "scope": {"paths": ["src/web"]}}],
  "skills": {"declared": [
    {"name": "team-review", "path": ".agents/skills/team-review", "replaces": ["craft-review:review-change"]}]}
}"#;
const SKILL: &str =
    "---\nname: team-review\ndescription: Review with the team checklist.\n---\nReview.\n";
const ZERO: &str = "ℹ tests 0\nℹ pass 0\nℹ fail 0\nℹ skipped 0\n";
const PASS: &str = "✔ refunds the order (1ms)\n✔ rejects a second refund (1ms)\nℹ tests 2\nℹ pass 2\nℹ fail 0\nℹ skipped 0\n";

/// The host's view of the scenario: its payload shapes for edits and test results.
struct Host {
    name: &'static str,
    session: &'static str,
}

impl Host {
    /// `craft hook <host> <lifecycle>` as the host runs it; the text the host is given.
    fn hook(&self, repo: &Repo, lifecycle: &str, extra: Value) -> String {
        let mut payload = json!({"session_id": self.session, "cwd": repo.path()});
        for (key, value) in extra.as_object().unwrap() {
            payload[key] = value.clone();
        }
        let mut child = Command::new(env!("CARGO_BIN_EXE_craft"))
            .args(["hook", self.name, lifecycle])
            .args(["--package", "craft-delivery", "--handler", "deliver"])
            .env("CRAFT_EVIDENCE_WARM", "0")
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
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
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(output.status.success(), "{lifecycle}: {stderr}");
        assert!(stderr.is_empty(), "{lifecycle} warned: {stderr}");
        let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if stdout.is_empty() {
            return stdout;
        }
        let wire: Value = serde_json::from_str(&stdout).unwrap();
        match wire["hookSpecificOutput"]["additionalContext"].as_str() {
            Some(context) => context.to_string(),
            None => wire["reason"].as_str().unwrap_or(&stdout).to_string(),
        }
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

fn verify(repo: &Repo, session: &str) -> Result<Value, String> {
    let bundle = support::bundle();
    repo.cli(&[
        "task-step",
        "--session",
        session,
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
    ])
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

fn ids(standards: &Value) -> Vec<&str> {
    standards
        .as_array()
        .unwrap_or_else(|| panic!("standards: {standards}"))
        .iter()
        .map(|s| s["id"].as_str().unwrap())
        .collect()
}

fn scenario(host: Host) {
    let repo = Repo::new(
        &format!("scenario-{}", host.name),
        &[
            (".craft/project.json", PROJECT),
            (".agents/skills/team-review/SKILL.md", SKILL),
            (".claude/skills/team-review/SKILL.md", SKILL),
            ("docs/api.md", "# API\n\nErrors map to status codes.\n"),
            ("src/domain/order.ts", "export class Order {}\n"),
            ("src/web/http.ts", "export const http = 1\n"),
            ("CHANGELOG.md", "# Changelog\n\n## [Unreleased]\n"),
        ],
    );
    // Craft installed for this project before the session, as doctor will read it.
    let installs = homes(&repo);
    let root = Path::new(repo.path());
    let session = host.session;

    // Session start names the session for task commands.
    let started = host.hook(&repo, "session-start", json!({"source": "startup"}));
    assert!(
        started.contains(&format!("\"id\":\"{session}\"")),
        "{started}"
    );

    // A review request elsewhere routes the project's own review skill in place of Craft's.
    let review = Host {
        name: host.name,
        session: "review",
    };
    let hint = review.hook(
        &repo,
        "before-prompt",
        json!({"prompt": "Review the change in src/web/http.ts"}),
    );
    let route = data_after(&hint, "Routing hints");
    assert_eq!(route["intent"], "review", "{hint}");
    assert_eq!(route["skills"][0], "team-review", "{hint}");
    assert!(!hint.contains("review-change"), "{hint}");
    assert!(ids(&route["standards"]).starts_with(&["rings", "changelog", "commits"]));

    // A change request stays quiet at the prompt; the task is then bound.
    let asked = host.hook(
        &repo,
        "before-prompt",
        json!({"prompt": "Add a refund endpoint to the web module"}),
    );
    assert_eq!(asked, "", "a change request names no skills at the prompt");
    repo.cli(&[
        "task-start",
        "--session",
        session,
        "--task",
        "refund",
        "--goal",
        "Refund endpoint",
        "--file",
        "src/web/refund.ts",
    ])
    .unwrap();

    // The first edit in the web module carries every standard of its scope.
    let call = host.edit(&repo, &["src/web/refund.ts"]).remove(0);
    let hint = host.hook(&repo, "before-tool", call);
    let route = data_after(&hint, "Routing hints");
    assert_eq!(route["intent"], "implement", "{hint}");
    assert_eq!(route["modules"][0]["name"], "web", "{hint}");
    assert_eq!(
        ids(&route["standards"]),
        ["rings", "changelog", "commits", "api-review", "api-guide"]
    );
    repo.write(
        "src/web/refund.ts",
        "import { Order } from '../domain/order'\nexport const refund = (o: Order) => o\n",
    );
    assert_eq!(
        host.after_edit(&repo, &["src/web/refund.ts"]),
        "",
        "an allowed import and no anchor in scope: quiet"
    );

    // Edits that break the rings and the changelog are reported in the tool result.
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
        "# Changelog\n\n## [Unreleased]\n\n### Added\n- Refunds.\n",
    );
    assert_eq!(
        host.after_edit(&repo, &["src/domain/order.ts", "CHANGELOG.md"]),
        "",
        "fixed: quiet"
    );

    // The commit message is checked against the declared commit standard.
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

    // A test run that executed nothing is said at once and cannot back a verified pass.
    let zero = host.hook(
        &repo,
        "after-tool",
        host.shell("node --test --test-name-pattern=refund", ZERO),
    );
    assert!(zero.contains("executed no test"), "{zero}");
    let refused = verify(&repo, session).unwrap_err();
    assert!(refused.contains("contradicts the test run"), "{refused}");
    let passed = host.hook(&repo, "after-tool", host.shell("node --test", PASS));
    assert_eq!(passed, "", "a passing run is quiet");
    verify(&repo, session).unwrap();

    // After compaction the task returns with its checked and reviewed standards and its tests.
    host.hook(&repo, "before-compact", json!({"trigger": "auto"}));
    let resumed = host.hook(&repo, "after-compact", json!({"source": "compact"}));
    let resume = data_after(&resumed, "Craft task records to resume from");
    assert_eq!(resume["task"], "refund", "{resumed}");
    assert_eq!(
        ids(&resume["standards"]),
        ["rings", "changelog", "commits", "api-review"],
        "advise-level guidance returns with routing, not with the resume"
    );
    assert_eq!(resume["tests"]["status"], "passed", "{resumed}");
    assert_eq!(resume["tests"]["executed"], 2, "{resumed}");

    // A subagent the host starts hears the standards of the work it was delegated from.
    let delegated = host.hook(&repo, "agent-start", json!({"agent_type": "default"}));
    let delegation = data_after(&delegated, "Project standards of the work");
    assert_eq!(delegation["intent"], "implement", "{delegated}");
    assert_eq!(
        ids(&delegation["standards"]),
        ["rings", "changelog", "commits", "api-review", "api-guide"]
    );

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
    assert_eq!(summary["review"][0]["id"], "api-review", "{stop}");
    assert_eq!(
        host.hook(&repo, "turn-end", json!({"stop_hook_active": false})),
        "",
        "the same findings never hold a stop twice"
    );

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
}

/// The same work as a live session does it: no Craft task is ever bound. One edit writes a
/// file in each module; compaction must still restate the standards of what was edited and
/// the tests the session ran.
fn untasked(host: Host) {
    let repo = Repo::new(
        &format!("untasked-{}", host.name),
        &[
            (".craft/project.json", PROJECT),
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
    untasked(Host {
        name: "claude",
        session: "claude-untasked",
    });
}

#[test]
fn codex_keeps_standards_and_tests_without_a_task() {
    untasked(Host {
        name: "codex",
        session: "codex-untasked",
    });
}

#[test]
fn claude_code_reaches_every_feature_in_one_task() {
    scenario(Host {
        name: "claude",
        session: "claude-session",
    });
}

#[test]
fn codex_reaches_every_feature_in_one_task() {
    scenario(Host {
        name: "codex",
        session: "codex-session",
    });
}
