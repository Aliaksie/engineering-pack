use craft_engine::grounding;
use serde_json::Value;
#[test]
fn frozen_grounding_contracts() {
    let corpus: Value = serde_json::from_str(include_str!(
        "../../tests/fixtures/runtime/grounding-contracts.json"
    ))
    .unwrap();
    for case in corpus["cases"].as_array().unwrap() {
        let input = &case["request"]["input"];
        let result = if case["request"]["operation"] == "grounding-request" {
            grounding::request(input)
        } else {
            grounding::compose(input)
        };
        match result {
            Ok(value) => assert_eq!(value, case["response"]["result"], "{}", case["id"]),
            Err(error) => {
                assert_eq!(case["response"]["ok"], false, "{}: {error}", case["id"]);
            }
        }
    }
}
