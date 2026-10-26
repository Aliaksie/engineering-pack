use super::*;
use crate::workers;

const SETTINGS: &str = include_str!("../../../tests/fixtures/runtime/settings-contracts.json");

/// The settings operations of the JS engine, over the same JSON input.
fn evaluate(operation: &str, input: &Value) -> Option<std::result::Result<Value, String>> {
    let empty = json!({});
    let field = |name: &str| input.get(name).unwrap_or(&Value::Null);
    let defaulted = |name: &str| input.get(name).unwrap_or(&empty);
    Some(match operation {
        "validate-project" => validate_project(field("project")).map(|()| field("project").clone()),
        "effective-settings" => effective_settings(
            field("project"),
            defaulted("preferences"),
            defaulted("overrides"),
        ),
        "worker-budget" => {
            let options = defaulted("options");
            let session = options.get("session").and_then(Value::as_str);
            let now = options
                .get("now")
                .and_then(Value::as_i64)
                .unwrap_or(i64::MIN);
            validate_project(field("project"))
                .and_then(|()| workers::budget(field("project"), defaulted("facts"), session, now))
        }
        _ => return None,
    })
}

#[test]
fn frozen_settings_cases_replay() {
    let mut replayed = 0;
    let corpus: Value = serde_json::from_str(SETTINGS).unwrap();
    for case in corpus["cases"].as_array().unwrap() {
        let request = &case["request"];
        let Some(operation) = request.get("operation").and_then(Value::as_str) else {
            continue;
        };
        let mut input = request["input"].clone();
        normalize_numbers(&mut input);
        let Some(actual) = evaluate(operation, &input) else {
            continue;
        };
        let response = &case["response"];
        match (actual, response["ok"].as_bool().unwrap()) {
            (Ok(result), true) => assert_eq!(result, response["result"], "{}", case["id"]),
            (Err(_), false) => {
                assert_eq!(response["error"]["code"], "INVALID_INPUT", "{}", case["id"])
            }
            (actual, _) => panic!("{}: expected {response}, got {actual:?}", case["id"]),
        }
        replayed += 1;
    }
    assert_eq!(replayed, 133);
}

#[test]
fn result_key_order_matches_javascript() {
    let result = effective_settings(&json!({"formatVersion": 1}), &json!({}), &json!({})).unwrap();
    let keys: Vec<_> = result["settings"]
        .as_object()
        .unwrap()
        .keys()
        .cloned()
        .collect();
    assert_eq!(
        keys,
        [
            "routing",
            "checkpoint",
            "workspace",
            "efficiency",
            "execution",
            "formatVersion"
        ]
    );
    assert_eq!(
        result["origins"]
            .as_object()
            .unwrap()
            .keys()
            .next_back()
            .unwrap(),
        "formatVersion"
    );
    assert_eq!(
        serde_json::to_string(&result["settings"]["efficiency"]["context"]).unwrap(),
        r#"{"elevatedAt":0.6,"compactAt":0.75,"restartAt":0.85,"action":"recommend"}"#
    );
}

#[test]
fn contract_corrections_over_the_frozen_javascript() {
    let project = json!({"formatVersion": 1});
    let error = effective_settings(&project, &json!({}), &json!({"bogus": 1})).unwrap_err();
    assert_eq!(error, "unknown overrides field: bogus");

    // Contradictory counts fail even when the observation could never be fresh.
    let settings = &effective_settings(&project, &json!({}), &json!({})).unwrap()["settings"];
    let facts = json!({"capacity": 4, "occupiedSlots": 1, "taskWorkers": 1});
    assert!(workers::budget(settings, &facts, None, 0).is_err());

    // Non-string identities fail their type check rather than a coerced pattern.
    let binding = |field: &str, value: Value| {
        let mut git = json!({"provider": "git", "transport": "cli", "command": "git"});
        git[field] = value;
        validate_project(&json!({"formatVersion": 1, "services": {"git": git}})).unwrap_err()
    };
    assert_eq!(binding("host", json!(443)), "invalid binding host");
    assert_eq!(
        binding("repository", json!(["a/b"])),
        "invalid binding repository"
    );
}
