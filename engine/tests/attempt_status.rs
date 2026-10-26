//! Attempt status over recorded receipts, and the capability class an operation starts from.
use craft_engine::{
    routing::capability,
    session::{attempt_status, unresolved},
};
use serde_json::json;

#[test]
fn switching_profiles_cannot_clear_an_unresolved_permission_failure() {
    let records = [
        json!({"operation": "implement", "profile": "routine", "outcome": "permission-denied", "evidence": "denial"}),
        json!({"operation": "implement", "profile": "deep", "outcome": "quality-failure", "evidence": "check"}),
    ];
    let status = attempt_status(&records, "implement");
    assert_eq!(status["attempts"], 1);
    assert_eq!(status["blocked"], true);
    let mut passed = records.to_vec();
    passed.push(json!({"operation": "implement", "profile": "deep", "outcome": "passed"}));
    assert_eq!(attempt_status(&passed, "implement")["blocked"], false);
}

#[test]
fn discovery_is_economical_and_declared_hard_work_is_strong() {
    let class = |operation: &str, risks: &[&str]| {
        let risks: Vec<String> = risks.iter().map(|r| r.to_string()).collect();
        capability(operation, &risks)
    };
    assert_eq!(class("locate", &[])["class"], "economical");
    assert_eq!(
        class("locate", &[])["execution"],
        "prefer-deterministic-tool"
    );
    assert_eq!(class("architecture", &[])["class"], "strong");
    assert_eq!(
        class("implement", &["distributed-consistency"])["class"],
        "strong"
    );
    assert_eq!(class("implement", &[])["class"], "standard");
}

#[test]
fn prerequisite_resolution_closes_only_matching_blockers() {
    let record = |target: &str, outcome: &str| json!({"operation": "verify", "target": target, "outcome": outcome});
    let records = [
        record("acceptance:scope", "quality-failure"),
        record("acceptance:scope", "tool-unavailable"),
        record("acceptance:other", "blocked"),
        record("acceptance:scope", "prerequisite-resolved"),
    ];
    let open: Vec<(&str, &str)> = unresolved(&records)
        .into_iter()
        .map(|i| {
            (
                records[i]["target"].as_str().unwrap(),
                records[i]["outcome"].as_str().unwrap(),
            )
        })
        .collect();
    assert_eq!(
        open,
        [
            ("acceptance:scope", "quality-failure"),
            ("acceptance:other", "blocked")
        ]
    );
}
