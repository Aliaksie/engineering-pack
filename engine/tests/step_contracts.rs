//! Replay of the frozen corpus (`tests/fixtures/runtime/step-contracts.json`) captured
//! from the JS engine: the `task-step` receipt and the action offer decisions. Results,
//! including the updated state, compare as serialized JSON, so key order is part of the
//! contract; error messages reach stderr and compare exactly.
//!
//! Edited in the corpus itself: `step:evidence-invalid` probes an unsupported ledger version
//! and names version 3 since ledger format 2 (test counts per record) became valid.
use craft_engine::actions::{self, Context};
use craft_engine::{session, values};
use serde_json::{Value, json};

/// Intentional message corrections over the captured JavaScript, by case ID.
/// The next step is named as a reader knows it, not by its internal field.
const NEXT_STEP: &str = "task step next step must be 1..512 characters without control characters";
const CORRECTIONS: &[(&str, &str)] = &[
    ("step:nextStep-missing", NEXT_STEP),
    ("step:nextStep-null", NEXT_STEP),
    ("step:nextStep-empty", NEXT_STEP),
    ("step:nextStep-control", NEXT_STEP),
    ("step:nextStep-tab", NEXT_STEP),
    ("step:nextStep-long513", NEXT_STEP),
    ("step:nextStep-astral", NEXT_STEP),
    ("step:nextStep-number", NEXT_STEP),
];

fn serialized(value: &Value) -> String {
    serde_json::to_string(value).unwrap()
}

fn replay(request: &Value, settings: &Value) -> Result<Value, String> {
    let settings = &settings[request["settings"].as_str().unwrap_or("base")];
    let mut state = request["state"].clone();
    values::normalize_numbers(&mut state);
    let none = json!({});
    let context = Context {
        settings,
        repository: &request["repository"],
        status: &request["status"],
        facts: request.get("facts").unwrap_or(&none),
        session: request["session"].as_str(),
        now: request["now"].as_i64().unwrap_or(0),
    };
    let with_state = |state: Value, result: Value| {
        let mut response = json!({"state": state});
        for (key, value) in result.as_object().unwrap() {
            response[key] = value.clone();
        }
        response
    };
    match request["engine"].as_str().unwrap() {
        "task-step" => {
            let step = session::step::step(
                &mut state,
                &request["input"],
                &request["repository"],
                request["id"].as_str().unwrap_or_default(),
                request["at"].as_str().unwrap_or_default(),
            )?;
            Ok(
                json!({"state": state, "receipt": step.receipt, "continuation": step.continuation, "acceptance": step.acceptance}),
            )
        }
        "action-facts" => {
            actions::facts(&request["facts"]).map(|facts| Value::Object(facts.clone()))
        }
        "action-propose" => {
            let result = actions::propose(&mut state, &request["input"], &context)?;
            Ok(with_state(state, result))
        }
        "action-decide" => {
            let result = actions::decide(
                &mut state,
                &request["input"],
                settings,
                &request["repository"],
                context.now,
            )?;
            Ok(with_state(state, result))
        }
        "action-check" => actions::check(&state, &request["input"], &context),
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

/// An `acceptance:` failure compares by its reason, the message's first word; what follows
/// locates it. Other messages compare exactly.
fn same(message: &str, expected: &str) -> bool {
    if expected.starts_with("acceptance:") {
        craft_engine::session::acceptance::reason(message) == expected
    } else {
        message == expected
    }
}

#[test]
fn frozen_step_contracts() {
    let corpus: Value = serde_json::from_str(include_str!(
        "../../tests/fixtures/runtime/step-contracts.json"
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
            (Some((_, message)), Err(actual)) => {
                corrected += 1;
                actual == message
            }
            (Some(_), Ok(_)) => false,
            (None, Ok(result)) => {
                expected["ok"] == true && serialized(result) == serialized(&expected["result"])
            }
            (None, Err(message)) => {
                expected["ok"] == false
                    && same(message, expected["error"]["message"].as_str().unwrap())
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

/// A completed task needs no next step: its continuation names the hand-back. Unfinished work
/// still needs one, named as a reader knows it.
#[test]
fn a_completion_without_a_next_step_names_the_hand_back() {
    let state = json!({"formatVersion": 1, "activity": {}, "lookups": {}, "attempts": [],
        "taskRecord": {"id": "t", "goal": "Fix the value", "stage": "verify", "status": "working",
            "goalComplete": false, "files": [], "risks": [], "checkout": "refs/heads/main"}});
    let repository = json!({"gitAvailable": true, "head": "1111111111111111111111111111111111111111",
        "checkout": "refs/heads/main",
        "fingerprints": {"complete": true, "index": "i", "workingTree": "w"}});
    let input = |complete: bool| {
        json!({"stage": "verify", "target": "acceptance:value", "approach": "manual-check",
            "evidence": "checks/value", "outcome": "passed", "result": "Value checked",
            "complete": complete})
    };
    let mut done = state.clone();
    let step = session::step::step(
        &mut done,
        &input(true),
        &repository,
        "a1",
        "2026-11-19T00:00:00.000Z",
    )
    .unwrap();
    assert_eq!(
        step.continuation["nextStep"],
        "Return the outcome and its evidence."
    );
    assert_eq!(step.continuation["remainingWork"], "");
    assert_eq!(done["taskRecord"]["goalComplete"], true);
    let mut open = state.clone();
    let refused = session::step::step(
        &mut open,
        &input(false),
        &repository,
        "a1",
        "2026-11-19T00:00:00.000Z",
    );
    assert!(matches!(refused, Err(message) if message == NEXT_STEP));
}
