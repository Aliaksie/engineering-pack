use craft_engine::{
    actions::{self, Context},
    contracts,
};
use serde_json::{Value, json};
#[test]
fn frozen_action_contracts() {
    let corpus: Value = serde_json::from_str(include_str!(
        "../../tests/fixtures/runtime/action-contracts.json"
    ))
    .unwrap();
    let mut failures = Vec::new();
    for case in corpus["cases"].as_array().unwrap() {
        let mut a = case["input"].clone();
        contracts::normalize_numbers(&mut a);
        let context = Context {
            settings: &a["project"],
            repository: &a["repository"],
            status: &a["status"],
            facts: a.get("facts").unwrap_or(&Value::Null),
            session: a["session"].as_str(),
            now: a["now"].as_i64().unwrap_or(0),
        };
        let mut state = a["state"].clone();
        let result = match case["operation"].as_str().unwrap() {
            "action-claim" => actions::claim(&mut state, &a["input"], &context, &a["checkpoint"]),
            "action-finish" => actions::finish(
                &mut state,
                &a["id"],
                &a["key"],
                &a["generation"],
                &a["result"],
            ),
            "action-facts" => {
                actions::facts(a.get("facts").unwrap_or(&json!({}))).map(|f| json!(f))
            }
            "action-check" => actions::check(&state, &a["input"], &context),
            _ => unreachable!(),
        };
        let expected = &case["expected"];
        // Internal JS TypeErrors on incomplete engine-only inputs become domain errors.
        let correction = match case["id"].as_str().unwrap() {
            "33" | "37" | "41" | "45" => Some("worker execution uses the shared worker controller"),
            "legacy:action-finish:empty" => Some("unknown action execution"),
            _ => None,
        };
        let matches = match &result {
            Ok(value) => {
                expected["ok"] == true
                    && serde_json::to_string(value).unwrap()
                        == serde_json::to_string(&expected["result"]).unwrap()
            }
            Err(error) => {
                expected["ok"] == false
                    && error
                        == correction
                            .unwrap_or_else(|| expected["error"]["message"].as_str().unwrap())
            }
        };
        if !matches {
            failures.push(format!(
                "{} expected {} got {:?}",
                case["id"], expected, result
            ));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn corrupt_generations_cannot_overflow_or_lose_the_fence() {
    let id = "a".repeat(64);
    let key = json!("b".repeat(64));
    for generation in [
        json!(-1),
        json!(1.5),
        json!(9_007_199_254_740_991_u64),
        json!(u64::MAX),
    ] {
        let mut state = json!({"recommendations": {&id: {"action": "review", "executionReceipt": {"key": key, "state": "dispatching", "generation": generation}}}});
        let before = state.clone();
        assert_eq!(
            actions::finish(
                &mut state,
                &json!(id),
                &key,
                &generation,
                &json!({"state":"uncertain"})
            )
            .unwrap_err(),
            "invalid action generation"
        );
        assert_eq!(state, before);
    }
}
