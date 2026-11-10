//! Every hook output follows a switch: a session under the fully manual policy shows the model
//! nothing and keeps no session records, while the same session on bundled defaults does both.
mod support;
use serde_json::{Value, json};
use std::path::Path;
use support::Repo;

const CARGO_ZERO: &str = "running 0 tests\n\ntest result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 3 filtered out; finished in 0.00s\n";

const MANUAL: &str = r#"{"formatVersion":1,
  "evidence":{"enabled":false},
  "routing":{"promptHints":false,"editHints":false,"dedupeHints":false,"standardChecks":false,
    "stopReview":false,"debtMarkers":false,"testRuns":false,"workerStandards":false,
    "sessionRecords":false,"sessionSummary":false,"conventionProposals":false},
  "checkpoint":{"enabled":false},
  "efficiency":{"lookup":{"enabled":false,"reuse":false},"batching":{"enabled":false},
    "attempts":{"enabled":false},"advice":{"enabled":false,"nextStep":false},
    "review":{"mode":"manual"},"context":{"action":"manual"},
    "delegation":{"mode":"manual","maxWorkers":0}},
  "standards":[{"id":"commits","anchor":"conventional-commits"},
    {"id":"api","file":"docs/api.md","scope":{"intents":["implement"]}}]}"#;

/// Bundled defaults, with debt markers reported so the stop has something to review.
const DEFAULTS: &str = r#"{"formatVersion":1,
  "evidence":{"enabled":false},"routing":{"debtMarkers":true},
  "standards":[{"id":"commits","anchor":"conventional-commits"},
    {"id":"api","file":"docs/api.md","scope":{"intents":["implement"]}}]}"#;

fn hook(lifecycle: &str, payload: Value) -> String {
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
    let (wire, _) = craft_application::hook::run(&args, &mut payload.to_string().as_bytes());
    assert_eq!(wire.exit_code, 0, "{lifecycle}: {}", wire.stderr);
    wire.stdout
}

/// One working session: start, a change request, an edit, a test run that ran nothing, a
/// subagent, a commit, more debt and the turn's end. Returns what each event printed.
fn session(repo: &Repo) -> Vec<(&'static str, String)> {
    let at = |extra: Value| {
        let mut payload = json!({"session_id": "S1", "cwd": repo.path()});
        for (key, value) in extra.as_object().unwrap() {
            payload[key] = value.clone();
        }
        payload
    };
    let mut said = vec![(
        "session-start",
        hook("session-start", at(json!({"source": "startup"}))),
    )];
    said.push((
        "prompt",
        hook(
            "before-prompt",
            at(json!({"prompt": "Implement the discount rule in src/lib.rs"})),
        ),
    ));
    repo.write("src/lib.rs", "pub fn f() {}\n// TODO: rounding\n");
    said.push((
        "edit",
        hook(
            "after-tool",
            at(json!({"tool_name": "Edit", "tool_input": {"file_path": "src/lib.rs"}})),
        ),
    ));
    said.push((
        "test run",
        hook(
            "after-tool",
            at(
                json!({"tool_name": "Bash", "tool_input": {"command": "cargo test nomatch"},
                "tool_response": {"stdout": CARGO_ZERO, "stderr": ""}}),
            ),
        ),
    ));
    said.push(("subagent", hook("agent-start", at(json!({})))));
    repo.git(&["commit", "-qam", "bad message"]);
    said.push((
        "commit",
        hook(
            "after-tool",
            at(json!({"tool_name": "Bash", "tool_input": {"command": "git commit -am 'bad message'"},
                "tool_response": {"stdout": "", "stderr": ""}})),
        ),
    ));
    for (event, lifecycle) in [
        ("compact", "before-compact"),
        ("compacted", "after-compact"),
    ] {
        said.push((event, hook(lifecycle, at(json!({"trigger": "auto"})))));
    }
    // Debt the turn adds after the commit, for the stop to review.
    repo.write("src/more.rs", "// TODO: later\n");
    said.push(("stop", hook("turn-end", at(json!({})))));
    said
}

fn configured(name: &str, config: &str) -> Repo {
    let repo = Repo::new(
        name,
        &[
            ("src/lib.rs", "pub fn f() {}\n"),
            ("docs/api.md", "# API\n"),
        ],
    );
    repo.write(".craft/project.json", config);
    repo.write(".craft/telemetry.json", r#"{"local":false}"#);
    repo
}

#[test]
fn the_fully_manual_policy_shows_nothing_and_keeps_no_session_records() {
    let repo = configured("switches-off", MANUAL);
    for (event, said) in session(&repo) {
        assert_eq!(said, "", "{event} spoke under the manual policy");
    }
    let sessions = Path::new(repo.path()).join(".craft/state/sessions");
    let kept: Vec<_> = std::fs::read_dir(&sessions)
        .into_iter()
        .flatten()
        .flatten()
        .map(|entry| entry.path())
        .collect();
    assert!(kept.is_empty(), "session records kept: {kept:?}");
}

#[test]
fn the_same_session_on_bundled_defaults_records_and_speaks() {
    let repo = configured("switches-on", DEFAULTS);
    let said: std::collections::HashMap<_, _> = session(&repo).into_iter().collect();
    for event in ["session-start", "test run", "subagent", "commit", "stop"] {
        assert_ne!(said[event], "", "{event} said nothing on defaults");
    }
    let directory = craft_platform::state::location(Path::new(repo.path()), Some("S1"))
        .unwrap()
        .directory;
    for record in ["intent.json", "test-runs.json", "host-context.json"] {
        assert!(directory.join(record).is_file(), "{record} not kept");
    }
}

#[test]
fn without_session_records_a_compaction_still_retires_the_occupancy_the_status_line_kept() {
    let repo = configured("switches-host-context", MANUAL);
    let root = Path::new(repo.path());
    // What an explicit integration (the status line) keeps for the session.
    let file = craft_platform::state::file(root, "host-context.json", true, Some("S1")).unwrap();
    let kept = json!({"formatVersion": 1, "host": "claude", "sessionId": "S1",
        "observedAt": "2026-11-09T10:00:00.000Z",
        "usage": {"inputTokens": 150000, "contextWindow": 200000,
            "observedAt": "2026-11-09T10:00:00.000Z"}});
    std::fs::write(&file, kept.to_string()).unwrap();
    let payload = json!({"session_id": "S1", "cwd": repo.path(), "trigger": "auto"});
    assert_eq!(hook("before-compact", payload), "");
    let saved: Value = serde_json::from_slice(&std::fs::read(&file).unwrap()).unwrap();
    assert_eq!(saved["compaction"]["status"], "started", "{saved}");
    assert!(saved.get("usage").is_none(), "{saved}");
}
