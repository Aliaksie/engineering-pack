//! Host hook events through the native protocol: decisions, telemetry and failures.
use serde_json::{Value, json};
use std::fs;
use std::path::{Path, PathBuf};

struct Scratch(PathBuf);
impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn project(name: &str, config: Option<&str>) -> Scratch {
    let root = std::env::temp_dir().join(format!("craft-host-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(root.join(".git")).unwrap();
    fs::create_dir_all(root.join("deep/er")).unwrap();
    if let Some(config) = config {
        fs::create_dir_all(root.join(".craft")).unwrap();
        fs::write(root.join(".craft/project.json"), config).unwrap();
    }
    Scratch(fs::canonicalize(&root).unwrap())
}

fn facts(root: &Path, handler: &str, extra: Value) -> Value {
    let mut facts = json!({
        "handler": handler, "host": "claude", "package": "craft-review", "lifecycle": "before-tool",
        "scope": if handler == "observe" { "any" } else { "shell" }, "inputStatus": "complete",
        "tool": "shell", "contextSupported": true, "cwd": root.to_string_lossy(), "session": "S-1",
    });
    for (key, value) in extra.as_object().unwrap() {
        facts[key] = value.clone();
    }
    facts
}

fn hook(input: Value) -> Value {
    craft_application::host::run(&input).unwrap()
}

fn events(root: &Path) -> Vec<Value> {
    fn walk(dir: &Path, found: &mut Vec<PathBuf>) {
        for entry in fs::read_dir(dir).into_iter().flatten().flatten() {
            let path = entry.path();
            if path.is_dir() {
                walk(&path, found);
            } else if path.ends_with("events.jsonl") {
                found.push(path);
            }
        }
    }
    let mut files = Vec::new();
    walk(&root.join(".craft/state"), &mut files);
    files
        .iter()
        .flat_map(|file| {
            fs::read_to_string(file)
                .unwrap()
                .lines()
                .map(|l| serde_json::from_str(l).unwrap())
                .collect::<Vec<Value>>()
        })
        .collect()
}

/// `name<parent>[reason] attributes`, for comparing event sequences.
fn view(events: &[Value]) -> Vec<String> {
    events
        .iter()
        .map(|e| {
            let parent = events
                .iter()
                .find(|p| p["operationId"] == e["parentOperationId"])
                .map_or("", |p| p["name"].as_str().unwrap());
            let reason = e["reasonCode"]
                .as_str()
                .map_or(String::new(), |r| format!("[{r}]"));
            format!(
                "{}<{parent}>{reason} {} {}",
                e["name"].as_str().unwrap(),
                e["outcome"].as_str().unwrap(),
                e["attributes"]
            )
        })
        .collect()
}

#[test]
fn observation_records_session_boundaries_without_reading_the_project() {
    let fixture = project("observe", Some("INVALID"));
    for lifecycle in [
        "before-prompt",
        "before-tool",
        "after-tool",
        "turn-end",
        "session-start",
    ] {
        let result = hook(facts(
            &fixture.0,
            "observe",
            json!({"package": "craft-engineering", "tool": "read", "lifecycle": lifecycle}),
        ));
        assert_eq!(result["decision"], "allow");
        assert_eq!(result["failed"], false);
        assert_eq!(result["export"], Value::Null);
    }
    // A routine event is recorded by the handler that acts on it, not by every observer.
    assert_eq!(
        view(&events(&fixture.0)),
        [
            r#"host.hook<> success {"lifecycle":"session-start","inputStatus":"complete","decision":"allow","contextSupported":true,"package":"craft-engineering","identityScope":"session"}"#,
        ]
    );
    let event = &events(&fixture.0)[0];
    assert_eq!(event["host"], "claude");
    assert_eq!(event["kind"], "operation");
}

#[test]
fn unread_input_fails_the_hook_and_invalid_input_is_diagnosed() {
    let fixture = project("unread", None);
    for (handler, status) in [
        ("observe", "oversized"),
        ("review-read-only", "read-failure"),
    ] {
        let result = hook(facts(
            &fixture.0,
            handler,
            json!({"inputStatus": status, "session": null, "package": "craft-stories"}),
        ));
        assert_eq!(result, json!({"failed": true, "export": null}));
    }
    hook(facts(
        &fixture.0,
        "observe",
        json!({"inputStatus": "invalid", "session": null, "package": "unknown-package"}),
    ));
    assert_eq!(
        view(&events(&fixture.0)),
        [
            r#"diagnostic<host.hook>[INPUT_TOO_LARGE] failure {"inputStatus":"oversized","lifecycle":"before-tool","area":"host","package":"craft-stories","identityScope":"invocation"}"#,
            r#"host.hook<>[HOOK_FAILED] failure {"lifecycle":"before-tool","inputStatus":"oversized","package":"craft-stories","identityScope":"invocation"}"#,
            r#"diagnostic<host.hook>[INVALID_INPUT] failure {"inputStatus":"read-failure","lifecycle":"before-tool","area":"host","package":"craft-stories","identityScope":"invocation"}"#,
            r#"host.hook<>[HOOK_FAILED] failure {"package":"craft-stories","identityScope":"invocation"}"#,
            r#"diagnostic<host.hook>[INVALID_INPUT] failure {"inputStatus":"invalid","lifecycle":"before-tool","area":"host","identityScope":"invocation"}"#,
            r#"host.hook<> success {"lifecycle":"before-tool","inputStatus":"invalid","decision":"allow","identityScope":"invocation"}"#,
        ]
    );
}

#[test]
fn an_enabled_shell_guard_denies_input_it_could_not_read() {
    let denied = json!({"decision": "deny", "reason": "Policy could not be evaluated: hook input could not be read in full", "failed": true, "export": null});
    let guarded = project(
        "unread-guarded",
        Some(r#"{"formatVersion":1,"review":{"readOnly":true}}"#),
    );
    let invalid = project("unread-invalid", Some("{"));
    for status in ["oversized", "read-failure"] {
        for fixture in [&guarded, &invalid] {
            let result = hook(facts(
                &fixture.0,
                "review-read-only",
                json!({"inputStatus": status}),
            ));
            assert_eq!(result, denied);
        }
        // Only shell enforcement before a tool call denies; other scopes keep failing open.
        for extra in [json!({"scope": "any"}), json!({"lifecycle": "after-tool"})] {
            let mut input = facts(&guarded.0, "review-read-only", extra);
            input["inputStatus"] = json!(status);
            assert_eq!(hook(input), json!({"failed": true, "export": null}));
        }
    }
}

#[test]
fn read_only_review_denies_publishing_shell_commands_only_when_configured() {
    let guarded = project(
        "guarded",
        Some(r#"{"formatVersion":1,"review":{"readOnly":true}}"#),
    );
    let deny = hook(facts(
        &guarded.0,
        "review-read-only",
        json!({"command": "git status && git push"}),
    ));
    assert_eq!(deny["decision"], "deny");
    assert!(
        deny["reason"]
            .as_str()
            .unwrap()
            .contains("read-only review")
    );
    for extra in [
        json!({"command": "git status"}),
        json!({"command": 42}),
        json!({"tool": "read", "command": "git push"}),
    ] {
        let allow = hook(facts(&guarded.0, "review-read-only", extra));
        assert_eq!(
            (allow["decision"].as_str(), allow.get("reason")),
            (Some("allow"), None)
        );
    }
    let names: Vec<String> = events(&guarded.0)
        .iter()
        .map(|e| e["name"].as_str().unwrap().to_string())
        .collect();
    // A non-shell call to a shell-scoped hook records the hook only; unchanged settings
    // are recorded once per session.
    assert_eq!(names.iter().filter(|n| *n == "project.loaded").count(), 1);
    assert_eq!(names.iter().filter(|n| *n == "host.hook").count(), 4);
    let applied = events(&guarded.0)
        .iter()
        .filter(|e| e["attributes"].get("contextSupported").is_some())
        .count();
    assert_eq!(applied, 3);
    let plain = project("plain", None);
    assert_eq!(
        hook(facts(
            &plain.0,
            "review-read-only",
            json!({"command": "git push"})
        ))["decision"],
        "allow"
    );
}

fn loads(root: &Path) -> Vec<String> {
    events(root)
        .iter()
        .filter(|e| e["name"] == "project.loaded")
        .map(|e| {
            format!(
                "{}{}",
                e["outcome"].as_str().unwrap(),
                e["reasonCode"]
                    .as_str()
                    .map_or(String::new(), |r| format!("[{r}]"))
            )
        })
        .collect()
}

#[test]
fn hooks_record_project_settings_once_per_session_until_they_change() {
    let fixture = project("settings-once", Some(r#"{"formatVersion":1}"#));
    let config = fixture.0.join(".craft/project.json");
    let shell = |extra: Value| hook(facts(&fixture.0, "review-read-only", extra));
    let deliver = |session: &str| {
        hook(facts(
            &fixture.0,
            "deliver",
            json!({"package": "craft-delivery", "lifecycle": "before-prompt", "scope": "any",
                "prompt": "explain the module", "session": session}),
        ))
    };
    for _ in 0..3 {
        assert_eq!(shell(json!({"command": "git status"}))["decision"], "allow");
    }
    deliver("S-1");
    assert_eq!(loads(&fixture.0), ["success"]);
    assert_eq!(
        events(&fixture.0)
            .iter()
            .filter(|e| e["name"] == "host.hook")
            .count(),
        4
    );

    // Changed settings are recorded again, then not while unchanged.
    fs::write(&config, r#"{"formatVersion":1,"review":{"readOnly":true}}"#).unwrap();
    assert_eq!(shell(json!({"command": "git push"}))["decision"], "deny");
    shell(json!({"command": "git status"}));
    assert_eq!(loads(&fixture.0), ["success", "success"]);

    // Every failure is recorded; the settings that follow it are recorded again.
    fs::write(&config, "{").unwrap();
    shell(json!({"command": "git status"}));
    shell(json!({"command": "git status"}));
    fs::write(&config, r#"{"formatVersion":1,"review":{"readOnly":true}}"#).unwrap();
    shell(json!({"command": "git status"}));
    assert_eq!(
        loads(&fixture.0),
        [
            "success",
            "success",
            "failure[CONFIG_INVALID]",
            "failure[CONFIG_INVALID]",
            "success"
        ]
    );

    // Another session records the same settings for itself.
    deliver("S-2");
    deliver("S-2");
    assert_eq!(loads(&fixture.0).len(), 6);

    // The record is telemetry state keyed by pseudonym; no session state is added for it.
    let record =
        fs::read_to_string(fixture.0.join(".craft/state/telemetry/settings.json")).unwrap();
    assert!(!record.contains("S-1") && !record.contains("S-2"));
    let record: Value = serde_json::from_str(&record).unwrap();
    assert_eq!(record["sessions"].as_array().unwrap().len(), 2);
    for session in fs::read_dir(fixture.0.join(".craft/state/sessions")).unwrap() {
        assert!(!session.unwrap().path().join("settings.json").exists());
    }
}

#[test]
fn hooks_without_a_session_record_project_settings_each_time() {
    let fixture = project("settings-sessionless", Some(r#"{"formatVersion":1}"#));
    for _ in 0..2 {
        let mut input = facts(
            &fixture.0,
            "review-read-only",
            json!({"command": "git status"}),
        );
        input.as_object_mut().unwrap().remove("session");
        hook(input);
    }
    assert_eq!(loads(&fixture.0), ["success", "success"]);
}

#[test]
fn an_unreadable_project_fails_shell_policy_closed() {
    let fixture = project("invalid", Some("{"));
    let result = hook(facts(
        &fixture.0,
        "review-read-only",
        json!({"command": "git status"}),
    ));
    assert_eq!(result["decision"], "deny");
    assert_eq!(result["failed"], true);
    assert!(
        result["reason"]
            .as_str()
            .unwrap()
            .starts_with("Policy could not be evaluated: ")
    );
    assert_eq!(
        view(&events(&fixture.0)),
        [
            r#"project.loaded<host.hook>[CONFIG_INVALID] failure {"package":"craft-review","identityScope":"session"}"#,
            r#"diagnostic<host.hook>[HOOK_FAILED] failure {"area":"host","package":"craft-review","identityScope":"session"}"#,
            r#"host.hook<>[HOOK_FAILED] failure {"lifecycle":"before-tool","inputStatus":"complete","decision":"deny","contextSupported":true,"package":"craft-review","identityScope":"session"}"#,
        ]
    );
}

#[test]
fn hooks_resolve_the_project_from_a_nested_working_directory() {
    let fixture = project(
        "nested",
        Some(r#"{"formatVersion":1,"review":{"readOnly":true}}"#),
    );
    let nested = fixture.0.join("deep/er");
    assert_eq!(
        hook(facts(
            &nested,
            "review-read-only",
            json!({"command": "git push"})
        ))["decision"],
        "deny"
    );
    assert_eq!(events(&fixture.0).len(), 2);
    assert!(!nested.join(".craft").exists());
    // Without project markers, the working directory itself is the root.
    let bare = std::env::temp_dir().join(format!("craft-host-bare-{}", std::process::id()));
    fs::create_dir_all(&bare).unwrap();
    let bare = Scratch(fs::canonicalize(&bare).unwrap());
    if !bare
        .0
        .ancestors()
        .any(|a| a.join(".git").exists() || a.join(".craft/project.json").exists())
    {
        hook(facts(
            &bare.0,
            "observe",
            json!({"lifecycle": "session-start"}),
        ));
        assert_eq!(events(&bare.0).len(), 1);
    }
}

#[test]
fn malformed_facts_are_rejected() {
    let fixture = project("malformed", None);
    for extra in [
        json!({"handler": "context"}),
        json!({"lifecycle": "PreToolUse"}),
        json!({"inputStatus": "partial"}),
        json!({"tool": "bash"}),
        json!({"scope": "edit"}),
        json!({"contextSupported": "yes"}),
        json!({"cwd": null}),
        json!({"host": 1}),
        json!({"session": 7}),
        json!({"package": false}),
    ] {
        let decision = craft_application::host::run(&facts(&fixture.0, "observe", extra.clone()));
        assert!(decision.is_err(), "{extra}");
    }
    assert!(
        !fixture.0.join(".craft").exists(),
        "rejected facts record nothing"
    );
}

#[test]
fn a_git_entry_alone_marks_the_project_root() {
    let fixture = project("git-root", None);
    let nested = fixture.0.join("deep/er");
    hook(facts(
        &nested,
        "observe",
        json!({"contextSupported": false, "lifecycle": "session-start"}),
    ));
    assert_eq!(
        view(&events(&fixture.0))[0],
        r#"host.hook<> success {"lifecycle":"session-start","inputStatus":"complete","decision":"allow","contextSupported":false,"package":"craft-review","identityScope":"session"}"#
    );
    assert!(!nested.join(".craft").exists());
}

#[test]
fn a_failed_policy_outside_shell_enforcement_continues_with_unavailable_context() {
    let fixture = project("non-enforcing", Some("{"));
    let result = hook(facts(
        &fixture.0,
        "review-read-only",
        json!({"lifecycle": "after-tool", "command": "git push"}),
    ));
    assert_eq!(result["decision"], "allow");
    assert_eq!(result["failed"], true);
    assert_eq!(result["context"]["kind"], "unavailable");
    assert!(result["context"]["data"].is_string());
    assert_eq!(result.get("reason"), None);
}

const STANDARDS: &str = r#"{"formatVersion":1,"standards":[
  {"id":"changelog","anchor":"keep-a-changelog"},
  {"id":"commits","anchor":"conventional-commits"}
]}"#;

fn written(root: &Path, extra: Value) -> Value {
    hook(facts(
        root,
        "deliver",
        json!({"package": "craft-delivery", "lifecycle": "after-tool", "scope": "any"})
            .as_object()
            .unwrap()
            .iter()
            .chain(extra.as_object().unwrap())
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect::<serde_json::Map<_, _>>()
            .into(),
    ))
}

#[test]
fn a_project_catalog_anchor_reuses_a_craft_check_on_its_own_files() {
    let fixture = project(
        "standards-catalog",
        Some(
            r#"{"formatVersion":1,"anchors":{"catalogs":[".craft/anchors.json"]},
            "standards":[{"id":"decisions","anchor":"acme-adr","scope":{"paths":["decisions"]}}]}"#,
        ),
    );
    let root = &fixture.0;
    fs::write(
        root.join(".craft/anchors.json"),
        r#"{"anchors":[{"id":"acme-adr","title":"ACME decision record","check":"nygard-adr"}]}"#,
    )
    .unwrap();
    fs::create_dir_all(root.join("decisions")).unwrap();
    fs::write(
        root.join("decisions/0001-store.md"),
        "# 1. Store\n\nWe chose X.\n",
    )
    .unwrap();
    let edit =
        json!({"tool": "edit", "file": root.join("decisions/0001-store.md").to_string_lossy()});
    let found = written(root, edit);
    assert_eq!(found["standards"][0]["standard"], "decisions");
    assert_eq!(found["standards"][0]["anchor"], "acme-adr");

    fs::remove_file(root.join(".craft/anchors.json")).unwrap();
    let edit =
        json!({"tool": "edit", "file": root.join("decisions/0001-store.md").to_string_lossy()});
    let missing = written(root, edit);
    assert_eq!(
        missing["decision"], "allow",
        "a broken catalog never blocks the tool"
    );
    assert!(missing.get("standards").is_none());
}

#[test]
fn a_written_file_that_misses_a_declared_standard_is_reported_in_the_tool_result() {
    let fixture = project("standards-file", Some(STANDARDS));
    let root = &fixture.0;
    let edit = json!({"tool": "edit", "file": root.join("CHANGELOG.md").to_string_lossy()});
    fs::write(
        root.join("CHANGELOG.md"),
        "# Changelog\n\n## Next\n\n### Misc\n- x\n",
    )
    .unwrap();
    let decision = written(root, edit.clone());
    assert_eq!(
        decision["decision"], "allow",
        "a check reports, it never blocks"
    );
    let found = decision["standards"]
        .as_array()
        .expect("unmet standard reported");
    assert_eq!(found[0]["standard"], "changelog");
    assert!(
        found[0]["findings"]
            .as_array()
            .is_some_and(|f| !f.is_empty())
    );

    fs::write(
        root.join("CHANGELOG.md"),
        "# Changelog\n\n## [Unreleased]\n\n### Fixed\n- Keep quoted keys.\n",
    )
    .unwrap();
    assert!(
        written(root, edit).get("standards").is_none(),
        "a passing check is quiet"
    );

    let other = json!({"tool": "edit", "file": root.join("notes.md").to_string_lossy()});
    fs::write(root.join("notes.md"), "## Next\n").unwrap();
    assert!(
        written(root, other).get("standards").is_none(),
        "outside the standard's scope"
    );
}

#[test]
fn a_commit_just_made_is_checked_against_the_declared_commit_standard() {
    let fixture = project("standards-commit", Some(STANDARDS));
    let root = &fixture.0;
    fs::remove_dir_all(root.join(".git")).unwrap();
    let git = |args: &[&str]| {
        let status = std::process::Command::new("git")
            .args([
                "-c",
                "user.name=t",
                "-c",
                "user.email=t@t",
                "-c",
                "commit.gpgsign=false",
            ])
            .args(args)
            .current_dir(root)
            .output()
            .unwrap();
        assert!(status.status.success(), "{status:?}");
    };
    git(&["init", "-q"]);
    fs::write(root.join("a.txt"), "a").unwrap();
    git(&["add", "a.txt"]);
    git(&["commit", "-qm", "Updated stuff"]);
    let shell =
        json!({"tool": "shell", "command": "git add a.txt && git commit -m 'Updated stuff'"});
    let found = written(root, shell);
    assert_eq!(found["standards"][0]["standard"], "commits");

    fs::write(root.join("a.txt"), "b").unwrap();
    git(&["commit", "-qam", "fix(a): keep b"]);
    let shell = json!({"tool": "shell", "command": "git commit -am 'fix(a): keep b'"});
    assert!(written(root, shell).get("standards").is_none());
    let unrelated = json!({"tool": "shell", "command": "git log -1"});
    assert!(
        written(root, unrelated).get("standards").is_none(),
        "only a commit command"
    );
}

#[test]
fn an_edit_that_adds_an_undeclared_module_import_is_reported_and_old_debt_is_not() {
    let fixture = project(
        "standards-modules",
        Some(
            r#"{"formatVersion":1,"modules":[
              {"name":"domain","path":"src/domain","dependsOn":[]},
              {"name":"web","path":"src/web","dependsOn":["domain"]},
              {"name":"store","path":"src/store","dependsOn":["domain"]}],
            "standards":[{"id":"rings","anchor":"hexagonal-architecture"}]}"#,
        ),
    );
    let root = &fixture.0;
    fs::remove_dir_all(root.join(".git")).unwrap();
    let git = |args: &[&str]| {
        let status = std::process::Command::new("git")
            .args([
                "-c",
                "user.name=t",
                "-c",
                "user.email=t@t",
                "-c",
                "commit.gpgsign=false",
            ])
            .args(args)
            .current_dir(root)
            .output()
            .unwrap();
        assert!(status.status.success(), "{status:?}");
    };
    fs::create_dir_all(root.join("src/domain")).unwrap();
    let order = root.join("src/domain/order.ts");
    // Existing debt in HEAD: the domain already imports the web module.
    fs::write(
        &order,
        "import { http } from '../web/http'\nexport class Order {}\n",
    )
    .unwrap();
    git(&["init", "-q"]);
    git(&["add", "-A"]);
    git(&["commit", "-qm", "init"]);
    let edit = json!({"tool": "edit", "file": order.to_string_lossy()});
    assert!(
        written(root, edit.clone()).get("standards").is_none(),
        "unchanged imports are not this change's debt"
    );

    fs::write(
        &order,
        "import { http } from '../web/http'\nimport { Db } from '../store/db'\nimport { Money } from './money'\nexport class Order {}\n",
    )
    .unwrap();
    let decision = written(root, edit);
    assert_eq!(decision["decision"], "allow", "a finding never blocks");
    let found = &decision["standards"][0];
    assert_eq!(found["standard"], "rings");
    assert_eq!(found["anchor"], "hexagonal-architecture");
    let findings = found["findings"].as_array().unwrap();
    assert_eq!(findings.len(), 1, "{findings:?}");
    assert_eq!(findings[0]["line"], 2);
    assert_eq!(findings[0]["rule"], "undeclared-dependency");
}

fn lifecycle(root: &Path, lifecycle: &str, extra: Value) -> Value {
    let mut input = facts(
        root,
        "deliver",
        json!({"package": "craft-delivery", "lifecycle": lifecycle, "scope": "any", "tool": "other"}),
    );
    for (key, value) in extra.as_object().unwrap() {
        input[key] = value.clone();
    }
    hook(input)
}

#[test]
fn a_turn_that_adds_debt_ends_with_one_summary_said_once() {
    let fixture = project(
        "stop-review",
        Some(
            r#"{"formatVersion":1,"routing":{"debtMarkers":true},"modules":[
              {"name":"domain","path":"src/domain","dependsOn":[]},
              {"name":"web","path":"src/web","dependsOn":["domain"]}],
            "standards":[{"id":"rings","anchor":"hexagonal-architecture"},
              {"id":"api-review","checklist":["Errors map to status codes"],"scope":{"paths":["src/web"]}}]}"#,
        ),
    );
    let root = &fixture.0;
    fs::remove_dir_all(root.join(".git")).unwrap();
    let git = |args: &[&str]| {
        let output = std::process::Command::new("git")
            .args([
                "-c",
                "user.name=t",
                "-c",
                "user.email=t@t",
                "-c",
                "commit.gpgsign=false",
            ])
            .args(args)
            .current_dir(root)
            .output()
            .unwrap();
        assert!(output.status.success(), "{output:?}");
    };
    fs::create_dir_all(root.join("src/domain")).unwrap();
    fs::create_dir_all(root.join("src/web")).unwrap();
    let order = root.join("src/domain/order.ts");
    fs::write(&order, "export class Order {}\n").unwrap();
    // Uncommitted work from before the session: a TODO the session did not write.
    fs::write(root.join("src/web/old.ts"), "// TODO: before\n").unwrap();
    git(&["init", "-q"]);
    git(&["add", "src/domain"]);
    git(&["commit", "-qm", "init"]);
    lifecycle(root, "session-start", json!({}));
    assert!(
        lifecycle(root, "turn-end", json!({})).get("stop").is_none(),
        "nothing changed this session"
    );

    fs::write(
        &order,
        "import { http } from '../web/http'\n// TODO: money\nexport class Order {}\n",
    )
    .unwrap();
    fs::write(root.join("src/web/http.ts"), "export const http = 1\n").unwrap();
    // While the host continues a held stop, nothing is said.
    assert!(
        lifecycle(root, "turn-end", json!({"stopActive": true}))
            .get("stop")
            .is_none()
    );
    let decision = lifecycle(root, "turn-end", json!({}));
    let stop = &decision["stop"];
    let rules: Vec<(&str, &str)> = stop["findings"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| (f["file"].as_str().unwrap(), f["rule"].as_str().unwrap()))
        .collect();
    assert_eq!(
        rules,
        vec![
            ("src/domain/order.ts", "undeclared-dependency"),
            ("src/domain/order.ts", "todo-marker")
        ],
        "{stop}"
    );
    assert_eq!(stop["findings"][1]["line"], 2);
    assert_eq!(stop["review"][0]["id"], "api-review");
    assert_eq!(decision["decision"], "allow");

    // The same findings never hold a stop again, even after another edit.
    fs::write(
        &order,
        "import { http } from '../web/http'\n// TODO: money\nexport class Order { id = 1 }\n",
    )
    .unwrap();
    assert!(lifecycle(root, "turn-end", json!({})).get("stop").is_none());

    // Off when the project says so.
    let config = fs::read_to_string(root.join(".craft/project.json")).unwrap();
    fs::write(
        root.join(".craft/project.json"),
        config.replace(
            r#""debtMarkers":true"#,
            r#""debtMarkers":true,"stopReview":false"#,
        ),
    )
    .unwrap();
    fs::write(&order, "// FIXME: new\n").unwrap();
    assert!(lifecycle(root, "turn-end", json!({})).get("stop").is_none());
}

/// A model can write every file through shell commands and commit before the turn ends: no
/// edit hook fires and the worktree is clean at the stop. The stop still reviews that work.
#[test]
fn a_stop_reviews_files_a_shell_command_wrote_and_work_committed_in_the_turn() {
    let fixture = project(
        "stop-committed",
        Some(
            r#"{"formatVersion":1,"routing":{"debtMarkers":true},
            "standards":[{"id":"changelog","anchor":"keep-a-changelog"}]}"#,
        ),
    );
    let root = &fixture.0;
    fs::remove_dir_all(root.join(".git")).unwrap();
    let git = |args: &[&str]| {
        let output = std::process::Command::new("git")
            .args([
                "-c",
                "user.name=t",
                "-c",
                "user.email=t@t",
                "-c",
                "commit.gpgsign=false",
            ])
            .args(args)
            .current_dir(root)
            .output()
            .unwrap();
        assert!(output.status.success(), "{output:?}");
    };
    let stop = || lifecycle(root, "turn-end", json!({}));
    let files = |decision: &Value| -> Vec<(String, String)> {
        decision["stop"]["findings"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|f| {
                let text = |key: &str| f[key].as_str().unwrap_or_default().to_owned();
                (text("file"), text("standard"))
            })
            .collect()
    };
    fs::write(root.join("a.ts"), "export const a = 1\n").unwrap();
    git(&["init", "-q"]);
    git(&["add", "a.ts"]);
    git(&["commit", "-qm", "init"]);
    lifecycle(root, "session-start", json!({}));

    // Written without an edit tool, then committed: clean at the stop.
    fs::write(
        root.join("CHANGELOG.md"),
        "# Changelog\n\n## Next\n\n### Misc\n- x\n",
    )
    .unwrap();
    fs::write(root.join("a.ts"), "// TODO: later\nexport const a = 2\n").unwrap();
    git(&["add", "-A"]);
    git(&["commit", "-qm", "feat: change"]);
    let decision = stop();
    let found = files(&decision);
    assert!(
        found.contains(&("CHANGELOG.md".into(), "changelog".into())),
        "the anchor check no edit hook ran: {decision}"
    );
    assert!(
        found.contains(&("a.ts".into(), String::new())),
        "the marker committed in the turn: {decision}"
    );
    assert!(stop().get("stop").is_none(), "said once");

    // A finding the session heard when it edited a file is not said again at the stop: this
    // file's first finding comes from the edit hook, never from a stop.
    fs::create_dir_all(root.join("docs")).unwrap();
    let heard_file = root.join("docs/CHANGELOG.md");
    fs::write(&heard_file, "# Changelog\n\n## Later\n\n### Other\n- y\n").unwrap();
    let heard = written(
        root,
        json!({"tool": "edit", "file": heard_file.to_string_lossy()}),
    );
    assert_eq!(heard["standards"][0]["standard"], "changelog", "{heard}");
    assert!(stop().get("stop").is_none(), "heard at the edit");

    // Commits that arrive another way (a pull, a merge) are other people's work.
    git(&["checkout", "-qb", "theirs"]);
    fs::write(root.join("c.ts"), "// FIXME: theirs\nexport const c = 1\n").unwrap();
    git(&["add", "-A"]);
    git(&["commit", "-qm", "feat: theirs"]);
    git(&["checkout", "-q", "-"]);
    // Concluded by hand, a merge is recorded as a commit; it is still their work.
    git(&["merge", "-q", "--no-ff", "--no-commit", "theirs"]);
    git(&["commit", "-qm", "merge theirs"]);
    assert!(
        stop().get("stop").is_none(),
        "a merge is not this session's change"
    );
    // What the session commits next is reviewed against the commit that stop saw.
    fs::write(root.join("d.ts"), "// HACK: ours\nexport const d = 1\n").unwrap();
    git(&["add", "-A"]);
    git(&["commit", "-qm", "feat: ours"]);
    let decision = stop();
    assert_eq!(
        files(&decision),
        [("d.ts".to_string(), String::new())],
        "{decision}"
    );
}

#[test]
fn a_subagent_the_host_starts_hears_the_standards_of_the_delegating_work() {
    let fixture = project(
        "delegation",
        Some(
            r#"{"formatVersion":1,"standards":[
            {"id":"stories","anchor":"connextra-user-story","scope":{"paths":["docs/stories"]}},
            {"id":"api","file":"docs/api.md","scope":{"intents":["implement"]}}
        ]}"#,
        ),
    );
    let root = &fixture.0;
    let deliver = |lifecycle: &str, extra: Value| {
        let mut input = facts(
            root,
            "deliver",
            json!({"package": "craft-delivery", "lifecycle": lifecycle, "scope": "any", "tool": "other"}),
        );
        for (key, value) in extra.as_object().unwrap() {
            input[key] = value.clone();
        }
        hook(input)
    };
    // Nothing established yet: the subagent gets nothing, no guess.
    let quiet = deliver("agent-start", json!({}));
    assert_eq!(quiet["decision"], "allow");
    assert!(quiet.get("delegation").is_none());

    deliver(
        "before-prompt",
        json!({"prompt": "Write user stories for the checkout flow"}),
    );
    let started = deliver("agent-start", json!({}));
    assert_eq!(started["decision"], "allow");
    assert_eq!(started["delegation"]["intent"], "story");
    let ids: Vec<&str> = started["delegation"]["standards"]
        .as_array()
        .expect("standards carried")
        .iter()
        .filter_map(|s| s["id"].as_str())
        .collect();
    assert_eq!(ids, vec!["stories"]);

    // A host that cannot take context at this event is not answered with any.
    let blind = deliver("agent-start", json!({"contextSupported": false}));
    assert!(blind.get("delegation").is_none());
}

/// A hook with nothing to say still answers where the host needs an answer: Cursor blocks a
/// permission hook whose output does not match its schema, so silence could block every call.
#[test]
fn a_silent_hook_answers_cursor_within_its_schema() {
    let fixture = project("silent-answer", None);
    let wire = |host: &str, lifecycle: &str, payload: &Value| {
        let args = [
            host,
            lifecycle,
            "--package",
            "craft-engineering",
            "--handler",
            "observe",
        ]
        .map(String::from);
        let (wire, _) = craft_application::hook::run(&args, &mut payload.to_string().as_bytes());
        assert_eq!((wire.exit_code, wire.stderr.as_str()), (0, ""), "{host}");
        wire.stdout
    };
    let cwd = fixture.0.to_string_lossy();
    let tool = json!({"session_id": "S-1", "cwd": cwd, "tool_name": "Shell", "tool_input": {"command": "ls"}});
    let prompt = json!({"session_id": "S-1", "cwd": cwd, "prompt": "hello"});
    assert_eq!(
        wire("cursor", "before-tool", &tool),
        "{\"permission\":\"allow\"}\n"
    );
    assert_eq!(
        wire("cursor", "before-prompt", &prompt),
        "{\"continue\":true}\n"
    );
    // Not a permission hook: nothing to say stays nothing.
    assert_eq!(wire("cursor", "after-tool", &tool), "");
    // Hosts that keep their own permission flow on empty output get no grant.
    for host in ["claude", "codex", "copilot", "gemini"] {
        assert_eq!(wire(host, "before-tool", &tool), "", "{host}");
    }
}

/// `craft hook <host> before-prompt` for the delivery package running from `bundle`.
fn prompted(host: &str, project: &Path, bundle: &Path, home: &Path, prompt: &str) -> String {
    use std::io::Write;
    let mut child = std::process::Command::new(env!("CARGO_BIN_EXE_craft"))
        .args(["hook", host, "before-prompt"])
        .args(["--package", "craft-delivery", "--handler", "deliver"])
        .env("CRAFT_PACKAGE_ROOT", bundle)
        .env("CRAFT_EVIDENCE_WARM", "0")
        .env("CLAUDE_CONFIG_DIR", home.join("claude"))
        .env("CODEX_HOME", home.join("codex"))
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    let payload = json!({"session_id": "S-1", "cwd": project, "prompt": prompt});
    child
        .stdin
        .take()
        .unwrap()
        .write_all(payload.to_string().as_bytes())
        .unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).into_owned()
}

