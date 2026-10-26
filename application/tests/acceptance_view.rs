//! `craft acceptance-view`: the reader views over contract, ledger and probe files, their
//! argument rules and the command telemetry they record.
use craft_application::cli;
use craft_engine::session::views::{self, Reader};
use serde_json::{Value, json};
use std::fs;
use std::path::PathBuf;

fn args(values: &[&str]) -> Vec<String> {
    values.iter().map(|value| value.to_string()).collect()
}

/// A scratch working directory holding the example's documents as separate files.
fn scratch() -> (PathBuf, Value) {
    let dir = std::env::temp_dir().join(format!("craft-acceptance-view-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let dir = dir.canonicalize().unwrap();
    std::env::set_current_dir(&dir).unwrap();
    let input: Value = serde_json::from_str(include_str!(
        "../../examples/evals/product-views/input.json"
    ))
    .unwrap();
    for (file, field) in [
        ("contract λ.json", "contract"),
        ("ledger.json", "evidence"),
        ("focus.json", "focus"),
    ] {
        fs::write(dir.join(file), input[field].to_string()).unwrap();
    }
    let probe = json!([{"id": "replay", "criterionIds": ["single-use"], "risk": "reuse",
        "flows": ["replay"], "preconditions": [], "actions": ["redeem twice"],
        "expectedBasis": "single use", "evidenceIds": []}]);
    fs::write(dir.join("probes.json"), probe.to_string()).unwrap();
    (dir, input)
}

fn view(extra: &[&str]) -> cli::Outcome {
    let mut argv = args(&[
        "acceptance-view",
        "--contract",
        "contract λ.json",
        "--acceptance-evidence",
        "ledger.json",
        "--current-kind",
        "environment",
    ]);
    argv.extend(args(extra));
    cli::run(&argv)
}

#[test]
fn views_read_files_and_refuse_options_outside_their_reader() {
    let (dir, input) = scratch();
    let current = json!({"kind": "environment", "identity": "build-1"});

    let outcome = view(&[
        "--view",
        "architecture",
        "--focus",
        "focus.json",
        "--current-identity",
        "build-1",
    ]);
    assert_eq!(outcome.exit_code, 0, "{}", outcome.stderr);
    let result: Value = serde_json::from_str(&outcome.stdout).unwrap();
    let expected = views::reader(
        Reader::Architecture,
        Some(&input["contract"]),
        Some(&input["evidence"]),
        &current,
        Some(&input["focus"]),
        false,
    )
    .unwrap();
    assert_eq!(result, expected);

    // Without probes, QA lists every criterion as unexplored.
    let outcome = view(&["--view", "qa"]);
    assert_eq!(outcome.exit_code, 0, "{}", outcome.stderr);
    let result: Value = serde_json::from_str(&outcome.stdout).unwrap();
    assert_eq!(
        result["binding"],
        json!({"kind": "environment", "identity": null})
    );
    assert_eq!(result["unexploredCriterionIds"], json!(["single-use"]));

    // A resolved contract with verified evidence is complete until blockers are declared.
    let mut contract = input["contract"].clone();
    for decision in contract["decisions"].as_array_mut().unwrap() {
        decision["resolved"] = json!(true);
    }
    for constraint in contract["context"]["constraints"].as_array_mut().unwrap() {
        constraint["status"] = json!("accepted");
        constraint["conflicts"] = json!([]);
    }
    let digest = craft_engine::session::acceptance::contract_digest(&contract).unwrap();
    let record = json!({"id": "replay-run", "criterionIds": ["single-use"], "scope": ["reset"],
        "contract": digest, "binding": current, "checkId": "replay", "kind": "manual",
        "artifact": "notes", "observedAt": "2026-10-22T00:00:00Z", "reporter": "assessor",
        "basis": "replay", "support": "verified", "execution": "passed"});
    fs::write(dir.join("resolved.json"), contract.to_string()).unwrap();
    let ledger = json!({"formatVersion": 1, "records": [record]});
    fs::write(dir.join("verified.json"), ledger.to_string()).unwrap();
    for (blockers, complete) in [(false, true), (true, false)] {
        let mut argv = args(&[
            "acceptance-view",
            "--view",
            "product",
            "--contract",
            "resolved.json",
            "--acceptance-evidence",
            "verified.json",
            "--current-kind",
            "environment",
            "--current-identity",
            "build-1",
        ]);
        if blockers {
            argv.push("--blockers".into());
        }
        let outcome = cli::run(&argv);
        assert_eq!(outcome.exit_code, 0, "{}", outcome.stderr);
        let result: Value = serde_json::from_str(&outcome.stdout).unwrap();
        assert_eq!(
            result["coverage"]["complete"], complete,
            "blockers {blockers}"
        );
    }
    let outcome = view(&["--view", "qa", "--probes", "probes.json"]);
    let result: Value = serde_json::from_str(&outcome.stdout).unwrap();
    assert_eq!(result["probes"][0]["execution"], "not-run");
    assert_eq!(result["criteria"][0]["probeIds"], json!(["replay"]));

    let product = view(&["--view", "product"]);
    let result: Value = serde_json::from_str(&product.stdout).unwrap();
    assert_eq!(result["dependencies"], json!([]));
    assert!(result.get("adrSources").is_none());

    for (extra, message) in [
        (
            &["--view", "qa", "--focus", "focus.json"][..],
            "usage: craft acceptance-view",
        ),
        (
            &["--view", "product", "--probes", "probes.json"],
            "usage: craft acceptance-view",
        ),
        (&["--view", "release"], "usage: craft acceptance-view"),
        (&[], "usage: craft acceptance-view"),
        (
            &["--view", "product", "--current-kind", "build"],
            "duplicate option",
        ),
        (
            &["--view", "product", "--focus", "missing.json"],
            "os error 2",
        ),
    ] {
        let outcome = view(extra);
        assert_eq!(outcome.exit_code, 2, "{extra:?}");
        assert!(outcome.stdout.is_empty());
        assert!(
            outcome.stderr.contains(message),
            "{extra:?}: {}",
            outcome.stderr
        );
    }
    let invalid = cli::run(&args(&[
        "acceptance-view",
        "--view",
        "product",
        "--contract",
        "ledger.json",
        "--acceptance-evidence",
        "ledger.json",
        "--current-kind",
        "environment",
    ]));
    assert_eq!(invalid.exit_code, 2);
    assert!(
        invalid.stderr.starts_with("acceptance:"),
        "{}",
        invalid.stderr
    );
    let unsupported = cli::run(&args(&[
        "route",
        "--intent",
        "implement",
        "--probes",
        "probes.json",
    ]));
    assert_eq!(unsupported.exit_code, 2);
    assert!(
        unsupported
            .stderr
            .contains("--probes is supported by acceptance-view")
    );
    let other = cli::run(&args(&[
        "route",
        "--intent",
        "implement",
        "--acceptance-evidence",
        "ledger.json",
    ]));
    assert!(
        other
            .stderr
            .contains("supported by task-step and acceptance-view")
    );

    let help = cli::run(&args(&["acceptance-view", "--help"]));
    assert!(help.stdout.contains("--view qa|product|architecture"));
    let catalog = cli::run(&args(&["help"]));
    assert!(catalog.stdout.contains("\"acceptance-view\""));
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn the_command_name_survives_telemetry_sanitizing() {
    let event = craft_engine::telemetry::Event {
        name: "cli.command",
        timestamp: "2026-10-26T00:00:00.000Z".into(),
        session_id: &"a".repeat(32),
        trace_id: &"b".repeat(32),
        operation_id: "cccccccc-cccc-4ccc-8ccc-cccccccccccc".into(),
        attributes: serde_json::Map::from_iter([("command".into(), json!("acceptance-view"))]),
        operation: true,
        ..Default::default()
    };
    let stored = craft_engine::telemetry::envelope(&event, &[]).unwrap();
    assert_eq!(stored["attributes"]["command"], "acceptance-view");
}
