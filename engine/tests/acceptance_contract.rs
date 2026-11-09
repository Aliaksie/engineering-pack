//! Engineering contracts, acceptance evidence and coverage over the shared acceptance fixtures.
use craft_engine::session::acceptance::{
    contract_digest, coverage as cover, validate_contract, validate_evidence,
};
use craft_engine::{checkpoint, session};
use serde_json::{Map, Value, json};

fn fixture(name: &str) -> Value {
    let text = match name {
        "contract" => include_str!("../../tests/fixtures/acceptance/contract.json"),
        "legacy" => include_str!("../../tests/fixtures/acceptance/legacy.json"),
        "legacy-completed" => include_str!("../../tests/fixtures/acceptance/legacy-completed.json"),
        other => panic!("unknown fixture {other}"),
    };
    serde_json::from_str(text).unwrap()
}

fn current() -> Value {
    json!({"kind": "repository", "identity": "snapshot-a"})
}

fn record(contract: &Value, overrides: Value) -> Value {
    let mut record = json!({"id": "check-value", "criterionIds": ["value"], "scope": ["src"],
        "contract": contract_digest(contract).unwrap(), "binding": current(), "checkId": "value-check",
        "kind": "check", "artifact": "retained/value-output", "observedAt": "2026-10-22T00:00:00Z",
        "reporter": "fixture runner", "basis": "Assert returned value equals the expected value",
        "support": "verified", "execution": "passed"});
    for (key, value) in overrides.as_object().unwrap() {
        record[key] = value.clone();
    }
    record
}

fn coverage_at(contract: &Value, records: &[Value], binding: &Value, blockers: bool) -> Value {
    let evidence = json!({"formatVersion": 1, "records": records});
    cover(Some(contract), Some(&evidence), binding, blockers).unwrap()
}

fn coverage(contract: &Value, records: &[Value]) -> Value {
    coverage_at(contract, records, &current(), false)
}

fn statuses(result: &Value) -> Vec<&str> {
    result["criteria"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["status"].as_str().unwrap())
        .collect()
}

#[test]
fn contract_identity_is_key_order_independent_and_tracks_meaning() {
    let contract = fixture("contract");
    validate_contract(&contract).unwrap();
    let reordered: Map<String, Value> = contract
        .as_object()
        .unwrap()
        .iter()
        .rev()
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();
    assert_eq!(
        contract_digest(&contract).unwrap(),
        contract_digest(&Value::Object(reordered)).unwrap()
    );
    for (pointer, value) in [
        ("/criteria/0/description", "Changed acceptance"),
        ("/context/sources/0/identity", "request-v2"),
        ("/context/sources/0/status", "proposed"),
    ] {
        let mut changed = contract.clone();
        *changed.pointer_mut(pointer).unwrap() = json!(value);
        assert_ne!(
            contract_digest(&contract),
            contract_digest(&changed),
            "{pointer}"
        );
    }
}

#[test]
fn one_criterion_never_covers_another_and_gaps_stay_visible() {
    let mut contract = fixture("contract");
    let value = record(&contract, json!({}));
    let partial = coverage(&contract, std::slice::from_ref(&value));
    assert_eq!(partial["complete"], false);
    assert_eq!(statuses(&partial), ["verified", "gap"]);
    let error = record(
        &contract,
        json!({"id": "check-error", "criterionIds": ["error"]}),
    );
    assert_eq!(
        coverage(&contract, &[value.clone(), error.clone()])["complete"],
        true
    );
    let blocked = coverage_at(&contract, &[value.clone(), error.clone()], &current(), true);
    assert_eq!(blocked["complete"], false);
    let mut irrelevant = error.clone();
    irrelevant["checkId"] = json!("irrelevant-build");
    assert_eq!(
        coverage(&contract, &[value.clone(), irrelevant])["complete"],
        false
    );
    let mut elsewhere = error;
    elsewhere["scope"] = json!(["elsewhere"]);
    assert_eq!(coverage(&contract, &[value, elsewhere])["complete"], false);
    contract["criteria"][1]["required"] = json!(false);
    let optional = coverage(&contract, &[record(&contract, json!({}))]);
    assert_eq!(optional["complete"], true);
    assert_eq!(optional["criteria"][1]["status"], "gap");
}