/// A prompt hook names only skills of packages the host installed beside it: a route into a
/// package that is not there says nothing instead of naming skills nobody can load.
#[test]
fn a_prompt_routes_only_into_packages_the_host_installed() {
    let fixture = project("installed-packages", Some(r#"{"formatVersion": 1}"#));
    let home = fixture.0.join("home");
    let bundle = |market: &str, package: &str| {
        let path = home.join(format!("{market}/{package}/0.0.5"));
        fs::create_dir_all(path.join("support/native-bin")).unwrap();
        fs::write(path.join("support/native-bin/runtime.json"), "{}").unwrap();
        path
    };
    let prompt = "Create stories for loyalty points";
    for host in ["claude", "codex"] {
        // Only delivery and engineering are in the plugin cache.
        let market = format!("{host}/plugins/cache/craft");
        let delivery = bundle(&market, "craft-delivery");
        bundle(&market, "craft-engineering");
        let silent = prompted(host, &fixture.0, &delivery, &home, prompt);
        assert_eq!(silent, "", "{host}");
        // With the stories package beside it, the same prompt names its workflow.
        bundle(&market, "craft-stories");
        fs::create_dir_all(home.join("codex")).unwrap();
        fs::write(
            home.join("codex/config.toml"),
            "[plugins.\"craft-stories@craft\"]\nenabled = true\n",
        )
        .unwrap();
        // The prompt changed nothing about the session; a new one hears the route.
        let _ = fs::remove_dir_all(fixture.0.join(".craft/state"));
        let routed = prompted(host, &fixture.0, &delivery, &home, prompt);
        assert!(
            routed.contains("craft-stories:workflow-prepare-stories"),
            "{host}: {routed}"
        );
        // Turned off in the host's settings, it is as good as absent.
        let _ = fs::remove_dir_all(fixture.0.join(".craft/state"));
        fs::write(
            home.join("codex/config.toml"),
            "[plugins.\"craft-stories@craft\"]\nenabled = false\n",
        )
        .unwrap();
        fs::write(
            home.join("claude/settings.json"),
            r#"{"enabledPlugins": {"craft-stories@craft": false}}"#,
        )
        .unwrap();
        let off = prompted(host, &fixture.0, &delivery, &home, prompt);
        assert_eq!(off, "", "{host}");
        let _ = fs::remove_file(home.join("claude/settings.json"));
        let _ = fs::remove_dir_all(fixture.0.join(".craft/state"));
    }
    // Outside an installed bundle nothing is known, so the route is the catalog's.
    let loose = prompted("claude", &fixture.0, &fixture.0, &home, prompt);
    assert!(
        loose.contains("craft-stories:workflow-prepare-stories"),
        "{loose}"
    );
}

/// A project whose root is a folder of a larger checkout: Git names changed files from the
/// top, the project reads them from its own root, and its neighbours are not its work.
#[test]
fn a_project_below_the_top_of_its_checkout_is_reviewed_from_its_own_root() {
    let top = std::env::temp_dir().join(format!("craft-host-below-top-{}", std::process::id()));
    let _ = fs::remove_dir_all(&top);
    fs::create_dir_all(top.join("web/.craft")).unwrap();
    fs::create_dir_all(top.join("other")).unwrap();
    let fixture = Scratch(fs::canonicalize(&top).unwrap());
    let top = &fixture.0;
    let root = top.join("web");
    fs::write(
        root.join(".craft/project.json"),
        r#"{"formatVersion":1,"routing":{"debtMarkers":true}}"#,
    )
    .unwrap();
    let git = |args: &[&str]| {
        let output = std::process::Command::new("git")
            .args(["-c", "user.name=t", "-c", "user.email=t@t"])
            .args(["-c", "commit.gpgsign=false"])
            .args(args)
            .current_dir(top)
            .output()
            .unwrap();
        assert!(output.status.success(), "{output:?}");
    };
    let files = |decision: &Value| -> Vec<String> {
        decision["stop"]["findings"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|finding| finding["file"].as_str().map(str::to_owned))
            .collect()
    };
    fs::write(root.join("a.ts"), "export const a = 1\n").unwrap();
    fs::write(top.join("other/x.ts"), "export const x = 1\n").unwrap();
    git(&["init", "-q"]);
    git(&["add", "-A"]);
    git(&["commit", "-qm", "init"]);
    lifecycle(&root, "session-start", json!({}));

    // One file left dirty, one committed in the turn, and a neighbour changed both ways.
    fs::write(root.join("b.ts"), "// TODO: later\nexport const b = 1\n").unwrap();
    fs::write(
        top.join("other/x.ts"),
        "// TODO: theirs\nexport const x = 2\n",
    )
    .unwrap();
    git(&["add", "-A"]);
    git(&["commit", "-qm", "feat: change"]);
    fs::write(root.join("a.ts"), "// FIXME: mine\nexport const a = 2\n").unwrap();
    fs::write(top.join("other/y.ts"), "// TODO: untracked\n").unwrap();
    let decision = lifecycle(&root, "turn-end", json!({}));
    let mut found = files(&decision);
    found.sort();
    assert_eq!(found, ["a.ts", "b.ts"], "{decision}");
}

/// The host names the directory it was opened in; a session whose shell moved into another
/// project still belongs to the first, and leaves no state in the second.
#[test]
fn a_session_keeps_the_project_it_was_opened_in_when_its_shell_moves() {
    let opened = project("opened-in", None);
    let visited = project("visited", None);
    let state = |root: &Path| root.join(".craft/state/sessions").exists();
    lifecycle(
        &visited.0,
        "session-start",
        json!({"projectDirectory": opened.0.to_string_lossy()}),
    );
    assert!(state(&opened.0), "the project the host was opened in");
    assert!(!state(&visited.0), "the project the shell moved to");
    // Opened in a folder of a project, the session is the project's.
    lifecycle(
        &visited.0,
        "session-start",
        json!({"projectDirectory": opened.0.join("deep/er").to_string_lossy(), "session": "S-2"}),
    );
    assert!(!state(&visited.0));
    assert!(!opened.0.join("deep/er/.craft").exists());
    // A directory that is not there names nothing: the shell's directory decides, as before.
    lifecycle(
        &visited.0,
        "session-start",
        json!({"projectDirectory": opened.0.join("gone").to_string_lossy(), "session": "S-3"}),
    );
    assert!(state(&visited.0));
}

/// A workspace root is no repository: the stop reads each member repository, names its files
/// from the workspace root, and the commit check reads the member a commit was just made in.
#[test]
fn a_workspace_root_reviews_what_changed_in_each_member_repository() {
    let top = std::env::temp_dir().join(format!("craft-host-workspace-{}", std::process::id()));
    let _ = fs::remove_dir_all(&top);
    for folder in [".craft", "api", "web", "notes"] {
        fs::create_dir_all(top.join(folder)).unwrap();
    }
    let fixture = Scratch(fs::canonicalize(&top).unwrap());
    let root = &fixture.0;
    fs::write(
        root.join(".craft/project.json"),
        r#"{"formatVersion":1,"routing":{"debtMarkers":true},"standards":[
          {"id":"commits","anchor":"conventional-commits"}]}"#,
    )
    .unwrap();
    let git = |member: &str, args: &[&str]| {
        let output = std::process::Command::new("git")
            .args(["-c", "user.name=t", "-c", "user.email=t@t"])
            .args(["-c", "commit.gpgsign=false"])
            .args(args)
            .current_dir(root.join(member))
            .output()
            .unwrap();
        assert!(output.status.success(), "{output:?}");
    };
    let files = |decision: &Value| -> Vec<String> {
        let mut files: Vec<String> = decision["stop"]["findings"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|finding| finding["file"].as_str().map(str::to_owned))
            .collect();
        files.sort();
        files
    };
    for member in ["api", "web"] {
        fs::write(root.join(member).join("a.ts"), "export const a = 1\n").unwrap();
        git(member, &["init", "-q"]);
        git(member, &["add", "-A"]);
        git(member, &["commit", "-qm", "feat: start"]);
    }
    fs::write(root.join("web/old.ts"), "// TODO: before the session\n").unwrap();
    lifecycle(root, "session-start", json!({}));

    // One member's file left dirty, the other's committed in the turn; a plain folder's
    // file belongs to no repository.
    fs::write(
        root.join("api/a.ts"),
        "// FIXME: mine\nexport const a = 2\n",
    )
    .unwrap();
    fs::write(
        root.join("web/b.ts"),
        "// TODO: later\nexport const b = 1\n",
    )
    .unwrap();
    fs::write(root.join("notes/n.ts"), "// TODO: a note\n").unwrap();
    git("web", &["add", "b.ts"]);
    git("web", &["commit", "-qm", "Added b"]);
    let shell = json!({"tool": "shell", "command": "git -C web commit -m 'Added b'"});
    let found = written(root, shell);
    assert_eq!(found["standards"][0]["standard"], "commits", "{found}");

    let decision = lifecycle(root, "turn-end", json!({}));
    assert_eq!(files(&decision), ["api/a.ts", "web/b.ts"], "{decision}");
    // Said once: the next stop without a change is quiet, and a later one names only the new.
    let quiet = lifecycle(root, "turn-end", json!({}));
    assert!(quiet.get("stop").is_none(), "{quiet}");
    fs::write(root.join("web/a.ts"), "// HACK: next\nexport const a = 3\n").unwrap();
    let decision = lifecycle(root, "turn-end", json!({}));
    assert_eq!(files(&decision), ["web/a.ts"], "{decision}");
}

