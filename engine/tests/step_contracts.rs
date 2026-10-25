//! Replay of the frozen corpus (`tests/fixtures/runtime/step-contracts.json`) captured
//! from the JS engine: the `task-step` receipt and the action offer decisions. Results,
//! including the updated state, compare as serialized JSON, so key order is part of the
//! contract; error messages reach stderr and compare exactly.
use craft_engine::actions::{self, Context};
use craft_engine::{contracts, session};
use serde_json::{Value, json};

/// Intentional message corrections over the captured JavaScript, by case ID.
const CORRECTIONS: &[(&str, &str)] = &[];

fn serialized(value: &Value) -> String {
    serde_json::to_string(value).unwrap()
}

fn replay(request: &Value, settings: &Value) -> Result<Value, String> {
    let settings = &settings[request["settings"].as_str().unwrap_or("base")];
    let mut state = request["state"].clone();
    contracts::normalize_numbers(&mut state);
    let none = json!({});
    let context = Context {
        settings,
        repository: &request["repository"],
        status: &request["status"],
        facts: request.get("facts").unwrap_or(&none),
        session: request["session"].as_str(),
        now: request["now"].as_i64().unwrap_or(0),
    };
    let with_state = |state: Value, result: Value| {
        let mut response = json!({"state": state});
        for (key, value) in result.as_object().unwrap() {
            response[key] = value.clone();
        }
        response
    };
    match request["engine"].as_str().unwrap() {
        "task-step" => {
            let step = session::step::step(
                &mut state,
                &request["input"],
                &request["repository"],
                request["id"].as_str().unwrap_or_default(),
                request["at"].as_str().unwrap_or_default(),
            )?;
            Ok(
                json!({"state": state, "receipt": step.receipt, "continuation": step.continuation, "acceptance": step.acceptance}),
            )
        }
        "action-facts" => {
            actions::facts(&request["facts"]).map(|facts| Value::Object(facts.clone()))
        }
        "action-propose" => {
            let result = actions::propose(&mut state, &request["input"], &context)?;
            Ok(with_state(state, result))
        }
        "action-decide" => {
            let result = actions::decide(
                &mut state,
                &request["input"],
                settings,
                &request["repository"],
                context.now,
            )?;
            Ok(with_state(state, result))
        }
        "action-check" => actions::check(&state, &request["input"], &context),
        other => panic!("unknown operation {other}"),
    }
}

/// Where a result first departs from the captured response, with a little context.
fn divergence(expected: &Value, actual: &Result<Value, String>) -> String {
    let (expected, actual) = match actual {
        Ok(result) => (serialized(&expected["result"]), serialized(result)),
        Err(message) => (expected.to_string(), format!("error {message}")),
    };
    let at = expected
        .chars()
        .zip(actual.chars())
        .take_while(|(a, b)| a == b)
        .count();
    let window =
        |text: &str| -> String { text.chars().skip(at.saturating_sub(40)).take(120).collect() };
    format!(
        "\n  expected …{}\n  actual   …{}",
        window(&expected),
        window(&actual)
    )
}

#[test]
fn frozen_step_contracts() {
    let corpus: Value = serde_json::from_str(include_str!(
        "../../tests/fixtures/runtime/step-contracts.json"
    ))
    .unwrap();
    let cases = corpus["cases"].as_array().unwrap();
    let mut failures = Vec::new();
    let mut corrected = 0;
    for case in cases {
        let id = case["id"].as_str().unwrap();
        let expected = &case["response"];
        let actual = replay(&case["request"], &corpus["settings"]);
        let matches = match (CORRECTIONS.iter().find(|(case, _)| *case == id), &actual) {
            (Some((_, message)), Err(actual)) => {
                corrected += 1;
                actual == message
            }
            (Some(_), Ok(_)) => false,
            (None, Ok(result)) => {
                expected["ok"] == true && serialized(result) == serialized(&expected["result"])
            }
            (None, Err(message)) => {
                expected["ok"] == false && message == expected["error"]["message"].as_str().unwrap()
            }
        };
        if !matches {
            failures.push(format!("{id}: {}", divergence(expected, &actual)));
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
