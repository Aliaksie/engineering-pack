use super::lifecycle::*;
use serde_json::{Value, json};
#[test]
fn frozen_worker_decisions() {
    // Reconcile cases are a separate file: each stays under the 2 MiB per-file limit of
    // working-tree snapshots, so checkpoints of this repository can complete.
    let corpora: [Value; 2] = [
        include_str!("../../../tests/fixtures/runtime/worker-contracts.json"),
        include_str!("../../../tests/fixtures/runtime/worker-reconcile-contracts.json"),
    ]
    .map(|text| serde_json::from_str(text).unwrap());
    let cases: Vec<&Value> = corpora
        .iter()
        .flat_map(|corpus| corpus["cases"].as_array().unwrap())
        .collect();
    assert_eq!(cases.len(), 953);
    for c in cases {
        let a = &c["input"];
        let mut state = a["state"].clone();
        let result = match c["operation"].as_str().unwrap() {
            "worker-input" => input(&a["input"]),
            "worker-state" => super::lifecycle::state(&a["state"], &a["tree"]),
            "worker-snapshot" => snapshot(
                &a["snapshot"],
                a["session"].as_str().unwrap_or_default(),
                a["now"].as_i64().unwrap_or(0),
            ),
            "worker-transition" => transition(&mut state, &a["id"], &a["result"]),
            "worker-reconcile" => reconcile(&mut state, &a["snapshot"], &a["targets"]),
            "worker-admit" => admit(
                &mut state,
                Admission {
                    input: &a["input"],
                    checked: &a["checked"],
                    snapshot: &a["snapshot"],
                    settings: &a["project"],
                    session: a["session"].as_str().unwrap_or_default(),
                    now: a["now"].as_i64().unwrap_or(0),
                    at: &a["at"],
                    revision: &a["revision"],
                },
            ),
            "worker-evidence" => evidence(&a["state"], &a["taskState"], &a["repository"]),
            _ => panic!("unexpected corpus operation: {}", c["operation"]),
        };
        let observed = match result {
            Ok(v) => json!({"result":v}),
            Err(e) => json!({"error":e}),
        };
        let correction = match c["id"].as_str().unwrap() {
            "legacy:worker-evidence:empty" => Some("invalid worker task state"),
            "legacy:worker-transition:empty" | "legacy:worker-reconcile:empty" => {
                Some("invalid worker tree state")
            }
            _ => None,
        };
        let expected = if let Some(message) = correction {
            json!({"error": message})
        } else if c.get("error").is_some() {
            json!({"error":c["error"]})
        } else {
            json!({"result":c["result"]})
        };
        assert_eq!(observed, expected, "case {} {}", c["id"], c["operation"]);
    }
}