/// With no configuration at all, a workspace is held at its stops and commits to what its
/// member repositories state.
#[test]
fn an_undeclared_workspace_is_held_to_its_members_formats_at_commit_and_stop() {
    let top = std::env::temp_dir().join(format!("craft-host-undeclared-{}", std::process::id()));
    let _ = fs::remove_dir_all(&top);
    fs::create_dir_all(&top).unwrap();
    let fixture = Scratch(fs::canonicalize(&top).unwrap());
    let root = &fixture.0;
    let git = |member: &str, args: &[&str]| {
        let output = std::process::Command::new("git")
            .args(["-c", "user.name=t", "-c", "user.email=t@t"])
            .args(["-c", "commit.gpgsign=false"])
            .args(args)
            .current_dir(root.join(member))
            .output()
            .unwrap();
        assert!(output.status.success(), "{output:?}");
    };
    let log = "# Changelog\n\nThe format is based on Keep a Changelog.\n\n## [Unreleased]\n";
    for member in ["api", "web"] {
        fs::create_dir_all(root.join(member)).unwrap();
        let rules = "# Rules\n\nCommit messages follow Conventional Commits.\n";
        fs::write(root.join(member).join("AGENTS.md"), rules).unwrap();
        git(member, &["init", "-q"]);
    }
    fs::write(root.join("api/CHANGELOG.md"), log).unwrap();
    for member in ["api", "web"] {
        git(member, &["add", "-A"]);
        git(member, &["commit", "-qm", "feat: start"]);
    }
    let session = json!({"projectDirectory": root.to_string_lossy()});
    lifecycle(root, "session-start", session.clone());

    fs::write(root.join("web/b.ts"), "export const b = 1\n").unwrap();
    git("web", &["add", "b.ts"]);
    git("web", &["commit", "-qm", "Added b"]);
    let mut shell = json!({"tool": "shell", "command": "cd web && git commit -m 'Added b'"});
    shell["projectDirectory"] = session["projectDirectory"].clone();
    let found = written(root, shell);
    assert_eq!(
        found["standards"][0]["anchor"], "conventional-commits",
        "{found}"
    );

    // A shell command rewrote one member's changelog out of its format: no edit hook saw it.
    fs::write(
        root.join("api/CHANGELOG.md"),
        log.replace("[Unreleased]", "Next"),
    )
    .unwrap();
    fs::write(root.join("api/README.md"), "# Api\n\n## Install\n").unwrap();
    let decision = lifecycle(root, "turn-end", session);
    let files: Vec<&str> = decision["stop"]["findings"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|finding| finding["file"].as_str())
        .collect();
    assert_eq!(files, ["api/CHANGELOG.md"], "{decision}");
    assert!(!root.join("api/.craft").exists() && !root.join("web/.craft").exists());
}

