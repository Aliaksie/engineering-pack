//! Durable native transport journals. Protocol facts may be supplied by an adapter;
//! record identity, bounded shape and operation admission belong to the runtime.
use crate::values::{sha256 as hash, text};
use serde_json::{Value, json};
fn choice(value: &Value, choices: &[&str]) -> bool {
    value.as_str().is_some_and(|s| choices.contains(&s))
}
fn fields(value: &Value, names: &[&str]) -> bool {
    value
        .as_object()
        .is_some_and(|o| o.len() == names.len() && names.iter().all(|key| o.contains_key(*key)))
}
pub fn validate(
    journal: &Value,
    tree: &Value,
    host: &Value,
    coordinator: &Value,
) -> Result<(), String> {
    let valid = fields(
        journal,
        &[
            "formatVersion",
            "host",
            "tree",
            "coordinatorId",
            "operations",
        ],
    ) && journal["formatVersion"] == 1
        && journal["tree"] == *tree
        && journal["coordinatorId"] == *coordinator
        && journal["host"] == *host
        && journal["operations"].as_array().is_some_and(|ops| {
            let mut keys = std::collections::HashSet::new();
            ops.len() <= 192
                && ops.iter().all(|op| {
                    fields(
                        op,
                        &[
                            "key",
                            "kind",
                            "source",
                            "checkpoint",
                            "nativeId",
                            "phase",
                            "released",
                            "acceptanceEvidence",
                        ],
                    ) && hash(&op["key"])
                        && keys.insert(op["key"].as_str())
                        && choice(
                            &op["kind"],
                            &["worker", "cancel", "review", "compact", "release"],
                        )
                        && choice(
                            &op["phase"],
                            &[
                                "intent",
                                "acknowledged",
                                "resume-intent",
                                "resumed",
                                "completed",
                            ],
                        )
                        && ["source", "checkpoint"]
                            .iter()
                            .all(|k| op[k].is_null() || hash(&op[k]))
                        && ["nativeId", "acceptanceEvidence"]
                            .iter()
                            .all(|k| op[k].is_null() || text(&op[k]))
                        && op["released"].is_boolean()
                })
        });
    if valid {
        Ok(())
    } else {
        Err("invalid bounded native operation journal".into())
    }
}
pub fn empty(tree: &str, host: &str, coordinator: &str) -> Value {
    json!({"formatVersion":1,"host":host,"tree":tree,"coordinatorId":coordinator,"operations":[]})
}
pub fn begin(
    journal: &mut Value,
    input: &Value,
    kind: &Value,
    native_id: &Value,
) -> Result<Value, String> {
    if !hash(&input["key"]) {
        return Err("invalid native operation key".into());
    }
    let ops = journal["operations"]
        .as_array_mut()
        .ok_or("invalid native journal")?;
    if ops.iter().any(|op| op["key"] == input["key"]) {
        return Err("native operation already attempted; reconcile its original key".into());
    }
    if ops.len() >= 192 {
        return Err("native operation journal capacity reached; no intent was evicted".into());
    }
    let op = json!({"key":input["key"],"kind":kind,"source":input["source"],"checkpoint":input["checkpoint"],
        "nativeId":native_id,"phase":"intent","released":false,"acceptanceEvidence":null});
    ops.push(op.clone());
    Ok(op)
}
/// A snapshot cannot replace ownership, erase an attempted effect, append an unadmitted
/// operation or change the operation's source/checkpoint. The driver may withdraw only a
/// release intent for which it explicitly observed that no effect was submitted.
pub fn replacement(before: &Value, after: &Value, withdrawn: Option<&str>) -> Result<(), String> {
    let old = before["operations"]
        .as_array()
        .ok_or("invalid native journal")?;
    let new = after["operations"]
        .as_array()
        .ok_or("invalid native journal")?;
    let remaining: Vec<_> = old
        .iter()
        .filter(|op| {
            !(withdrawn == op["key"].as_str()
                && op["kind"] == "release"
                && op["phase"] == "intent"
                && op["released"] == false)
        })
        .collect();
    if remaining.len() != new.len()
        || remaining.iter().zip(new).any(|(a, b)| {
            ["key", "kind", "source", "checkpoint"]
                .iter()
                .any(|key| a[key] != b[key])
        })
    {
        return Err("native journal operation identity changed".into());
    }
    Ok(())
}
pub fn support(host: &str) -> Result<Value, String> {
    let table: Value = serde_json::from_str(include_str!("../../hosts/capabilities.json"))
        .expect("embedded host capabilities");
    table
        .get(host)
        .cloned()
        .ok_or_else(|| "unknown host adapter".into())
}
