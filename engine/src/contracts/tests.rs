use super::*;
use crate::values::normalize_numbers;
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
fn the_safety_policy_is_on_until_a_layer_or_the_project_turns_it_off_and_no_task_can() {
    let resolved = |project: Value| effective_settings(&project, &json!({}), &json!({})).unwrap();
    let shipped = crate::safety::layered_listing(&resolved(json!({"formatVersion": 1})));
    assert_eq!(
        (&shipped["enabled"], &shipped["origin"]),
        (&json!(true), &json!("bundled"))
    );
    assert_eq!(shipped["rules"].as_array().map(Vec::len), Some(5));

    let narrowed = resolved(json!({"formatVersion": 1, "safety": {"disables": ["force-push"]}}));
    assert_eq!(narrowed["settings"]["safety"]["enabled"], true);

    let off = crate::safety::layered_listing(&resolved(
        json!({"formatVersion": 1, "safety": {"enabled": false}}),
    ));
    assert_eq!(
        (&off["enabled"], &off["origin"]),
        (&json!(false), &json!("project"))
    );

    let task = json!({"safety": {"enabled": false}});
    assert!(effective_settings(&json!({"formatVersion": 1}), &json!({}), &task).is_err());
    let user = effective_settings(&json!({"formatVersion": 1}), &task, &json!({})).unwrap();
    assert_eq!(user["origins"]["safety.enabled"], "user");
    assert_eq!(user["settings"]["safety"]["enabled"], false);
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
            "safety",
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

#[test]
fn project_context_limits_map_model_names_to_positive_token_counts() {
    let project = |limits: Value| json!({"formatVersion": 1, "execution": {"hosts": {"claude": {"contextLimits": limits}}}});
    let accepted = project(json!({"m": {"windowTokens": 1_000_000, "budgetTokens": 200_000}}));
    assert!(validate_project(&accepted).is_ok());
    for limits in [
        json!({"m": {}}),
        json!({"m": {"budgetTokens": 0}}),
        json!({"m": {"windowTokens": 1.5}}),
        json!({"m": {"maxTokens": 10}}),
        json!({"": {"windowTokens": 10}}),
        json!([]),
    ] {
        assert!(
            validate_project(&project(limits.clone())).is_err(),
            "{limits}"
        );
    }
}

#[test]
fn evidence_exclusions_are_normalized_component_prefixes() {
    let project = |exclude: Value| json!({"formatVersion": 1, "evidence": {"exclude": exclude}});
    let prefixes = evidence_exclusions(&project(json!(["samples/", "a/b", "samples"]))).unwrap();
    assert_eq!(prefixes, ["a/b", "samples"]);
    assert!(excluded("samples", &prefixes));
    assert!(excluded("samples/x/Y.java", &prefixes));
    assert!(!excluded("samples2/Y.java", &prefixes));
    assert!(!excluded("a/bc.js", &prefixes));
    assert!(
        evidence_exclusions(&json!({"formatVersion": 1}))
            .unwrap()
            .is_empty()
    );
    for exclude in [
        json!("samples"),
        json!(["/abs"]),
        json!(["../up"]),
        json!([""]),
        json!([1]),
        json!(vec!["x"; 65]),
    ] {
        assert!(
            validate_project(&project(exclude.clone())).is_err(),
            "{exclude}"
        );
    }
    assert!(validate_project(&json!({"formatVersion": 1, "evidence": {"include": []}})).is_err());
}

#[test]
fn evidence_is_enabled_only_by_an_explicit_boolean() {
    let project = |evidence: Value| json!({"formatVersion": 1, "evidence": evidence});
    assert!(evidence_enabled(&json!({"formatVersion": 1})).unwrap());
    assert!(evidence_enabled(&project(json!({"exclude": []}))).unwrap());
    assert!(evidence_enabled(&project(json!({"enabled": true}))).unwrap());
    assert!(!evidence_enabled(&project(json!({"enabled": false}))).unwrap());
    assert!(validate_project(&project(json!({"enabled": true, "exclude": ["a"]}))).is_ok());
    assert!(validate_project(&project(json!({"enabled": "yes"}))).is_err());
}

#[test]
fn a_schema_link_is_accepted_and_kept_out_of_settings() {
    let link = json!({"formatVersion": 1, "$schema": "../schemas/project.schema.json"});
    let settings = effective_settings(&link, &json!({}), &json!({})).unwrap();
    assert!(settings["settings"].get("$schema").is_none());
    assert!(settings["origins"].get("$schema").is_none());
    let wrong = json!({"formatVersion": 1, "$schema": 1});
    assert!(validate_project(&wrong).is_err());
}
