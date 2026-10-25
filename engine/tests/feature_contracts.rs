//! Replay of the frozen W2g corpus (`tests/fixtures/runtime/feature-contracts.json`) captured
//! from the JS engine: feature outcome records and the feature report. Results, including the
//! updated state, compare as serialized JSON, so key order is part of the contract; error
//! messages reach stderr and compare exactly.
use craft_engine::{contracts, session};
use serde_json::{Value, json};

/// Intentional message corrections over the captured JavaScript, by case ID.
const CORRECTIONS: &[(&str, &str)] = &[
    // Local date-times and impossible calendar dates are not timestamps (W2a, W2e).
    ("record:at-local", "invalid feature outcome record"),
    ("record:at-impossible", "invalid feature outcome record"),
    ("record:saved-local-time", "invalid feature outcome record"),
    // JS crashed reading a missing state.
    (
        "feature-record:empty",
        "task feature outcomes require task-start",
    ),
];
/// JS crashed on a missing state; every Rust caller reports over a validated state record.
const UNREACHABLE: &[&str] = &["feature-report:empty"];

fn serialized(value: &Value) -> String {
    serde_json::to_string(value).unwrap()
}

fn replay(request: &Value) -> Result<Value, String> {
    let mut state = request["state"].clone();
    contracts::normalize_numbers(&mut state);
    match request["engine"].as_str().unwrap() {
        "feature-record" => {
            let source = match request.get("source") {
                Some(source) => source.clone(),
                None => session::source(&request["repository"]),
            };
            // An explicit non-string provenance is invalid, as in JS.
            let provenance = match request.get("provenance") {
                None => "agent-declared",
                Some(provenance) => provenance.as_str().unwrap_or_default(),
            };
            let recorded = session::record_feature(
                &mut state,
                &request["input"],
                &source,
                request["at"].as_str().unwrap_or_default(),
                provenance,
            )?;
            Ok(json!({"state": state, "recorded": recorded}))
        }
        "feature-report" => {
            session::feature_report(&state, &session::source(&request["repository"]))
        }
        other => panic!("unknown operation {other}"),
    }
}

#[test]
fn frozen_feature_contracts() {
    let corpus: Value = serde_json::from_str(include_str!(
        "../../tests/fixtures/runtime/feature-contracts.json"
    ))
    .unwrap();
    let cases = corpus["cases"].as_array().unwrap();
    let mut failures = Vec::new();
    let mut corrected = 0;
    for case in cases {
        let id = case["id"].as_str().unwrap();
        if UNREACHABLE.contains(&id) {
            continue;
        }
        let expected = &case["response"];
        let actual = replay(&case["request"]);
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
            let shown = match &actual {
                Ok(result) => serialized(result),
                Err(message) => format!("error {message}"),
            };
            failures.push(format!("{id}:\n  expected {expected}\n  actual   {shown}"));
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
