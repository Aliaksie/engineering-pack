//! JS engine operations over their captured JSON input, shared by the frozen replays.
use craft_engine::{checkpoint, hash, repository, session, session::acceptance};
use serde_json::{Map, Value, json};

pub fn object(value: &Value) -> Result<&Map<String, Value>, String> {
    value
        .as_object()
        .ok_or_else(|| "expected an object".to_string())
}

pub fn truthy(value: &Value) -> bool {
    !matches!(value, Value::Null | Value::Bool(false))
        && value.as_str() != Some("")
        && value.as_f64() != Some(0.0)
}

pub fn run(operation: &str, input: &Value) -> Result<Value, String> {
    let done = |result: Result<(), String>| result.map(|()| Value::Null);
    match operation {
        "repository-scopes" => repository::scopes(&input["repositories"]).map(|s| json!(s)),
        "repository-snapshot" => repository::snapshot(
            input["repositories"]
                .as_array()
                .ok_or("repositories must be an array")?,
        ),
        "repository-checkout" => {
            let entries = input["repositories"]
                .as_array()
                .ok_or("repositories must be an array")?
                .iter()
                .map(|e| match e["path"].as_str() {
                    Some(path) => Ok((path.to_string(), e["checkout"].clone())),
                    None => Err("repository path must be a string".to_string()),
                })
                .collect::<Result<Vec<_>, _>>()?;
            Ok(json!(repository::checkout(&entries)))
        }
        "hash" => Ok(json!(hash(input["text"].as_str().unwrap().as_bytes()))),
        "checkpoint-metadata" => done(checkpoint::validate_metadata(object(&input["metadata"])?)),
        "checkpoint-state" => checkpoint::state(
            &input["previous"],
            object(&input["metadata"])?,
            &input["repository"],
            input["updated"]
                .as_str()
                .ok_or("updated must be a string")?,
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
