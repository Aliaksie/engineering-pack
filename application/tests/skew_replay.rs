//! Version skew: a project, user file and state written by an earlier release, read by this
//! build. Its hooks and commands answer as usual, keep the earlier session's records and fail
//! nothing. Each fixture is what one release binary wrote in one short Claude session: hooks at
//! startup, prompt, edit, test run, compaction and turn end, a checkpoint and a commit-stage
//! check (`tests/fixtures/skew/README.md`).
use serde_json::{Value, json};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

const SESSION: &str = "skew-session-1";

fn fixture(release: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../tests/fixtures/skew")
        .join(release)
}

fn copy(from: &Path, to: &Path) {
    fs::create_dir_all(to).unwrap();
    for entry in fs::read_dir(from).unwrap().flatten() {
        let target = to.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy(&entry.path(), &target);
        } else {
            fs::copy(entry.path(), target).unwrap();
        }
    }
}

struct Upgraded {
    root: PathBuf,
    home: PathBuf,
}

impl Drop for Upgraded {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
        let _ = fs::remove_dir_all(&self.home);
    }
}

fn git(root: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args([
            "-c",
            "core.hooksPath=/dev/null",
            "-c",
            "commit.gpgsign=false",
        ])
        .args([
            "-c",
            "user.name=Fixture",
            "-c",
            "user.email=fixture@example.test",
        ])
        .args(args)
        .env("GIT_AUTHOR_DATE", "2026-11-01T00:00:00Z")
        .env("GIT_COMMITTER_DATE", "2026-11-01T00:00:00Z")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap().trim().to_string()
}