#[test]
fn stale_unknown_unexecuted_and_weak_support_cannot_satisfy_required_criteria() {
    let mut contract = fixture("contract");
    for overrides in [
        json!({"execution": "failed"}),
        json!({"execution": "not-run"}),
        json!({"execution": "not-available"}),
        json!({"support": "observed"}),
        json!({"support": "inferred"}),
        json!({"support": "unknown"}),
        json!({"kind": "manual"}),
        json!({"binding": {"kind": "repository", "identity": null}}),
        json!({"contract": "a".repeat(64)}),
    ] {
        let mut patch = overrides.clone();
        patch["criterionIds"] = json!(["value", "error"]);
        let result = coverage(&contract, &[record(&contract, patch)]);
        assert_eq!(result["complete"], false, "{overrides}");
    }
    let records = [record(
        &contract,
        json!({"criterionIds": ["value", "error"]}),
    )];
    let other = json!({"kind": "repository", "identity": "snapshot-b"});
    assert_eq!(
        coverage_at(&contract, &records, &other, false)["records"][0]["freshness"],
        "stale"
    );
    let unknown = json!({"kind": "repository", "identity": null});
    assert_eq!(
        coverage_at(&contract, &records, &unknown, false)["records"][0]["freshness"],
        "unknown"
    );
    contract["criteria"][0]["description"] = json!("Amended contract without a source edit");
    assert_eq!(
        coverage(&contract, &records)["records"][0]["freshness"],
        "stale"
    );
}

#[test]
fn non_git_qa_binds_to_an_explicit_environment_and_stays_attributed() {
    let mut contract = fixture("contract");
    for criterion in contract["criteria"].as_array_mut().unwrap() {
        criterion["evidenceKind"] = json!("manual");
    }
    let environment = json!({"kind": "environment", "identity": "build-42/browser-fixture"});
    let evidence = record(
        &contract,
        json!({"criterionIds": ["value", "error"], "kind": "manual", "binding": environment}),
    );
    let result = coverage_at(
        &contract,
        std::slice::from_ref(&evidence),
        &environment,
        false,
    );
    assert_eq!(result["complete"], true);
    assert_eq!(result["assurance"], "attributed");
    let later = json!({"kind": "environment", "identity": "build-43/browser-fixture"});
    assert_eq!(
        coverage_at(&contract, std::slice::from_ref(&evidence), &later, false)["complete"],
        false
    );
    let repository = json!({"kind": "repository", "identity": "build-42/browser-fixture"});
    assert_eq!(
        coverage_at(&contract, &[evidence], &repository, false)["complete"],
        false
    );
}

#[test]
fn conflicts_open_decisions_and_failures_cannot_be_hidden_by_a_pass() {
    let mut contract = fixture("contract");
    contract["context"]["constraints"] = json!(
        ["allow", "deny"]
            .iter()
            .enumerate()
            .map(|(index, id)| json!({
        "id": id, "description": id, "sourceIds": ["request"], "authority": "project",
        "status": "accepted", "scope": ["src"], "required": true,
        "conflicts": [if index == 1 { "allow" } else { "deny" }]}))
            .collect::<Vec<_>>()
    );
    let good = |contract: &Value| record(contract, json!({"criterionIds": ["value", "error"]}));
    let conflicted = coverage(&contract, &[good(&contract)]);
    assert_eq!(conflicted["conflicts"], json!(["allow", "deny"]));
    assert_eq!(conflicted["complete"], false);
    contract["context"]["constraints"] = json!([]);
    contract["decisions"] = json!([{"id": "decision", "description": "Unanswered product question", "blocking": true, "resolved": false}]);
    assert_eq!(coverage(&contract, &[good(&contract)])["complete"], false);
    contract["decisions"] = json!([]);
    let mut failed = good(&contract);
    failed["id"] = json!("failed-check");
    failed["execution"] = json!("failed");
    assert_eq!(
        coverage(&contract, &[good(&contract), failed])["complete"],
        false
    );
}

