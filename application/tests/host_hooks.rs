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
fn observation_records_the_lifecycle_without_reading_the_project() {
    let fixture = project("observe", Some("INVALID"));
    let result = hook(facts(
        &fixture.0,
        "observe",
        json!({"package": "craft-engineering", "tool": "read", "lifecycle": "after-tool"}),
    ));
    assert_eq!(result["decision"], "allow");
    assert_eq!(result["failed"], false);
    assert_eq!(result["export"], Value::Null);
    assert_eq!(
        view(&events(&fixture.0)),
        [
            r#"host.hook<> success {"lifecycle":"after-tool","inputStatus":"complete","decision":"allow","contextSupported":true,"package":"craft-engineering","identityScope":"session"}"#,
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
    // A non-shell call to a shell-scoped hook records the hook only.
    assert_eq!(names.iter().filter(|n| *n == "project.loaded").count(), 3);
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
        hook(facts(&bare.0, "observe", json!({})));
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
        json!({"contextSupported": false}),
    ));
    assert_eq!(
        view(&events(&fixture.0))[0],
        r#"host.hook<> success {"lifecycle":"before-tool","inputStatus":"complete","decision":"allow","contextSupported":false,"package":"craft-review","identityScope":"session"}"#
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
