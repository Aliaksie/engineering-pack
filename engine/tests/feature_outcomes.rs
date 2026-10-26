//! Feature outcome records: declarations and runtime receipts, bounded and never inferred.
use craft_engine::session::{feature_report, record_feature, source as task_source};
use serde_json::{Value, json};

const AT: &str = "2026-10-20T00:00:00.000Z";

fn record(state: &Value, input: Value, source: &Value, provenance: &str) -> Result<Value, String> {
    let mut state = state.clone();
    record_feature(&mut state, &input, source, AT, provenance)?;
    Ok(state)
}

#[test]
fn evidence_is_bounded_and_distinguishes_declarations_from_runtime_receipts() {
    let state = json!({"taskRecord": {"id": "task"}, "activity": {"routingDecisions": 1, "operationsAttempted": 2}});
    let source = json!("a".repeat(64));
    let input = json!({"feature": "compaction", "status": "unsupported", "reason": "No verified host control", "evidence": "host:capabilities"});
    let recorded = record(&state, input, &source, "agent-declared").unwrap();
    let report = feature_report(&recorded, &task_source(&json!({}))).unwrap();
    let features = &report["features"];
    assert_eq!(features["compaction"]["provenance"], "agent-declared");
    assert_eq!(features["compaction"]["sourceStatus"], "unknown");
    assert_eq!(features["routing"]["provenance"], "runtime-receipt");
    assert_eq!(features["routing"]["sourceStatus"], "historical");
    // Missing applicability is never filled in.
    assert_eq!(features["review"]["status"], "unknown");
    assert_eq!(report["allClassified"], false);
    for input in [
        json!({"feature": "unknown", "status": "used"}),
        json!({"feature": "review", "status": "passed"}),
        json!({"feature": "review", "status": "used", "reason": "x".repeat(513), "evidence": "check:one"}),
    ] {
        assert!(
            record(&state, input.clone(), &Value::Null, "agent-declared").is_err(),
            "{input}"
        );
    }
}

#[test]
fn a_declaration_never_replaces_a_runtime_observed_outcome() {
    let task = json!({"taskRecord": {"id": "task"}});
    let source = json!("a".repeat(64));
    let used = json!({"feature": "review", "status": "used", "reason": "Host adapter reported the scoped action", "evidence": "review:receipt"});
    let declared = json!({"feature": "review", "status": "disabled", "reason": "Declared afterwards", "evidence": "agent:claim"});
    let observed = record(&task, used.clone(), &source, "runtime-observed").unwrap();
    let error = record(&observed, declared.clone(), &source, "agent-declared").unwrap_err();
    assert!(error.contains("runtime-observed"), "{error}");
    assert_eq!(observed["featureOutcomes"]["review"]["status"], "used");
    let claimed = record(&task, declared, &source, "agent-declared").unwrap();
    let replaced = record(&claimed, used, &source, "runtime-observed").unwrap();
    assert_eq!(
        replaced["featureOutcomes"]["review"]["provenance"],
        "runtime-observed"
    );
}
