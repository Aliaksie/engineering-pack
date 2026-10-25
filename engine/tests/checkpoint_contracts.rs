//! Replay of the frozen corpus (`tests/fixtures/runtime/checkpoint-contracts.json`)
//! captured from the JS engine. Error prose is not a contract; `acceptance:` reasons are.
use craft_engine::{checkpoint, hash, repository, session, session::acceptance};
use serde_json::{Map, Value, json};

/// Intentional corrections over the captured JavaScript, by case ID; each now fails.
/// `Date.parse` rolled impossible calendar dates over, so `2026-02-30` was accepted.
const CORRECTIONS: &[&str] = &[
    "engineering-contract-validate:context.sources.0.observedAt=bad-date",
    "engineering-contract-validate:derivation.observedAt=bad-date",
    "acceptance-evidence-validate:records.0.observedAt=bad-date",
    "activity-validate:featureOutcomes.routing.at=bad-date",
    "activity-validate:featureOutcomes.lookup.at=bad-date",
];

fn expand(value: &Value, documents: &Value) -> Value {
    let Some(patch) = value.get("$mutate") else {
        return value.clone();
    };
    let mut document = documents[patch["base"].as_str().unwrap()].clone();
    let at = patch["at"].as_array().unwrap();
    let mut target = &mut document;
    for key in &at[..at.len() - 1] {
        target = match key {
            Value::Number(index) => &mut target[index.as_u64().unwrap() as usize],
            key => &mut target[key.as_str().unwrap()],
        };
    }
    match (&at[at.len() - 1], patch.get("value")) {
        (Value::Number(index), None) => {
            target
                .as_array_mut()
                .unwrap()
                .remove(index.as_u64().unwrap() as usize);
        }
        (Value::Number(index), Some(value)) => {
            target[index.as_u64().unwrap() as usize] = value.clone()
        }
        (key, None) => {
            target
                .as_object_mut()
                .unwrap()
                .shift_remove(key.as_str().unwrap());
        }
        (key, Some(value)) => target[key.as_str().unwrap()] = value.clone(),
    }
    document
}

fn object(value: &Value) -> Result<&Map<String, Value>, String> {
    value
        .as_object()
        .ok_or_else(|| "expected an object".to_string())
}

fn truthy(value: &Value) -> bool {
    !matches!(value, Value::Null | Value::Bool(false))
        && value.as_str() != Some("")
        && value.as_f64() != Some(0.0)
}

fn run(operation: &str, input: &Value) -> Result<Value, String> {
    let done = |result: Result<(), String>| result.map(|()| Value::Null);
    match operation {
        "repository-scopes" => repository::scopes(&input["repositories"]).map(|s| json!(s)),
        "repository-snapshot" => repository::snapshot(
            input["repositories"]
                .as_array()
                .ok_or("repositories must be an array")?,
        ),
        "repository-checkout" => {
            let entries: Vec<(String, Value)> = input["repositories"]
                .as_array()
                .unwrap()
                .iter()
                .map(|e| {
                    (
                        e["path"].as_str().unwrap().to_string(),
                        e["checkout"].clone(),
                    )
                })
                .collect();
            Ok(json!(repository::checkout(&entries)))
        }
        "hash" => Ok(json!(hash(input["text"].as_str().unwrap().as_bytes()))),
        "checkpoint-metadata" => done(checkpoint::validate_metadata(object(&input["metadata"])?)),
        "checkpoint-state" => checkpoint::state(
            &input["previous"],
            object(&input["metadata"])?,
            &input["repository"],
            input["updated"].as_str().unwrap(),
        ),
        "checkpoint-compare" => Ok(checkpoint::compare(&input["saved"], &input["current"])),
        "checkpoint-unchanged" => Ok(json!(checkpoint::unchanged(
            &input["previous"],
            &input["state"]
        ))),
        "checkpoint-validate" => done(checkpoint::validate(&input["state"])),
        "engineering-contract-validate" => done(acceptance::validate_contract(&input["contract"])),
        "engineering-contract-digest" => {
            acceptance::contract_digest(&input["contract"]).map(|d| json!(d))
        }
        "task-context-validate" => done(acceptance::validate_context(&input["context"])),
        "acceptance-evidence-validate" => done(acceptance::validate_evidence(&input["evidence"])),
        "acceptance-coverage" => {
            // JS also type-checks `blockers` after the documents; Rust takes a `bool`.
            let blockers = input.get("blockers").unwrap_or(&json!(false));
            let result = acceptance::coverage(
                input.get("contract"),
                input.get("evidence"),
                &input["current"],
                blockers == &json!(true),
            )?;
            if !blockers.is_boolean() && truthy(&input["contract"]) {
                return Err("acceptance:invalid-value".into());
            }
            Ok(result)
        }
        "activity-validate" => done(session::validate(&input["state"])),
        "activity-identity" => {
            session::identity(&input["session"], &input["task"]).map(|()| json!(true))
        }
        "activity-update" => {
            let mut state = input["state"].clone();
            session::add_activity(&mut state, object(&input["values"])?)?;
            Ok(state)
        }
        "task-binding" => {
            let checkout = input.get("repository").map(|r| &r["checkout"]);
            session::binding(&input["state"], checkout).map(|b| b.unwrap_or(Value::Null))
        }
        "task-contract" => session::contract(input.get("record").filter(|r| truthy(r))),
        "task-source" => Ok(session::source(&input["repository"])),
        "task-context" => session::context(&input["state"], &input["repository"]),
        other => panic!("unmapped operation {other}"),
    }
}

#[test]
fn frozen_checkpoint_and_session_contracts() {
    let corpus: Value = serde_json::from_str(include_str!(
        "../../tests/fixtures/runtime/checkpoint-contracts.json"
    ))
    .unwrap();
    let mut failures = Vec::new();
    let mut corrected = 0;
    for case in corpus["cases"].as_array().unwrap() {
        let id = case["id"].as_str().unwrap();
        let operation = case["request"]["operation"].as_str().unwrap();
        let input: Map<String, Value> = case["request"]["input"]
            .as_object()
            .unwrap()
            .iter()
            .map(|(k, v)| (k.clone(), expand(v, &corpus["documents"])))
            .collect();
        let expected = &case["response"];
        let actual = run(operation, &Value::Object(input));
        if CORRECTIONS.contains(&id) {
            corrected += 1;
            if actual.is_ok() {
                failures.push(format!("{id}: correction not applied"));
            }
            continue;
        }
        let matches = match (&actual, expected["ok"].as_bool().unwrap()) {
            (Ok(result), true) => expected.get("result").is_none_or(|e| e == result),
            (Err(message), false) => {
                let reason = expected["error"]["message"].as_str().unwrap();
                !reason.starts_with("acceptance:") || reason == message
            }
            _ => false,
        };
        if !matches {
            failures.push(format!("{id}: expected {expected}, got {actual:?}"));
        }
    }
    assert_eq!(corrected, CORRECTIONS.len());
    assert!(
        failures.is_empty(),
        "{} of {} cases differ:\n{}",
        failures.len(),
        corpus["cases"].as_array().unwrap().len(),
        failures.join("\n")
    );
}
