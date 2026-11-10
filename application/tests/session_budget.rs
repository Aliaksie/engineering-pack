//! What a long session's hooks inject stays bounded, and the user is told what Craft did:
//! an identical Project Intelligence answer is given once per context window, a spent window
//! gets none, and the turn's end shows one line of counts where the host displays it.
mod support;
use serde_json::{Value, json};
use std::path::Path;
use support::Repo;

const CARGO_ZERO: &str = "running 0 tests\n\ntest result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 3 filtered out; finished in 0.00s\n";
const CONFIG: &str = r#"{"formatVersion":1,
  "standards":[{"id":"commits","anchor":"conventional-commits"}]}"#;

fn hook(host: &str, lifecycle: &str, payload: &Value) -> String {
    let args: Vec<String> = [
        host,
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

fn project(name: &str, config: &str) -> Repo {
    let java = |name: &str, body: &str| format!("class {name} {{\n{body}\n}}\n");
    let repo = Repo::new(
        name,
        &[
            (
                "Policy.java",
                &java("Policy", "  long apply(long cents) { return cents; }"),
            ),
            (
                "Checkout.java",
                &java(
                    "Checkout",
                    "  long finish(Policy p) {\n    return p.apply(10);\n  }",
                ),
            ),
            (
                "Quote.java",
                &java(
                    "Quote",
                    "  long preview(Policy p) {\n    return p.apply(5);\n  }",
                ),
            ),
        ],
    );
    repo.write(".craft/project.json", config);
    repo.write(".craft/telemetry.json", r#"{"local":false}"#);
    repo
}

struct Session<'a> {
    repo: &'a Repo,
    host: &'a str,
    id: &'a str,
}

impl Session<'_> {
    fn at(&self, lifecycle: &str, extra: Value) -> String {
        let mut payload = json!({"session_id": self.id, "cwd": self.repo.path()});
        for (key, value) in extra.as_object().unwrap() {
            payload[key] = value.clone();
        }
        hook(self.host, lifecycle, &payload)
    }

    fn grep(&self, name: &str) -> String {
        self.at(
            "after-tool",
            json!({"tool_name": "Grep", "tool_input": {"pattern": name}}),
        )
    }

    /// The first answered search for `name`. A search answer may spend only a short index
    /// refresh, which a slow runner can exceed while the index is cold; the model's search
    /// then simply goes unanswered, and here it is run again.
    fn answered(&self, name: &str) -> String {
        self.at(
            "before-prompt",
            json!({"prompt": "Which methods call `Policy.apply`?"}),
        );
        for _ in 0..20 {
            let said = self.grep(name);
            if !said.is_empty() {
                return said;
            }
        }
        panic!("no search answer for {name}");
    }

    fn record(&self) -> Value {
        let file = craft_platform::state::file(
            Path::new(self.repo.path()),
            "delivered.json",
            false,
            Some(self.id),
        )
        .unwrap();
        std::fs::read_to_string(file)
            .map_or(Value::Null, |text| serde_json::from_str(&text).unwrap())
    }

    fn message(&self) -> Option<String> {
        let said = self.at("turn-end", json!({}));
        if said.is_empty() {
            return None;
        }
        let output: Value = serde_json::from_str(&said).unwrap();
        assert!(output.get("decision").is_none(), "stop held: {said}");
        output["systemMessage"].as_str().map(str::to_owned)
    }
}

#[test]
fn a_repeated_search_is_answered_once_per_context_window() {
    let repo = project("budget-repeat", CONFIG);
    let session = Session {
        repo: &repo,
        host: "claude",
        id: "S1",
    };
    session.at("session-start", json!({"source": "startup"}));
    let first = session.answered("apply");
    assert!(
        first.contains("Project Intelligence for the search just run"),
        "{first}"
    );
    let mut repeated = 0;
    for _ in 0..40 {
        repeated += session.grep("apply").len();
    }
    assert_eq!(repeated, 0, "the same answer was injected again");
    let record = session.record();
    assert!(record["answers"]["repeated"].as_u64().unwrap() >= 40);
    let delivered = record["answers"]["delivered"].as_u64().unwrap();
    // After a compaction the answer is no longer in context: it is given once more.
    session.at("before-compact", json!({"trigger": "auto"}));
    session.at("after-compact", json!({"source": "compact"}));
    assert_eq!(session.grep("apply"), first);
    assert_eq!(session.grep("apply"), "");
    let record = session.record();
    assert_eq!(record["answers"]["delivered"], delivered + 1);
    // Every hook answer stayed under the host's ceiling, and the total is on record.
    assert!(record["context"]["bytes"].as_u64().unwrap() > first.len() as u64 / 2);
    assert!(first.len() < 9_000);
}

#[test]
fn a_spent_answer_window_gets_no_further_answer() {
    let repo = project("budget-spent", CONFIG);
    let session = Session {
        repo: &repo,
        host: "claude",
        id: "S1",
    };
    // Warm the index in another session, so only the budget decides below.
    let warm = Session {
        repo: &repo,
        host: "claude",
        id: "S0",
    };
    warm.answered("apply");
    let file =
        craft_platform::state::file(Path::new(repo.path()), "delivered.json", true, Some("S1"))
            .unwrap();
    let spent = json!({"formatVersion": 1, "answers": {"delivered": 11, "bytes": 16384,
        "window": {"bytes": 16384, "keys": ["0123456789abcdef"]}}});
    std::fs::write(&file, spent.to_string()).unwrap();
    assert_eq!(session.grep("apply"), "");
    let record = session.record();
    assert_eq!(record["answers"]["overBudget"], 1);
    assert_eq!(record["answers"]["delivered"], 11);
    session.at("after-compact", json!({"source": "compact"}));
    assert!(session.grep("apply").contains("Project Intelligence"));
}

#[test]
fn the_turn_end_tells_the_user_what_craft_did_once_and_only_where_shown() {
    let repo = project("summary", CONFIG);
    let session = Session {
        repo: &repo,
        host: "claude",
        id: "S1",
    };
    session.at("session-start", json!({"source": "startup"}));
    assert_eq!(session.message(), None, "nothing happened yet");
    session.answered("apply");
    session.at(
        "after-tool",
        json!({"tool_name": "Bash", "tool_input": {"command": "cargo test nomatch"},
            "tool_response": {"stdout": CARGO_ZERO, "stderr": ""}}),
    );
    let commit = |message: &str| {
        repo.write("notes.txt", message);
        repo.git(&["add", "."]);
        repo.git(&["commit", "-qm", message]);
        session.at(
            "after-tool",
            json!({"tool_name": "Bash", "tool_input": {"command": format!("git commit -m '{message}'")},
                "tool_response": {"stdout": "", "stderr": ""}}),
        )
    };
    assert!(commit("bad message").contains("Project standards not met"));
    let lookups = session.record()["answers"]["delivered"].as_u64().unwrap();
    assert!(lookups >= 1);
    let plural = if lookups == 1 { "lookup" } else { "lookups" };
    let raised = session.message().expect("a summary line");
    assert_eq!(
        raised,
        format!(
            "Craft this session: 1 standards finding raised, 0 fixed · 1 test run that ran no test caught · {lookups} {plural} answered from the project index"
        )
    );
    assert_eq!(session.message(), None, "the same line is not shown twice");
    assert_eq!(commit("fix: follow the commit standard"), "");
    let fixed = session.message().expect("a changed line");
    assert!(
        fixed.contains("1 standards finding raised, 1 fixed"),
        "{fixed}"
    );
    // `craft session` reports the same counts and what was injected.
    let status = repo
        .cli(&[
            "session",
            "--session",
            "S1",
            "--bundle",
            repo.path(),
            "--json",
        ])
        .unwrap();
    let delivered = &status["delivered"];
    assert_eq!(delivered["summary"], fixed);
    assert_eq!(delivered["findingsFixed"], 1);
    assert_eq!(delivered["emptyTestRuns"], 1);
    assert!(delivered["contextBytes"].as_u64().unwrap() > 0);
    // A host with no message for the user at stop gets nothing there.
    let cursor = Session {
        repo: &repo,
        host: "cursor",
        id: "S2",
    };
    cursor.at(
        "after-tool",
        json!({"tool_name": "Shell", "tool_input": {"command": "cargo test nomatch"},
            "tool_response": {"stdout": CARGO_ZERO, "stderr": ""}}),
    );
    assert_eq!(cursor.record()["tests"]["empty"], 1);
    assert_eq!(cursor.at("turn-end", json!({})), "");
}

#[test]
fn the_summary_switch_shows_no_line_and_keeps_the_counts() {
    let config = r#"{"formatVersion":1,"routing":{"sessionSummary":false}}"#;
    let repo = project("summary-off", config);
    let session = Session {
        repo: &repo,
        host: "codex",
        id: "S1",
    };
    session.at(
        "after-tool",
        json!({"tool_name": "Bash", "tool_input": {"command": "cargo test nomatch"},
            "tool_response": CARGO_ZERO}),
    );
    assert_eq!(session.record()["tests"]["empty"], 1);
    assert_eq!(session.at("turn-end", json!({})), "");
}
