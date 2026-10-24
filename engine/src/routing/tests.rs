//! Replays `tests/fixtures/runtime/lookup-contracts.json`, captured from the JS use case.
use super::*;
use crate::contracts::{effective_settings, normalize_numbers};

const CORPUS: &str = include_str!("../../../tests/fixtures/runtime/lookup-contracts.json");

/// Intentional corrections over the captured JavaScript, by case ID; each now fails.
const CORRECTIONS: &[&str] = &[
    // An empty module name listed every module.
    "module-empty",
    // A module selector silently ignored the file, unless the file was private.
    "module-and-file",
    "module-and-private-file",
];

fn settings(project: &Value) -> Result<Value> {
    Ok(effective_settings(project, &json!({}), &json!({}))?["settings"].take())
}

fn run(request: &Value) -> Result<Value> {
    let options = &request["options"];
    let files: Vec<Value> = options.get("file").into_iter().cloned().collect();
    lookup(
        &settings(&request["project"])?,
        &files,
        options["module"].as_str(),
    )
}

#[test]
fn frozen_lookup_cases_replay_with_recorded_corrections() {
    let corpus: Value = serde_json::from_str(CORPUS).unwrap();
    let cases = corpus["cases"].as_array().unwrap();
    for case in cases {
        let id = case["id"].as_str().unwrap();
        let mut request = case["request"].clone();
        normalize_numbers(&mut request);
        let response = &case["response"];
        let expected_ok = response["ok"].as_bool().unwrap() && !CORRECTIONS.contains(&id);
        match (run(&request), expected_ok) {
            (Ok(result), true) => assert_eq!(
                serde_json::to_string(&result).unwrap(),
                serde_json::to_string(&response["result"]).unwrap(),
                "{id}"
            ),
            (Err(_), false) => {}
            (actual, _) => panic!("{id}: expected {response}, got {actual:?}"),
        }
    }
    assert_eq!(cases.len(), 42);
}

#[test]
fn every_file_selects_its_modules_and_private_files_are_skipped() {
    let settings = settings(&json!({"formatVersion": 1, "modules": [
        {"name": "root", "path": "."},
        {"name": "api", "path": "src/api"},
        {"name": "client", "path": "client"}
    ]}))
    .unwrap();
    let names = |files: &[&str]| {
        let files: Vec<Value> = files.iter().map(|file| json!(file)).collect();
        let result = lookup(&settings, &files, None).unwrap();
        let modules = result["modules"].as_array().unwrap().iter();
        (
            modules
                .map(|module| module["name"].as_str().unwrap().to_string())
                .collect::<Vec<_>>(),
            result["availability"].as_str().unwrap().to_string(),
        )
    };
    assert_eq!(
        names(&["client/a", "src/api/b", "client/c"]).0,
        ["client", "api"]
    );
    assert_eq!(
        names(&[".git/x", "src/api/b"]),
        (vec!["api".to_string()], DECLARED.to_string())
    );
    assert_eq!(
        names(&[".git/x", ".craft/state/y"]),
        (vec![], PRIVATE.to_string())
    );
    assert!(lookup(&settings, &[json!("ok"), json!("../no")], None).is_err());
}

#[test]
fn private_paths_match_whole_components() {
    for path in [
        ".git",
        "a/.git/b",
        ".craft/state",
        "x/.capabilities/state/y",
    ] {
        assert!(private_path(path), "{path}");
    }
    for path in [
        ".gitignore",
        "x.git/y",
        ".craft/project.json",
        ".craft/states",
        "state/.craft",
    ] {
        assert!(!private_path(path), "{path}");
    }
}
