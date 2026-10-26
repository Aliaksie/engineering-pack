//! Replay of the frozen corpus (`tests/fixtures/runtime/attempt-contracts.json`) captured
//! from the JS engine: receipt validation and identity, and append walks whose every step
//! records the saved attempt IDs and counters, or the refusal.
use craft_engine::session;
use serde_json::{Value, json};

fn view(state: &Value) -> Value {
    let ids: Vec<&Value> = state["attempts"]
        .as_array()
        .unwrap()
        .iter()
        .map(|record| &record["id"])
        .collect();
    json!({"attempts": ids, "activity": state["activity"]})
}

#[test]
fn frozen_attempt_contracts() {
    let corpus: Value = serde_json::from_str(include_str!(
        "../../tests/fixtures/runtime/attempt-contracts.json"
    ))
    .unwrap();
    let mut failures = Vec::new();
    for case in corpus["cases"].as_array().unwrap() {
        let id = case["id"].as_str().unwrap();
        let input = &case["request"]["input"];
        let expected = &case["response"];
        let actual = match case["request"]["operation"].as_str().unwrap() {
            "attempt-receipt" => session::attempt_receipt(
                &input["attempt"],
                &input["repository"],
                input["id"].as_str().unwrap_or_default(),
                input["at"].as_str().unwrap_or_default(),
            )
            .map_err(|message| json!({"message": message})),
            // Former engine-contract cases: one receipt appended to `state`.
            "attempt-append" if input.get("initial").is_none() => {
                let mut state = input["state"].clone();
                session::append_attempt(&mut state, input["receipt"].clone())
                    .map(|()| state)
                    .map_err(|message| json!({"message": message}))
            }
            "attempt-append" => {
                let mut state = input["initial"].clone();
                let steps = input["receipts"].as_array().unwrap().iter().map(|receipt| {
                    let mut next = state.clone();
                    match session::append_attempt(&mut next, receipt.clone()) {
                        Ok(()) => {
                            state = next;
                            view(&state)
                        }
                        Err(message) => json!({"error": message}),
                    }
                });
                Ok(Value::Array(steps.collect()))
            }
            other => panic!("unmapped operation {other}"),
        };
        let matches = match (&actual, expected["ok"].as_bool().unwrap()) {
            (Ok(result), true) => result == &expected["result"],
            // Former engine-contract cases froze only the error code, not the prose.
            (Err(error), false) => expected["error"]
                .get("message")
                .is_none_or(|message| message == &error["message"]),
            _ => false,
        };
        if !matches {
            failures.push(format!("{id}: expected {expected}, got {actual:?}"));
        }
    }
    assert!(
        failures.is_empty(),
        "{} of {} cases differ:\n{}",
        failures.len(),
        corpus["cases"].as_array().unwrap().len(),
        failures.join("\n")
    );
}