#[test]
fn invalid_ambiguous_unsupported_and_oversized_payloads_fail_explicitly() {
    let reason = |result: Result<(), String>| result.unwrap_err();
    let base = fixture("contract");
    let first = base["criteria"][0].clone();
    for (pointer, value, expected) in [
        ("/formatVersion", json!(2), Some("unsupported-version")),
        (
            "/authorization",
            json!("execute arbitrary source commands"),
            Some("invalid-fields"),
        ),
        ("/criteria/1/id", first["id"].clone(), Some("duplicate-id")),
        (
            "/criteria/0/sourceIds",
            json!(["missing"]),
            Some("unknown-reference"),
        ),
        ("/criteria/0/sourceIds", json!([]), Some("missing-basis")),
        (
            "/criteria",
            json!(vec![first.clone(); 33]),
            Some("capacity"),
        ),
        ("/criteria/0/scope", json!(["C:\\source"]), None),
        (
            "/exclusions",
            json!(vec!["🙂".repeat(256); 32]),
            Some("capacity"),
        ),
    ] {
        let mut contract = base.clone();
        let (parent, key) = pointer.rsplit_once('/').unwrap();
        let parent = if parent.is_empty() {
            &mut contract
        } else {
            contract.pointer_mut(parent).unwrap()
        };
        match parent {
            Value::Array(items) => items[key.parse::<usize>().unwrap()] = value,
            object => object[key] = value,
        }
        let error = reason(validate_contract(&contract));
        if let Some(expected) = expected {
            assert_eq!(error, format!("acceptance:{expected}"), "{pointer}");
        }
    }
    let mut unidentified = base.clone();
    unidentified["context"]["sources"][0]
        .as_object_mut()
        .unwrap()
        .shift_remove("identity");
    assert_eq!(
        reason(validate_contract(&unidentified)),
        "acceptance:invalid-text"
    );
    let full = json!({"formatVersion": 1, "records": vec![record(&base, json!({})); 65]});
    assert_eq!(reason(validate_evidence(&full)), "acceptance:capacity");
    let unknown = json!({"formatVersion": 1, "records": [record(&base, json!({"criterionIds": ["missing"]}))]});
    assert_eq!(
        cover(Some(&base), Some(&unknown), &current(), false).unwrap_err(),
        "acceptance:unknown-reference"
    );
}

#[test]
fn legacy_lifecycle_and_checkpoints_keep_declared_completion_without_inferred_coverage() {
    let steps: Value = serde_json::from_str(include_str!(
        "../../tests/fixtures/runtime/step-contracts.json"
    ))
    .unwrap();
    let starts: Value = serde_json::from_str(include_str!(
        "../../tests/fixtures/runtime/task-contracts.json"
    ))
    .unwrap();
    let find = |corpus: &Value, id: &str| {
        corpus["cases"]
            .as_array()
            .unwrap()
            .iter()
            .find(|case| case["id"] == id)
            .unwrap()["response"]["result"]
            .clone()
    };
    let legacy = fixture("legacy");
    let frozen = fixture("legacy-completed");
    let stepped = find(&steps, "step:legacy-step");
    assert_eq!(stepped["state"], frozen["state"]);
    assert_eq!(
        session::contract(Some(&stepped["state"]["taskRecord"])).unwrap(),
        frozen["checkpoint"]["taskContract"]
    );
    checkpoint::validate(&frozen["checkpoint"]).unwrap();
    assert_eq!(stepped["state"]["taskRecord"]["goalComplete"], true);
    session::validate(&stepped["state"]).unwrap();
    let metadata = json!({"taskId": "fix-value", "continuation": stepped["continuation"]});
    let saved = checkpoint::state(
        &json!({}),
        metadata.as_object().unwrap(),
        &legacy["repository"],
        legacy["at"].as_str().unwrap(),
    )
    .unwrap();
    checkpoint::validate(&saved).unwrap();
    assert_eq!(
        cover(Some(&Value::Null), None, &Value::Null, false).unwrap(),
        json!({"assurance": "declared", "coverage": "unknown", "complete": false, "criteria": []})
    );
    // Declared checkpoints never acquire criterion coverage.
    let contract = json!({"engineeringContract": fixture("contract")});
    assert!(checkpoint::validate_metadata(contract.as_object().unwrap()).is_err());
    let mut future = legacy["state"].clone();
    future["formatVersion"] = json!(2);
    assert!(session::validate(&future).is_err());
    let mut augmented = frozen["state"].clone();
    augmented["taskRecord"]["engineeringContract"] = fixture("contract");
    assert!(session::validate(&augmented).is_err());
    let upgraded = find(&starts, "task-start:legacy-contract-upgrade");
    assert_eq!(upgraded["state"]["formatVersion"], 2);
    assert_eq!(upgraded["state"]["taskRecord"]["goalComplete"], false);
    session::validate(&upgraded["state"]).unwrap();
    let mut checkpoint_contract = frozen["checkpoint"].clone();
    checkpoint_contract["engineeringContract"] = fixture("contract");
    checkpoint::validate(&checkpoint_contract).unwrap();
}

