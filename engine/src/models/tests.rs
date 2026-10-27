//! Replays `tests/fixtures/runtime/models-contracts.json`, captured from the JS use case.
use super::*;
use crate::{contracts::effective_settings, values::normalize_numbers};

const CORPUS: &str = include_str!("../../../tests/fixtures/runtime/models-contracts.json");

fn run(request: &Value) -> Result<Value> {
    let settings =
        effective_settings(&request["project"], &json!({}), &json!({}))?["settings"].take();
    advise(&settings, &request["input"])
}

#[test]
fn frozen_model_advice_cases_replay() {
    let corpus: Value = serde_json::from_str(CORPUS).unwrap();
    let cases = corpus["cases"].as_array().unwrap();
    for case in cases {
        let id = case["id"].as_str().unwrap();
        let mut request = case["request"].clone();
        normalize_numbers(&mut request);
        let response = &case["response"];
        match (run(&request), response["ok"].as_bool().unwrap()) {
            (Ok(result), true) => assert_eq!(
                serde_json::to_string(&result).unwrap(),
                serde_json::to_string(&response["result"]).unwrap(),
                "{id}"
            ),
            (Err(_), false) => {}
            (actual, _) => panic!("{id}: expected {response}, got {actual:?}"),
        }
    }
    assert_eq!(cases.len(), 83);
}

#[test]
fn blank_text_follows_javascript_trim() {
    let settings = effective_settings(&json!({"formatVersion": 1}), &json!({}), &json!({}))
        .unwrap()["settings"]
        .take();
    let input =
        |host: &str| json!({"host": host, "role": "implementer", "task": "T", "revision": "r"});
    assert!(advise(&settings, &input("\u{feff} ")).is_err());
    assert!(advise(&settings, &input("\u{85}")).is_ok());
}
