//! Real runner outputs (captured, scratch paths replaced) and the verdict each must get:
//! zero tests, everything skipped and unknown output are never a pass.
use craft_engine::session::test_runs::classify;
use std::path::Path;

/// `(capture, result, reason, executed)`; `None` where the summary has no counts.
const EXPECTED: &[(&str, &str, &str, Option<u64>)] = &[
    ("cargo-pass", "passed", "summary", Some(3)),
    ("cargo-fail", "failed", "exit-code", Some(3)),
    ("cargo-filtered", "passed", "summary", Some(1)),
    ("cargo-zero", "failed", "zero-tests", Some(0)),
    ("cargo-exact-zero", "failed", "zero-tests", Some(0)),
    ("cargo-empty-suite", "failed", "zero-tests", Some(0)),
    ("cargo-no-run", "unknown", "unrecognized-output", None),
    ("node-pass", "passed", "summary", Some(3)),
    ("node-fail", "failed", "exit-code", Some(3)),
    ("node-filtered", "passed", "summary", Some(1)),
    ("node-zero-filter", "failed", "zero-tests", Some(0)),
    ("node-zero-filter-tap", "failed", "zero-tests", Some(0)),
    ("node-zero-filter-junit", "failed", "zero-tests", Some(0)),
    ("node-zero-filter-direct", "failed", "zero-tests", Some(0)),
    (
        "node-zero-filter-isolation-none",
        "failed",
        "zero-tests",
        Some(0),
    ),
    ("node-no-files", "failed", "zero-tests", Some(0)),
    ("node-empty-file", "failed", "zero-tests", Some(0)),
    ("pytest-pass", "passed", "summary", Some(3)),
    ("pytest-quiet-pass", "passed", "summary", Some(3)),
    ("pytest-fail", "failed", "exit-code", Some(3)),
    ("pytest-filtered", "passed", "summary", Some(1)),
    ("pytest-zero-filter", "failed", "exit-code", Some(0)),
    ("pytest-empty-dir", "failed", "exit-code", Some(0)),
    ("pytest-all-skipped", "failed", "all-skipped", Some(0)),
    ("unittest-pass", "passed", "summary", Some(3)),
    ("unittest-fail", "failed", "exit-code", Some(3)),
    ("unittest-filtered", "passed", "summary", Some(1)),
    ("unittest-zero-filter", "failed", "exit-code", Some(0)),
    ("unittest-empty-discover", "failed", "exit-code", Some(0)),
    ("unittest-import-error", "failed", "exit-code", Some(1)),
    ("vitest-pass", "passed", "summary", Some(3)),
    ("vitest-fail", "failed", "exit-code", Some(3)),
    ("vitest-filtered", "passed", "summary", Some(1)),
    ("vitest-zero-filter", "failed", "all-skipped", Some(0)),
    ("vitest-no-files", "failed", "exit-code", Some(0)),
    (
        "vitest-no-files-pass-with-no-tests",
        "failed",
        "zero-tests",
        Some(0),
    ),
    ("vitest-empty-file", "failed", "exit-code", Some(0)),
    ("jest-pass", "passed", "summary", Some(3)),
    ("jest-fail", "failed", "exit-code", Some(3)),
    ("jest-filtered", "passed", "summary", Some(1)),
    ("jest-zero-filter", "failed", "all-skipped", Some(0)),
    ("jest-no-files", "failed", "exit-code", Some(0)),
    (
        "jest-no-files-pass-with-no-tests",
        "failed",
        "zero-tests",
        Some(0),
    ),
    ("jest-empty-file", "failed", "exit-code", Some(0)),
    ("maven-pass", "passed", "summary", Some(3)),
    ("maven-fail", "failed", "exit-code", Some(3)),
    ("maven-filtered", "passed", "summary", Some(1)),
    ("maven-zero-method-filter", "failed", "zero-tests", Some(0)),
    ("maven-zero-filter", "failed", "exit-code", Some(0)),
    ("maven-skip-tests", "failed", "zero-tests", Some(0)),
    (
        "maven-zero-filter-allowed",
        "unknown",
        "unrecognized-output",
        None,
    ),
];

/// The capture's exit code and output (everything after the header).
fn capture(name: &str) -> (i64, String) {
    let file =
        Path::new(env!("CARGO_MANIFEST_DIR")).join(format!("tests/fixtures/test-runs/{name}.txt"));
    let text = std::fs::read_to_string(&file).unwrap_or_else(|e| panic!("{name}: {e}"));
    let exit = text
        .lines()
        .find_map(|l| l.strip_prefix("# exit-code: "))
        .and_then(|c| c.trim().parse().ok())
        .unwrap_or_else(|| panic!("{name}: no exit code"));
    let output = text
        .split_once("# --- combined stdout+stderr below ---\n")
        .map(|(_, output)| output.to_owned())
        .unwrap_or_else(|| panic!("{name}: no output marker"));
    (exit, output)
}

#[test]
fn every_captured_runner_output_gets_its_verdict() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/test-runs");
    let captured = std::fs::read_dir(dir).unwrap().count();
    assert_eq!(captured, EXPECTED.len(), "every capture has an expectation");
    let mut wrong = Vec::new();
    for (name, result, reason, executed) in EXPECTED {
        let (exit, output) = capture(name);
        let verdict = classify(Some(exit), &output);
        let got = (
            verdict["result"].as_str().unwrap_or_default(),
            verdict["reason"].as_str().unwrap_or_default(),
            verdict["executed"].as_u64(),
        );
        if got != (*result, *reason, *executed) {
            wrong.push(format!("{name}: {verdict}"));
        }
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

#[test]
fn runners_are_named_from_their_summary() {
    for (name, runner) in [
        ("cargo-pass", "cargo"),
        ("node-zero-filter-tap", "node"),
        ("pytest-quiet-pass", "pytest"),
        ("unittest-pass", "unittest"),
        ("vitest-pass", "vitest"),
        ("jest-pass", "jest"),
        ("maven-pass", "maven"),
    ] {
        let (exit, output) = capture(name);
        assert_eq!(classify(Some(exit), &output)["runner"], runner, "{name}");
    }
}