fn coverage_v2(contract: &Value, records: &[Value]) -> Value {
    let evidence = json!({"formatVersion": 2, "records": records});
    cover(Some(contract), Some(&evidence), &current(), false).unwrap()
}

fn ran(executed: u64, failed: u64, runner: Value) -> Value {
    json!({"runner": runner, "executed": executed, "failed": failed, "skipped": 0, "source": "observed"})
}

#[test]
fn a_v2_run_that_executed_no_test_or_counted_failures_fails_its_criterion() {
    let contract = fixture("contract");
    let both = |tests: Value| {
        record(
            &contract,
            json!({"criterionIds": ["value", "error"], "tests": tests}),
        )
    };
    let passing = coverage_v2(&contract, &[both(ran(3, 0, json!("cargo")))]);
    assert_eq!(statuses(&passing), ["verified", "verified"]);
    assert_eq!(passing["complete"], true);

    let zero = coverage_v2(&contract, &[both(ran(0, 0, json!("cargo")))]);
    assert_eq!(statuses(&zero), ["failed", "failed"]);
    assert_eq!(zero["criteria"][0]["reason"], "zero-tests");
    assert_eq!(zero["complete"], false);

    // A failure exit with no summary reports no counts: it failed, but nothing says no test ran.
    let uncounted = record(
        &contract,
        json!({"criterionIds": ["value", "error"], "execution": "failed",
            "tests": {"runner": null, "source": "observed"}}),
    );
    let failed = coverage_v2(&contract, &[uncounted]);
    assert_eq!(statuses(&failed), ["failed", "failed"]);
    assert!(failed["criteria"][0].get("reason").is_none(), "{failed}");

    let counted = coverage_v2(&contract, &[both(ran(3, 1, json!("jest")))]);
    assert_eq!(statuses(&counted), ["failed", "failed"]);
    assert!(counted["criteria"][0].get("reason").is_none());

    // A runner Craft did not recognize leaves the pass undecided, never verified.
    let unknown = coverage_v2(&contract, &[both(ran(3, 0, Value::Null))]);
    assert_eq!(statuses(&unknown), ["unknown", "unknown"]);
    assert_eq!(unknown["complete"], false);
}

#[test]
fn v2_keeps_undecided_evidence_apart_from_missing_evidence() {
    let contract = fixture("contract");
    let not_run = record(&contract, json!({"execution": "not-run"}));
    // `value` has current evidence that does not decide; `error` has none.
    assert_eq!(
        statuses(&coverage_v2(&contract, std::slice::from_ref(&not_run))),
        ["unknown", "gap"]
    );
    // v1 reads as before: both are gaps.
    assert_eq!(statuses(&coverage(&contract, &[not_run])), ["gap", "gap"]);
    // Stale evidence is not current evidence: a gap, not unknown.
    let stale = record(
        &contract,
        json!({"binding": {"kind": "repository", "identity": "snapshot-b"}}),
    );
    assert_eq!(statuses(&coverage_v2(&contract, &[stale])), ["gap", "gap"]);
}

#[test]
fn test_counts_are_a_v2_field_with_bounded_values() {
    let contract = fixture("contract");
    let with = |tests: Value| record(&contract, json!({"tests": tests}));
    assert!(
        validate_evidence(
            &json!({"formatVersion": 1, "records": [with(ran(1, 0, json!("cargo")))]})
        )
        .is_err()
    );
    assert!(
        validate_evidence(
            &json!({"formatVersion": 2, "records": [with(ran(1, 0, json!("cargo")))]})
        )
        .is_ok()
    );
    for bad in [
        json!({"runner": "cargo", "executed": -1, "failed": 0, "skipped": 0, "source": "observed"}),
        json!({"runner": "cargo", "executed": 1, "failed": 0, "skipped": 0, "source": "guessed"}),
        json!({"runner": "cargo", "executed": 1, "failed": 0, "skipped": 0, "source": "observed", "extra": 1}),
        json!({"executed": 1, "failed": 0, "skipped": 0, "source": "observed"}),
    ] {
        assert!(
            validate_evidence(&json!({"formatVersion": 2, "records": [with(bad.clone())]}))
                .is_err(),
            "{bad}"
        );
    }
    assert!(validate_evidence(&json!({"formatVersion": 3, "records": []})).is_err());
}
