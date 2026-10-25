//! Replay of the frozen corpus (`tests/fixtures/runtime/task-contracts.json`) captured
//! from the JS engine `task-start` decision. Results compare as serialized JSON, so key order
//! is part of the contract; error messages reach stderr and compare exactly.
use craft_engine::{contracts, session};
use serde_json::{Value, json};

/// Intentional message corrections over the captured JavaScript, by case ID: JS printed a
/// non-string path with `String()`, Rust prints its JSON.
const CORRECTIONS: &[(&str, &str)] = &[(
    "task-start:file=object",
    "expected project-relative path: {}",
)];

/// Serialized text, so key order counts (`Value` equality ignores object key order).
fn serialized(value: &Value) -> String {
    serde_json::to_string(value).unwrap()
}

fn start(request: &Value) -> Result<Value, String> {
    let mut state = request["state"].clone();
    contracts::normalize_numbers(&mut state);
    let repository = &request["repository"];
    let intent = session::start(&mut state, &request["input"], repository)?;
    let context = session::context(&state, repository)?;
    Ok(json!({"state": state, "intent": intent, "context": context}))
}

#[test]
fn frozen_task_start_contracts() {
    let corpus: Value = serde_json::from_str(include_str!(
        "../../tests/fixtures/runtime/task-contracts.json"
    ))
    .unwrap();
    let cases = corpus["cases"].as_array().unwrap();
    let mut failures = Vec::new();
    let mut corrected = 0;
    for case in cases {
        let id = case["id"].as_str().unwrap();
        let expected = &case["response"];
        let actual = start(&case["request"]);
        let correction = CORRECTIONS.iter().find(|(case, _)| *case == id);
        let matches = match (&actual, expected["ok"].as_bool().unwrap()) {
            (Ok(result), true) => serialized(result) == serialized(&expected["result"]),
            (Err(message), false) => match correction {
                Some((_, corrected_message)) => {
                    corrected += 1;
                    message == corrected_message
                }
                None => message == expected["error"]["message"].as_str().unwrap(),
            },
            _ => false,
        };
        if !matches {
            failures.push(format!("{id}: expected {expected}, got {actual:?}"));
        }
    }
    assert_eq!(corrected, CORRECTIONS.len());
    assert!(
        failures.is_empty(),
        "{} of {} cases differ:\n{}",
        failures.len(),
        cases.len(),
        failures.join("\n")
    );
}
