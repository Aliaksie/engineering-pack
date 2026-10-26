//! Replay of the frozen acceptance-view corpus (`tests/fixtures/runtime/view-contracts.json`)
//! captured from the JS engine. Results compare as serialized JSON, so key order is part of
//! the contract; the `qa:`, `product:` and `acceptance:` reasons compare exactly.
use craft_engine::contracts::normalize_numbers;
use craft_engine::session::views::{self, Reader};
use serde_json::Value;

/// Intentional corrections over the captured JavaScript, by case ID, with the new reason.
/// A QA view without a contract was reported as invalid probes; it names the contract now.
const CORRECTIONS: &[(&str, &str)] = &[
    ("qa:contract-null", "qa:missing-contract"),
    ("qa:contract-missing", "qa:missing-contract"),
    ("qa:empty", "qa:missing-contract"),
];

/// The views take a boolean; JS checked the type of `blockers` after the documents.
fn run(view: &str, input: &Value) -> Result<Value, String> {
    let blockers = input.get("blockers");
    let result = match view {
        "qa" => views::qa(
            input.get("contract"),
            input.get("evidence"),
            input.get("current").unwrap_or(&Value::Null),
            input.get("probes"),
            blockers == Some(&Value::Bool(true)),
        ),
        reader => views::reader(
            if reader == "product" {
                Reader::Product
            } else {
                Reader::Architecture
            },
            input.get("contract"),
            input.get("evidence"),
            input.get("current").unwrap_or(&Value::Null),
            input.get("focus"),
            blockers == Some(&Value::Bool(true)),
        ),
    }?;
    if blockers.is_some_and(|b| !b.is_boolean()) {
        return Err("acceptance:invalid-value".into());
    }
    Ok(result)
}

#[test]
fn frozen_view_contracts() {
    let corpus: Value = serde_json::from_str(include_str!(
        "../../tests/fixtures/runtime/view-contracts.json"
    ))
    .unwrap();
    let cases = corpus["cases"].as_array().unwrap();
    let (mut failures, mut corrected) = (Vec::new(), 0);
    for case in cases {
        let id = case["id"].as_str().unwrap();
        let mut input = case["input"].clone();
        normalize_numbers(&mut input);
        let actual = run(case["view"].as_str().unwrap(), &input);
        let expected = &case["response"];
        let matches = if let Some((_, reason)) = CORRECTIONS.iter().find(|(c, _)| *c == id) {
            corrected += 1;
            actual.as_ref().err().map(String::as_str) == Some(*reason)
        } else if expected["ok"] == true {
            actual.as_ref().is_ok_and(|result| {
                serde_json::to_string(result).unwrap()
                    == serde_json::to_string(&expected["result"]).unwrap()
            })
        } else {
            actual.as_ref().err()
                == expected["error"]["message"]
                    .as_str()
                    .map(String::from)
                    .as_ref()
        };
        if !matches {
            failures.push(format!("{id}: expected {expected}, got {actual:?}"));
        }
    }
    assert_eq!(corrected, CORRECTIONS.len());
    assert_eq!(cases.len(), 157);
    assert!(
        failures.is_empty(),
        "{} of {} cases differ:\n{}",
        failures.len(),
        cases.len(),
        failures.join("\n")
    );
}
