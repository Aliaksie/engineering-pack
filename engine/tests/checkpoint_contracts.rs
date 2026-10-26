//! Replay of the frozen corpus (`tests/fixtures/runtime/checkpoint-contracts.json`)
//! captured from the JS engine. Error prose is not a contract; `acceptance:` reasons are.
mod support;

use serde_json::{Map, Value};
use support::run;

/// Intentional corrections over the captured JavaScript, by case ID; each now fails.
/// `Date.parse` rolled impossible calendar dates over, so `2026-02-30` was accepted.
const CORRECTIONS: &[&str] = &[
    "engineering-contract-validate:context.sources.0.observedAt=bad-date",
    "engineering-contract-validate:derivation.observedAt=bad-date",
    "acceptance-evidence-validate:records.0.observedAt=bad-date",
    "activity-validate:featureOutcomes.routing.at=bad-date",
    "activity-validate:featureOutcomes.lookup.at=bad-date",
];

fn expand(value: &Value, documents: &Value) -> Value {
    let Some(patch) = value.get("$mutate") else {
        return value.clone();
    };
    let mut document = documents[patch["base"].as_str().unwrap()].clone();
    let at = patch["at"].as_array().unwrap();
    let mut target = &mut document;
    for key in &at[..at.len() - 1] {
        target = match key {
            Value::Number(index) => &mut target[index.as_u64().unwrap() as usize],
            key => &mut target[key.as_str().unwrap()],
        };
    }
    match (&at[at.len() - 1], patch.get("value")) {
        (Value::Number(index), None) => {
            target
                .as_array_mut()
                .unwrap()
                .remove(index.as_u64().unwrap() as usize);
        }
        (Value::Number(index), Some(value)) => {
            target[index.as_u64().unwrap() as usize] = value.clone()
        }
        (key, None) => {
            target
                .as_object_mut()
                .unwrap()
                .shift_remove(key.as_str().unwrap());
        }
        (key, Some(value)) => target[key.as_str().unwrap()] = value.clone(),
    }
    document
}

#[test]
fn frozen_checkpoint_and_session_contracts() {
    let corpus: Value = serde_json::from_str(include_str!(
        "../../tests/fixtures/runtime/checkpoint-contracts.json"
    ))
    .unwrap();
    let mut failures = Vec::new();
    let mut corrected = 0;
    for case in corpus["cases"].as_array().unwrap() {
        let id = case["id"].as_str().unwrap();
        let operation = case["request"]["operation"].as_str().unwrap();
        let input: Map<String, Value> = case["request"]["input"]
            .as_object()
            .unwrap()
            .iter()
            .map(|(k, v)| (k.clone(), expand(v, &corpus["documents"])))
            .collect();
        let expected = &case["response"];
        let actual = run(operation, &Value::Object(input));
        if CORRECTIONS.contains(&id) {
            corrected += 1;
            if actual.is_ok() {
                failures.push(format!("{id}: correction not applied"));
            }
            continue;
        }
        let matches = match (&actual, expected["ok"].as_bool().unwrap()) {
            (Ok(result), true) => expected.get("result").is_none_or(|e| e == result),
            (Err(message), false) => {
                let reason = expected["error"]["message"].as_str().unwrap();
                !reason.starts_with("acceptance:") || reason == message
            }
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
        corpus["cases"].as_array().unwrap().len(),
        failures.join("\n")
    );
}
