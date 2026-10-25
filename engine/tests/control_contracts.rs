use craft_engine::control;
use serde_json::{Value, json};
#[test]
fn frozen_native_journal_validation() {
    let corpus: Value = serde_json::from_str(include_str!(
        "../../tests/fixtures/runtime/control-contracts.json"
    ))
    .unwrap();
    for case in corpus["cases"].as_array().unwrap() {
        let result = control::validate(
            &case["journal"],
            case["mode"].as_str().unwrap(),
            &json!("TREE"),
            &json!("claude"),
            &json!("coordinator"),
        );
        assert_eq!(
            result.is_ok(),
            case["valid"].as_bool().unwrap(),
            "{}: {:?}",
            case["id"],
            result
        );
    }
}
