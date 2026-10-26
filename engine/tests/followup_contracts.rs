//! Replays `followup-contracts.json`: the follow-up statuses and signals the JavaScript inbox
//! derived from the same observations, with its recorded corrections.
use craft_engine::{
    review::followup::{Facts, check},
    timestamp,
};
use serde_json::{Value, json};
use std::collections::HashSet;

const CORPUS: &str = include_str!("../../tests/fixtures/runtime/followup-contracts.json");

/// Every field the JavaScript result has must match; native-only fields are not compared.
fn assert_within(expected: &Value, actual: &Value, at: &str) {
    match expected {
        Value::Object(fields) => {
            for (name, value) in fields {
                assert_within(value, &actual[name], &format!("{at}.{name}"));
            }
        }
        Value::Array(items) => {
            assert_eq!(items.len(), actual.as_array().map_or(0, Vec::len), "{at}");
            for (index, item) in items.iter().enumerate() {
                assert_within(item, &actual[index], &format!("{at}[{index}]"));
            }
        }
        _ => assert_eq!(expected, actual, "{at}"),
    }
}

#[test]
fn frozen_followup_cases_replay() {
    let corpus: Value = serde_json::from_str(CORPUS).unwrap();
    let reviewer = corpus["reviewer"].as_str().unwrap();
    let acknowledged = HashSet::new();
    let (mut replayed, mut corrected) = (0, 0);
    for case in corpus["cases"].as_array().unwrap() {
        let id = case["id"].as_str().unwrap();
        let facts = Facts {
            reviewer,
            now: timestamp::parse(case["now"].as_str().unwrap()).unwrap(),
            stale_days: case["staleDays"].as_i64().unwrap(),
            acknowledged: &acknowledged,
        };
        let response = json!({"data": {"repository": {"pullRequest": case["observation"]}}});
        let result = check(&response, &facts).unwrap();
        let actual = &result["pullRequests"][0];
        let correction = &case["correction"];
        if correction.is_null() {
            assert_eq!(result["complete"], true, "{id}");
            assert_within(&case["javascript"], actual, id);
        } else if let Some(reason) = correction["incomplete"].as_str() {
            assert_eq!(result["complete"], false, "{id}");
            assert_eq!(actual["incomplete"], reason, "{id}");
            assert_eq!(actual["status"], Value::Null, "{id}");
            corrected += 1;
        } else {
            assert_ne!(correction["status"], case["javascript"]["status"], "{id}");
            assert_eq!(actual["status"], correction["status"], "{id}");
            corrected += 1;
        }
        replayed += 1;
    }
    assert_eq!((replayed, corrected), (186, 9));
}
