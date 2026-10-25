//! Replay of the frozen corpus (`tests/fixtures/runtime/session-contracts.json`) captured
//! from the JS engine: session advice, attempt status, feature report, capability, the
//! observation plan and unified status. Results compare as serialized JSON, so key order is
//! part of the contract; error messages reach stderr and compare exactly.
use craft_engine::session::status::{self, Observations};
use craft_engine::{contracts, routing, session};
use serde_json::{Value, json};

/// Intentional corrections over the captured JavaScript, by case ID, with the corrected
/// result: a message, or a JSON pointer's serialized value.
/// - A previous notice time without `Z` or an offset was read in the machine's time zone;
///   like other non-ISO instants it is now invalid.
/// - A stage without a guidance observation (unreachable from the application) reported a
///   JavaScript `TypeError`.
const CORRECTIONS: &[(&str, Correction)] = &[
    (
        "advice:notice-local-at",
        Correction::Message("invalid previous suggestion"),
    ),
    (
        "status:pack-missing",
        Correction::Value(
            "/diagnostics",
            r#"["Pack selection unavailable: No guidance observation."]"#,
        ),
    ),
];

enum Correction {
    Message(&'static str),
    Value(&'static str, &'static str),
}

/// Serialized text, so key order counts (`Value` equality ignores object key order).
fn serialized(value: &Value) -> String {
    serde_json::to_string(value).unwrap()
}

fn text(value: &Value) -> Option<String> {
    value.as_str().map(str::to_string)
}

fn replay(request: &Value, settings: &Value) -> Result<Value, String> {
    let settings = &settings[request["settings"].as_str().unwrap_or("base")];
    let mut input = request["input"].clone();
    contracts::normalize_numbers(&mut input);
    let now = request["now"].as_i64().unwrap_or(0);
    match request["engine"].as_str().unwrap() {
        "session-advice" => status::advice(&input, settings, now),
        "attempt-status" => Ok(session::attempt_status(
            request["records"].as_array().unwrap(),
            request["operation"].as_str().unwrap(),
        )),
        "feature-report" => {
            session::feature_report(&request["state"], &session::source(&request["repository"]))
        }
        "capability" => {
            let risks: Vec<String> = request["risks"]
                .as_array()
                .unwrap()
                .iter()
                .map(|r| r.as_str().unwrap().to_string())
                .collect();
            Ok(routing::capability(
                request["operation"].as_str().unwrap(),
                &risks,
            ))
        }
        "session-observation-plan" => {
            let needs = status::needs(
                &input,
                settings,
                request.get("state"),
                request.get("saved").unwrap_or(&Value::Null),
            );
            let mut result = json!({});
            match needs.plan {
                Some(Ok(plan)) => result["plan"] = plan,
                Some(Err(error)) => result["packError"] = json!(error),
                None => {}
            }
            result["recover"] = json!(needs.recover);
            result["repository"] = json!(needs.repository);
            Ok(result)
        }
        "session-status" => {
            let observed = &request["observations"];
            let observations = Observations {
                state: observed.get("state").cloned(),
                state_error: text(&observed["stateError"]),
                saved: observed.get("saved").cloned().unwrap_or(Value::Null),
                recovered: observed.get("recovered").cloned(),
                checkpoint_error: text(&observed["checkpointError"]),
                repository: observed.get("repository").cloned(),
                pack: observed.get("pack").cloned(),
                plan: observed.get("plan").cloned(),
                pack_error: text(&observed["packError"]),
            };
            status::status(status::Request {
                input: &input,
                settings,
                session: request["session"].as_str(),
                task: request["task"].as_str(),
                now,
                observations: &observations,
            })
        }
        other => panic!("unknown operation {other}"),
    }
}

/// Where a result first departs from the captured response, with a little context.
fn divergence(expected: &Value, actual: &Result<Value, String>) -> String {
    let (expected, actual) = match actual {
        Ok(result) => (serialized(&expected["result"]), serialized(result)),
        Err(message) => (expected.to_string(), format!("error {message}")),
    };
    let at = expected
        .chars()
        .zip(actual.chars())
        .take_while(|(a, b)| a == b)
        .count();
    let window =
        |text: &str| -> String { text.chars().skip(at.saturating_sub(40)).take(120).collect() };
    format!(
        "\n  expected …{}\n  actual   …{}",
        window(&expected),
        window(&actual)
    )
}

#[test]
fn frozen_session_contracts() {
    let corpus: Value = serde_json::from_str(include_str!(
        "../../tests/fixtures/runtime/session-contracts.json"
    ))
    .unwrap();
    let cases = corpus["cases"].as_array().unwrap();
    let mut failures = Vec::new();
    let mut corrected = 0;
    for case in cases {
        let id = case["id"].as_str().unwrap();
        let expected = &case["response"];
        let actual = replay(&case["request"], &corpus["settings"]);
        let matches = match (CORRECTIONS.iter().find(|(case, _)| *case == id), &actual) {
            (Some((_, Correction::Message(message))), Err(actual)) => {
                corrected += 1;
                actual == message
            }
            (Some((_, Correction::Value(pointer, value))), Ok(actual)) => {
                corrected += 1;
                serialized(actual.pointer(pointer).unwrap_or(&Value::Null)) == *value
            }
            (Some(_), _) => false,
            (None, Ok(result)) => {
                expected["ok"] == true && serialized(result) == serialized(&expected["result"])
            }
            (None, Err(message)) => {
                expected["ok"] == false && message == expected["error"]["message"].as_str().unwrap()
            }
        };
        if !matches {
            failures.push(format!("{id}: {}", divergence(expected, &actual)));
        }
    }
    assert_eq!(corrected, CORRECTIONS.len());
    assert!(
        failures.is_empty(),
        "{} of {} cases differ:\n{}",
        failures.len(),
        cases.len(),
        failures.join("\n")
    );
}
