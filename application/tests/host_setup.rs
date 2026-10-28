//! CLI-to-settings and native-observation-to-session acceptance checks.
use craft_application::{cli, statusline};
use serde_json::{Value, json};
use std::{
    fs,
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};
struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "craft-host-setup-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(path.join("bundle/bin")).unwrap();
        fs::write(path.join("bundle/bin/craft"), "launcher").unwrap();
        fs::create_dir_all(path.join("project")).unwrap();
        Self(path)
    }
    fn root(&self) -> PathBuf {
        self.0.join("project")
    }
    fn run(&self, command: &str, args: &[&str]) -> craft_application::Outcome {
        let mut argv = vec![
            command.into(),
            "--root".into(),
            self.root().to_str().unwrap().into(),
            "--bundle".into(),
            self.0.join("bundle").to_str().unwrap().into(),
        ];
        if command == "statusline" {
            // This entry point takes the host before its options.
            argv[0] = args[0].into();
            argv.extend(args[1..].iter().map(|s| s.to_string()));
            return statusline::run(&argv, &mut std::io::empty(), false);
        }
        argv.extend(args.iter().map(|s| s.to_string()));
        if command == "host-setup" {
            let output = std::process::Command::new(env!("CARGO_BIN_EXE_craft"))
                .args(&argv)
                .env("CLAUDE_CONFIG_DIR", self.0.join("user-settings"))
                .output()
                .unwrap();
            return craft_application::Outcome {
                exit_code: output.status.code().unwrap_or(1),
                stdout: String::from_utf8(output.stdout).unwrap(),
                stderr: String::from_utf8(output.stderr).unwrap(),
                export: None,
            };
        }
        cli::run(&argv)
    }
    fn json(&self, command: &str, args: &[&str]) -> Value {
        let result = self.run(command, args);
        assert_eq!(result.exit_code, 0, "{}", result.stderr);
        serde_json::from_str(&result.stdout).unwrap()
    }
    fn data(&self, name: &str, value: &Value) -> PathBuf {
        let file = self.0.join(name);
        fs::write(&file, value.to_string()).unwrap();
        file
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn at(delta: i64) -> String {
    craft_engine::timestamp::format(
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis() as i64
            + delta,
    )
}
#[test]
#[cfg(not(windows))]
fn preview_apply_and_conflicts_preserve_consumer_configuration() {
    let f = Fixture::new();
    let args = ["--host", "claude", "--package", "craft-engineering"];
    let preview = f.json("host-setup", &args);
    assert!(
        !f.root().join(".claude").exists(),
        "preview must not configure the host"
    );
    assert_eq!(preview["permissions"]["applied"], false);
    fs::create_dir(f.root().join(".claude")).unwrap();
    let settings = f.root().join(".claude/settings.local.json");
    let existing = json!({"permissions":{"deny":["Bash(secret *)"]},"env":{"CUSTOM":"value"}});
    fs::write(&settings, existing.to_string()).unwrap();
    let applied = f.json(
        "host-setup",
        &[
            "--host",
            "claude",
            "--package",
            "craft-engineering",
            "--apply",
        ],
    );
    assert_eq!(
        applied["display"]["application"]["status"],
        "configured-unverified"
    );
    let actual: Value = serde_json::from_slice(&fs::read(&settings).unwrap()).unwrap();
    assert_eq!(actual["permissions"], existing["permissions"]);
    assert_eq!(actual["env"], existing["env"]);
    assert_eq!(
        actual["statusLine"],
        preview["display"]["merge"]["statusLine"]
    );
    assert!(
        !actual["statusLine"]["command"]
            .as_str()
            .unwrap()
            .contains("--root"),
        "native worktree chooses storage"
    );
    assert_eq!(
        f.json(
            "host-setup",
            &[
                "--host",
                "claude",
                "--package",
                "craft-engineering",
                "--apply"
            ]
        )["display"]["application"]["status"],
        "already-configured"
    );
    fs::write(
        &settings,
        r#"{"statusLine":{"type":"command","command":"custom"},"keep":true}"#,
    )
    .unwrap();
    let before = fs::read(&settings).unwrap();
    assert_ne!(
        f.run(
            "host-setup",
            &[
                "--host",
                "claude",
                "--package",
                "craft-engineering",
                "--apply"
            ]
        )
        .exit_code,
        0
    );
    assert_eq!(fs::read(&settings).unwrap(), before);
}
#[test]
#[cfg(not(windows))]
fn inherited_displays_are_reported_and_preserved_with_native_precedence() {
    let f = Fixture::new();
    let user = f.0.join("user-settings/settings.json");
    let shared = f.root().join(".claude/settings.json");
    let local = f.root().join(".claude/settings.local.json");
    fs::create_dir_all(user.parent().unwrap()).unwrap();
    fs::create_dir_all(shared.parent().unwrap()).unwrap();
    let args = ["--host", "claude", "--package", "craft-engineering"];
    let apply = [
        "--host",
        "claude",
        "--package",
        "craft-engineering",
        "--apply",
    ];
    for (path, command) in [(&user, "user-display"), (&shared, "shared-display")] {
        let contents =
            json!({"statusLine":{"type":"command","command":command},"keep":true}).to_string();
        fs::write(path, &contents).unwrap();
        let preview = f.json("host-setup", &args);
        assert_eq!(
            preview["display"]["current"]["statusLine"]["command"],
            command
        );
        assert_eq!(
            fs::canonicalize(preview["display"]["current"]["source"].as_str().unwrap()).unwrap(),
            fs::canonicalize(path).unwrap()
        );
        assert_ne!(f.run("host-setup", &apply).exit_code, 0);
        assert!(!local.exists());
        assert_eq!(fs::read_to_string(path).unwrap(), contents);
    }
    // An effective Craft display is idempotent even with custom inherited values.
    let preview = f.json("host-setup", &args);
    fs::write(&local, preview["display"]["merge"].to_string()).unwrap();
    let before = fs::read(&local).unwrap();
    assert_eq!(
        f.json("host-setup", &apply)["display"]["application"]["status"],
        "already-configured"
    );
    assert_eq!(fs::read(&local).unwrap(), before);
    // An inherited Craft display needs no higher-precedence local override either.
    fs::remove_file(&local).unwrap();
    fs::write(&shared, preview["display"]["merge"].to_string()).unwrap();
    assert_eq!(
        f.json("host-setup", &apply)["display"]["application"]["status"],
        "already-configured"
    );
    assert!(!local.exists());
}

#[test]
fn permission_suggestions_are_scoped_and_never_applied() {
    let f = Fixture::new();
    for host in [
        "claude",
        "codex",
        "cursor",
        "copilot",
        "gemini",
        "gemini-extension",
        "jetbrains",
        "agents",
    ] {
        let preview = f.json(
            "host-setup",
            &["--host", host, "--package", "craft-engineering"],
        );
        assert_eq!(preview["connection"], "unverified");
        assert_eq!(preview["compaction"]["control"], "not-connected");
        let permissions = f.json("permissions", &["--host", host]);
        let commands = permissions["commands"].as_array().unwrap();
        for excluded in [
            "host-setup",
            "checkpoint-save",
            "operation",
            "project-evidence",
            "plan-integrate",
            "task-step",
        ] {
            assert!(!commands.contains(&json!(excluded)));
        }
        assert_eq!(permissions["applied"], false);
        assert_ne!(
            f.run("permissions", &["--host", host, "--apply"]).exit_code,
            0
        );
        if host != "claude" {
            assert_ne!(
                f.run(
                    "host-setup",
                    &["--host", host, "--package", "craft-engineering", "--apply"]
                )
                .exit_code,
                0
            );
        }
    }
    assert!(!f.root().join(".claude").exists());
    assert!(!f.root().join(".codex").exists());
}
#[test]
fn forwarded_native_events_feed_session_and_compaction_invalidates_usage() {
    let f = Fixture::new();
    let event = json!({"method":"thread/tokenUsage/updated","params":{"threadId":"s1","turnId":"t1","tokenUsage":{
        "last":{"inputTokens":100,"cachedInputTokens":80},"total":{"inputTokens":1000,"outputTokens":200,"totalTokens":1200},"modelContextWindow":1000}}});
    let data = f.data("native.json", &event);
    let time = at(-3000);
    f.json(
        "host-observe",
        &[
            "--host",
            "codex",
            "--session",
            "s1",
            "--data",
            data.to_str().unwrap(),
            "--observed-at",
            &time,
        ],
    );
    let status = f.json("session", &["--session", "s1", "--json"]);
    assert_eq!(status["usage"]["currentTokens"], 100);
    let declared = f.data("session.json", &json!({"sessionId":"s1"}));
    let from_json = f.json("session", &["--data", declared.to_str().unwrap(), "--json"]);
    assert_eq!(from_json["usage"], status["usage"]);
    assert_eq!(from_json["hostContext"], status["hostContext"]);
    let display = f.json(
        "statusline",
        &[
            "codex",
            "--package",
            "craft-engineering",
            "--data",
            declared.to_str().unwrap(),
            "--json",
        ],
    );
    assert_eq!(display["usage"], status["usage"]);
    assert_eq!(display["hostContext"], status["hostContext"]);
    assert_ne!(
        f.run(
            "session",
            &[
                "--session",
                "different",
                "--data",
                declared.to_str().unwrap(),
                "--json"
            ]
        )
        .exit_code,
        0
    );
    assert_eq!(status["usage"]["usedRatio"], 0.1);
    assert_eq!(status["host"]["reliableCompaction"], true);
    let text = f.run("session", &["--session", "s1"]);
    assert_eq!(text.exit_code, 0, "{}", text.stderr);
    assert!(
        text.stdout
            .contains("Consumed: 1000 input + 200 output = 1200 tokens")
    );

    assert_eq!(status["hostContext"]["consumption"]["totalTokens"], 1200);
    assert_eq!(
        f.json("session", &["--session", "different", "--json"])["usage"]["status"],
        "unknown"
    );
    assert_ne!(
        f.run(
            "host-observe",
            &[
                "--host",
                "codex",
                "--session",
                "different",
                "--data",
                data.to_str().unwrap(),
                "--observed-at",
                &time
            ]
        )
        .exit_code,
        0
    );
    assert_ne!(
        f.run(
            "host-observe",
            &[
                "--host",
                "codex",
                "--session",
                "s1",
                "--data",
                data.to_str().unwrap(),
                "--observed-at",
                &at(-180000)
            ]
        )
        .exit_code,
        0
    );
    assert_ne!(
        f.run(
            "host-observe",
            &[
                "--host",
                "codex",
                "--session",
                "s1",
                "--data",
                data.to_str().unwrap(),
                "--observed-at",
                &at(60000)
            ]
        )
        .exit_code,
        0
    );
    for (method, expected, delta) in [
        ("item/started", "started", -2000),
        ("item/completed", "completed", -1000),
    ] {
        let data=f.data("compact.json",&json!({"method":method,"params":{"threadId":"s1","turnId":"t1","item":{"type":"contextCompaction","id":"c1"}}}));
        f.json(
            "host-observe",
            &[
                "--host",
                "codex",
                "--session",
                "s1",
                "--data",
                data.to_str().unwrap(),
                "--observed-at",
                &at(delta),
            ],
        );
        let status = f.json("session", &["--session", "s1", "--json"]);
        assert_eq!(status["usage"]["status"], "unknown");
        assert_eq!(status["hostContext"]["compaction"]["status"], expected);
    }
}
#[test]
fn claude_live_payload_uses_its_worktree_and_persists_for_session_reads() {
    let f = Fixture::new();
    let cwd = f.root();
    let payload = json!({"cwd":cwd,"session_id":"s1","context_window":{"context_window_size":1000,"current_usage":{
        "input_tokens":10,"cache_creation_input_tokens":20,"cache_read_input_tokens":70}}});
    let args = vec![
        "claude".into(),
        "--package".into(),
        "craft-engineering".into(),
        "--bundle".into(),
        f.0.join("bundle").to_str().unwrap().into(),
        "--native-stdin".into(),
    ];
    let result = statusline::run(&args, &mut payload.to_string().as_bytes(), false);
    assert_eq!(result.exit_code, 0, "{}", result.stderr);
    assert!(result.stdout.contains("Context 100/1000 tokens"));
    let status = f.json("session", &["--session", "s1", "--json"]);
    assert_eq!(status["usage"]["currentTokens"], 100);
    assert!(cwd.join(".craft/state/sessions").exists());
    let missing = json!({"cwd":cwd,"session_id":"s1","context_window":{"context_window_size":1000,"current_usage":null}});
    let result = statusline::run(&args, &mut missing.to_string().as_bytes(), false);
    assert_eq!(result.exit_code, 0, "{}", result.stderr);
    assert_eq!(result.stdout, "Context usage unknown\n");
    assert_eq!(
        f.json("session", &["--session", "s1", "--json"])["usage"]["status"],
        "unknown"
    );
}
#[cfg(unix)]
#[test]
fn settings_symlinks_and_rule_metacharacters_are_not_followed_or_broadened() {
    use std::os::unix::fs::symlink;
    let f = Fixture::new();
    let outside = f.0.join("outside");
    fs::create_dir(&outside).unwrap();
    symlink(&outside, f.root().join(".claude")).unwrap();
    assert_ne!(
        f.run(
            "host-setup",
            &[
                "--host",
                "claude",
                "--package",
                "craft-engineering",
                "--apply"
            ]
        )
        .exit_code,
        0
    );
    assert!(!outside.join("settings.local.json").exists());
    fs::remove_file(f.root().join(".claude")).unwrap();
    fs::create_dir(f.root().join(".claude")).unwrap();
    let victim = f.data("victim.json", &json!({"keep":true}));
    symlink(&victim, f.root().join(".claude/settings.local.json")).unwrap();
    assert_ne!(
        f.run(
            "host-setup",
            &[
                "--host",
                "claude",
                "--package",
                "craft-engineering",
                "--apply"
            ]
        )
        .exit_code,
        0
    );
    assert_eq!(fs::read_to_string(&victim).unwrap(), "{\"keep\":true}");
    let wildcard = f.0.join("bundle[*]");
    fs::rename(f.0.join("bundle"), &wildcard).unwrap();
    let argv = [
        "permissions",
        "--host",
        "claude",
        "--root",
        f.root().to_str().unwrap(),
        "--bundle",
        wildcard.to_str().unwrap(),
    ]
    .map(str::to_owned);
    let result = cli::run(&argv);
    assert_eq!(result.exit_code, 0, "{}", result.stderr);
    let output: Value = serde_json::from_str(&result.stdout).unwrap();
    assert_eq!(output["status"], "unsupported");
    assert!(output.get("merge").is_none());
}
