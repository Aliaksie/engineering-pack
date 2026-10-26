use craft_engine::otlp;
use serde_json::Value;
#[test]
fn frozen_otlp_mapping_and_export_outcomes() {
    let corpus: Value = serde_json::from_str(include_str!(
        "../../tests/fixtures/runtime/otlp-contracts.json"
    ))
    .unwrap();
    for case in corpus["traces"].as_array().unwrap() {
        let events = case["events"].as_array().unwrap();
        // Compare serialized text: key order is part of the wire contract.
        assert_eq!(
            otlp::traces(events).unwrap().to_string(),
            case["expected"].to_string(),
            "{}",
            case["id"]
        );
    }
    for case in corpus["outcomes"].as_array().unwrap() {
        assert_eq!(
            otlp::outcome(&case["result"]).to_string(),
            case["expected"].to_string(),
            "{}: {}",
            case["id"],
            case["result"]
        );
    }
}