/// A payload over the input bound (a large file read, a long patch) is not read. A host would
/// show a failing hook as an error, so where no policy applies the hook stays silent; before a
/// shell command, or a tool the input does not name, the safety policy denies what it could
/// not read.
#[test]
fn input_too_large_to_read_is_silent_unless_the_safety_policy_must_judge_a_command() {
    let fixture = project("unread-large", None);
    let off = project(
        "unread-large-off",
        Some(r#"{"formatVersion":1,"safety":{"enabled":false}}"#),
    );
    let run = |lifecycle: &str, payload: String| {
        let args: Vec<String> = [
            "claude",
            lifecycle,
            "--package",
            "craft-delivery",
            "--handler",
            "deliver",
        ]
        .iter()
        .map(|a| a.to_string())
        .collect();
        craft_application::hook::run(&args, &mut payload.as_bytes()).0
    };
    let large = "x".repeat(1024 * 1024);
    // In the host's order: where and which tool first, then the tool's input and result.
    let payload = |root: &Path, tool: Option<&str>, text: &str| {
        let named = tool.map_or_else(String::new, |tool| format!(r#""tool_name":"{tool}","#));
        format!(
            r#"{{"session_id":"S-1","cwd":{},{named}"tool_input":{},"tool_response":{}}}"#,
            json!(root.to_string_lossy()),
            json!({"command": text, "file_path": "a.txt"}),
            json!({"stdout": text}),
        )
    };
    // A large read result after the tool ran: nothing to judge, nothing said.
    let read = run("after-tool", payload(&fixture.0, Some("Read"), &large));
    assert_eq!(
        (read.exit_code, read.stdout.as_str(), read.stderr.as_str()),
        (0, "", "")
    );
    // Before a write the policy does not judge: silent too.
    let write = run("before-tool", payload(&fixture.0, Some("Write"), &large));
    assert_eq!((write.exit_code, write.stderr.as_str()), (0, ""));
    // Before a shell command, or a tool the input does not name: denied with the reason, which
    // Claude Code takes as exit code 2 and the reason on stderr.
    for tool in [Some("Bash"), None] {
        let shell = run("before-tool", payload(&fixture.0, tool, &large));
        assert_eq!(shell.exit_code, 2, "{tool:?}: {}", shell.stderr);
        assert!(
            shell.stderr.contains("could not be read in full"),
            "{}",
            shell.stderr
        );
    }
    // With the policy off there is nothing to enforce. The project is the one the input names,
    // also when its directory is written with escapes (as on Windows), not the hook's own.
    let free = run("before-tool", payload(&off.0, Some("Bash"), &large));
    assert_eq!((free.exit_code, free.stderr.as_str()), (0, ""));
    let escaped = payload(&off.0, Some("Bash"), &large).replacen(
        &json!(off.0.to_string_lossy()).to_string(),
        &json!(off.0.to_string_lossy())
            .to_string()
            .replace('/', "\\/"),
        1,
    );
    assert!(escaped.contains("\\/"), "the fixture escapes the directory");
    let free = run("before-tool", escaped);
    assert_eq!((free.exit_code, free.stderr.as_str()), (0, ""));
    // Under the bound a large payload is read as any other.
    let read = run(
        "after-tool",
        payload(&fixture.0, Some("Read"), &"x".repeat(200 * 1024)),
    );
    assert_eq!((read.exit_code, read.stderr.as_str()), (0, ""));
}
