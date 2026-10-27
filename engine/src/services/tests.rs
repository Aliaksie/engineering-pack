//! Replays `tests/fixtures/runtime/services-contracts.json`, captured from the JS use cases.
use super::*;
use crate::{contracts::effective_settings, values::normalize_numbers};

const CORPUS: &str = include_str!("../../../tests/fixtures/runtime/services-contracts.json");

/// Intentional corrections over the captured JavaScript, by case ID.
fn correction(id: &str) -> Option<std::result::Result<Value, ()>> {
    let diff = |range: &str| {
        json!([
            "diff",
            "--no-ext-diff",
            "--no-textconv",
            "--no-color",
            range,
            "--"
        ])
    };
    Some(match id {
        // Built-in arguments were substituted again, so `{commit}` in a revision was a placeholder.
        "operation:git-diff-refspec-chars" => Ok(diff("v1.0^{commit}...feature/x@{1}")),
        // Placeholders inside text stringified null, objects and arrays.
        "operation:configured-null-parameter"
        | "operation:configured-object-partial"
        | "operation:configured-array-partial"
        // `String([5])` passed the change number check.
        | "operation:change-id-array"
        // Built-ins came from any binding with a matching provider, not the operation's service.
        | "operation:tasks-github-supplies-change"
        | "operation:codehost-git-supplies-git" => Err(()),
        _ => return None,
    })
}

fn run(request: &Value) -> std::result::Result<Value, String> {
    let settings =
        effective_settings(&request["project"], &json!({}), &json!({}))?["settings"].clone();
    match request["command"].as_str().unwrap() {
        "operation" => plan(
            &settings,
            request["operation"].as_str().unwrap(),
            &request["params"],
        ),
        _ => prepare(
            &settings,
            request.get("availableCommands"),
            request.get("availableMcpTools"),
        ),
    }
}

#[test]
fn frozen_service_cases_replay_with_recorded_corrections() {
    let corpus: Value = serde_json::from_str(CORPUS).unwrap();
    let cases = corpus["cases"].as_array().unwrap();
    let mut corrected = 0;
    for case in cases {
        let id = case["id"].as_str().unwrap();
        let mut request = case["request"].clone();
        normalize_numbers(&mut request);
        let actual = run(&request);
        let response = &case["response"];
        match correction(id) {
            Some(expected) => {
                corrected += 1;
                match (actual, expected) {
                    (Ok(result), Ok(args)) => assert_eq!(result["args"], args, "{id}"),
                    (Err(_), Err(())) => {}
                    (actual, _) => panic!("{id}: correction not applied, got {actual:?}"),
                }
            }
            None => match (actual, response["ok"].as_bool().unwrap()) {
                (Ok(result), true) => assert_eq!(result, response["result"], "{id}"),
                (Err(_), false) => {}
                (actual, _) => panic!("{id}: expected {response}, got {actual:?}"),
            },
        }
    }
    assert_eq!((cases.len(), corrected), (63, 7));
}

#[test]
fn numbers_render_as_javascript_strings() {
    for (value, text) in [
        (json!(42), "42"),
        (json!(-3), "-3"),
        (json!(2.5), "2.5"),
        (json!(0.1), "0.1"),
        (json!(1e21), "1e+21"),
        (json!(1.5e300), "1.5e+300"),
        (json!(123456789012345680000.0), "123456789012345680000"),
        (json!(0.000001), "0.000001"),
        (json!(1e-7), "1e-7"),
        (json!(-2.5e-8), "-2.5e-8"),
    ] {
        assert_eq!(scalar_text(&value).unwrap(), text, "{value}");
    }
}

#[test]
fn placeholders_follow_the_javascript_pattern() {
    let params = json!({"a": "x", "b1_": 2, "obj": {"k": 1}});
    let params = params.as_object().unwrap();
    let text = |template: &str| substitute(&json!(template), params);
    assert_eq!(text("{a}").unwrap(), json!("x"));
    assert_eq!(text("{obj}").unwrap(), json!({"k": 1}));
    assert_eq!(text("{b1_}").unwrap(), json!(2));
    assert_eq!(text("-{a}-{b1_}-").unwrap(), json!("-x-2-"));
    assert_eq!(
        text("{a{a}}{1a}{}{ a}λ{a}").unwrap(),
        json!("{ax}{1a}{}{ a}λx")
    );
    assert!(text("{a}{obj}").is_err());
    assert!(text("{missing}").is_err());
}
