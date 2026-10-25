//! Replay of the frozen corpus (`tests/fixtures/runtime/review-contracts.json`) captured
//! from the JS read-only review guard: publishing-command recognition and the decision.
use craft_engine::review;
use serde_json::{Value, json};

#[test]
fn frozen_review_contracts() {
    let corpus: Value = serde_json::from_str(include_str!(
        "../../tests/fixtures/runtime/review-contracts.json"
    ))
    .unwrap();
    let mut failures = Vec::new();
    for case in corpus["publishes"].as_array().unwrap() {
        let command = case["command"].as_str().unwrap();
        if review::publishes(command) != case["publishes"].as_bool().unwrap() {
            failures.push(format!("publishes {command:?}"));
        }
    }
    for case in corpus["policy"].as_array().unwrap() {
        let settings = match case.get("readOnly") {
            Some(read_only) => json!({"review": {"readOnly": read_only}}),
            None => json!({}),
        };
        let command = case.get("command").cloned().unwrap_or(Value::Null);
        let actual = review::read_only_decision(&settings, case["tool"] == "shell", &command);
        if actual != case["response"] {
            failures.push(format!("policy {case}: {actual}"));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
