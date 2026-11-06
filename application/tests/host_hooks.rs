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