/// The checkout as `release` left it: the same commit, the uncommitted edit, its
/// `.craft/state` and the user's preferences.
fn upgraded(release: &str) -> Upgraded {
    let base = std::env::temp_dir().join(format!("craft-skew-{}-{release}", std::process::id()));
    let _ = fs::remove_dir_all(&base);
    fs::create_dir_all(base.join(".craft")).unwrap();
    // Without Windows' verbatim prefix, as a host names the directory.
    let canonical = fs::canonicalize(&base).unwrap();
    let root = PathBuf::from(
        canonical
            .to_str()
            .unwrap()
            .strip_prefix(r"\\?\")
            .unwrap_or(canonical.to_str().unwrap()),
    );
    let home = root.with_extension("home");
    let _ = fs::remove_dir_all(&home);
    fs::create_dir_all(home.join(".config/craft")).unwrap();
    let fixture = fixture(release);
    fs::copy(
        fixture.join("preferences.json"),
        home.join(".config/craft/preferences.json"),
    )
    .unwrap();
    git(&root, &["init", "-q", "-b", "main"]);
    fs::copy(
        fixture.join("project.json"),
        root.join(".craft/project.json"),
    )
    .unwrap();
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(root.join("src/lib.rs"), "pub fn a() {}\n").unwrap();
    git(&root, &["add", "-A"]);
    git(&root, &["commit", "-qm", "chore: start"]);
    fs::write(
        root.join("src/lib.rs"),
        "pub fn a() {}\n// TODO: finish b\npub fn b() {}\n",
    )
    .unwrap();
    copy(&fixture.join("state"), &root.join(".craft/state"));
    Upgraded { root, home }
}

/// `craft` as a host starts it in the project: exit code, stdout, stderr.
fn craft(at: &Upgraded, args: &[&str], stdin: Option<&Value>) -> (i32, String, String) {
    let mut child = Command::new(env!("CARGO_BIN_EXE_craft"))
        .args(args)
        .current_dir(&at.root)
        .env("HOME", &at.home)
        .env("USERPROFILE", &at.home)
        .env("CRAFT_ORGANIZATION_CONFIG", at.home.join("none.json"))
        .env("CRAFT_EVIDENCE_WARM", "0")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env_remove("CRAFT_PACKAGE_ROOT")
        .env_remove("CLAUDE_PROJECT_DIR")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut input = child.stdin.take().unwrap();
    if let Some(payload) = stdin {
        input.write_all(payload.to_string().as_bytes()).unwrap();
    }
    drop(input);
    let output = child.wait_with_output().unwrap();
    (
        output.status.code().unwrap_or(-1),
        String::from_utf8(output.stdout).unwrap(),
        String::from_utf8(output.stderr).unwrap(),
    )
}

/// One Claude hook of `package`; asserts a clean answer and returns it.
fn hook(at: &Upgraded, lifecycle: &str, package: &str, extra: Value) -> Value {
    let handler = if package == "craft-delivery" {
        "deliver"
    } else {
        "observe"
    };
    let mut payload = json!({"session_id": SESSION, "cwd": at.root});
    for (key, value) in extra.as_object().unwrap() {
        payload[key] = value.clone();
    }
    let args = [
        "hook",
        "claude",
        lifecycle,
        "--package",
        package,
        "--handler",
        handler,
        "--scope",
        "any",
    ];
    let (code, stdout, stderr) = craft(at, &args, Some(&payload));
    let label = format!("{lifecycle} {package}");
    assert_eq!((code, stderr.as_str()), (0, ""), "{label}: {stdout}");
    if stdout.trim().is_empty() {
        return Value::Null;
    }
    let answer: Value =
        serde_json::from_str(&stdout).unwrap_or_else(|_| panic!("{label}: {stdout}"));
    assert!(answer.is_object(), "{label}: {stdout}");
    answer
}

fn reasons(root: &Path, from: usize) -> Vec<String> {
    let events = fs::read_to_string(root.join(".craft/state/telemetry/events.jsonl")).unwrap();
    events
        .lines()
        .skip(from)
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .filter_map(|event| event["reasonCode"].as_str().map(str::to_owned))
        .collect()
}

#[test]
fn a_session_left_by_0_0_6_continues_on_this_build() {
    continues("0.0.6");
}

#[test]
fn a_session_left_by_0_0_7_continues_on_this_build() {
    continues("0.0.7");
}

fn continues(release: &str) {
    let at = upgraded(release);
    let checkpoint: Value = serde_json::from_str(
        &fs::read_to_string(
            fixture(release).join("state/sessions/99594efaa02c66347cbe7abf/checkpoint.json"),
        )
        .unwrap(),
    )
    .unwrap();
    assert_eq!(
        git(&at.root, &["rev-parse", "HEAD"]),
        checkpoint["head"],
        "replayed checkout"
    );
    let earlier = fs::read_to_string(at.root.join(".craft/state/telemetry/events.jsonl"))
        .unwrap()
        .lines()
        .count();

    let packages = ["craft-delivery", "craft-engineering"];
    let mut answers = Vec::new();
    for (lifecycle, extra) in [
        ("session-start", json!({"source": "resume"})),
        ("before-prompt", json!({"prompt": "finish b in src/lib.rs"})),
        (
            "before-tool",
            json!({"tool_name": "Bash", "tool_input": {"command": "git status --short"}}),
        ),
        (
            "after-tool",
            json!({"tool_name": "Edit", "tool_input": {"file_path": at.root.join("src/lib.rs")}, "tool_response": {}}),
        ),
        ("turn-end", json!({"stop_hook_active": false})),
    ] {
        for package in packages {
            answers.push((
                lifecycle,
                package,
                hook(&at, lifecycle, package, extra.clone()),
            ));
        }
    }
    let answer = |lifecycle: &str, package: &str| {
        answers
            .iter()
            .find(|(l, p, _)| *l == lifecycle && *p == package)
            .map(|(_, _, a)| a.clone())
            .unwrap()
    };
    // The resumed session restores the earlier checkpoint as unchanged, with its test run.
    let resumed =
        answer("session-start", "craft-delivery")["hookSpecificOutput"]["additionalContext"]
            .as_str()
            .unwrap()
            .to_string();
    assert!(resumed.contains(r#""changed":false"#), "{resumed}");
    assert!(
        resumed.contains(r#""tests":{"status":"passed""#),
        "{resumed}"
    );
    // A handoff saved with `checkpoint-save` comes back with it.
    if let Some(continuation) = checkpoint.get("continuation") {
        assert!(
            resumed.contains(&format!(r#""continuation":{continuation}"#)),
            "{resumed}"
        );
    }
    // The turn end continues the earlier summary record and does not raise again the stop
    // review the previous release already delivered.
    let ended = answer("turn-end", "craft-delivery");
    assert_eq!(
        ended,
        json!({"systemMessage": "Craft this session: 1 standards finding raised, 0 fixed \u{b7} 1 context restored after compaction"}),
    );
    for (lifecycle, package, answer) in &answers {
        if *package == "craft-engineering" {
            assert!(answer.is_null(), "{lifecycle}: {answer}");
        }
    }

    let bundle = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    let bundle = bundle.to_str().unwrap();
    let (code, stdout, stderr) = craft(
        &at,
        &[
            "session",
            "--session",
            SESSION,
            "--bundle",
            bundle,
            "--json",
        ],
        None,
    );
    assert_eq!((code, stderr.as_str()), (0, ""), "{stdout}");
    let session: Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(session["lastCheck"]["result"], "passed", "{session:#}");
    assert!(
        session["delivered"]["hookAnswers"].as_u64().unwrap() > 5,
        "{session:#}"
    );

    let (code, stdout, stderr) = craft(&at, &["settings", "--json"], None);
    assert_eq!((code, stderr.as_str()), (0, ""), "{stdout}");

    // Nothing the previous release wrote failed a hook or a record.
    assert_eq!(reasons(&at.root, earlier), Vec::<String>::new());
}
