//! Replays `tests/fixtures/runtime/lookup-contracts.json`, captured from the JS use case.
use super::*;
use crate::{contracts::effective_settings, values::normalize_numbers};

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

const ROUTES: &str = include_str!("../../../tests/fixtures/runtime/route-contracts.json");

fn run_route(request: &Value) -> Result<Value> {
    let options = &request["options"];
    let strings = |name: &str| -> Vec<String> {
        options[name]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|item| item.as_str().map(str::to_string))
            .collect()
    };
    let files: Vec<Value> = options["file"].as_array().cloned().unwrap_or_default();
    route(
        &settings(&request["project"])?,
        &Request {
            intent: options["intent"].as_str(),
            files: &files,
            risks: &strings("risk"),
            host: options["host"].as_str().unwrap_or("agents"),
            delegate: options["delegate"] == true,
            task: options["task"].as_str(),
            verification_mode: options["verificationMode"].as_str(),
        },
    )
}

#[test]
fn frozen_route_cases_replay() {
    let corpus: Value = serde_json::from_str(ROUTES).unwrap();
    let cases = corpus["cases"].as_array().unwrap();
    let mut failures = Vec::new();
    for case in cases {
        let id = case["id"].as_str().unwrap();
        let mut request = case["request"].clone();
        normalize_numbers(&mut request);
        let response = &case["response"];
        match (run_route(&request), response["ok"].as_bool().unwrap()) {
            (Ok(result), true) if result == response["result"] => {
                assert_eq!(
                    serde_json::to_string(&result).unwrap(),
                    serde_json::to_string(&response["result"]).unwrap(),
                    "{id}: key order"
                );
            }
            (Err(_), false) => {}
            (actual, _) => failures.push(format!("{id}: got {actual:?}")),
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
    assert_eq!(cases.len(), 120);
}

#[test]
fn embedded_catalog_is_valid() {
    let catalog = catalog::catalog().unwrap();
    assert!(catalog::skills(catalog).contains(&"craft-engineering:foundations"));
}

#[test]
fn catalog_rejects_invalid_selectors_and_load_targets() {
    let original = catalog::catalog().unwrap();
    for (pointer, value) in [
        ("/formatVersion", json!(2)),
        ("/entries/0/id", json!("vendor:implement")),
        ("/entries/0/when", json!({})),
        ("/entries/0/when", json!({"framework": ["spring"]})),
        ("/entries/0/when/intents", json!(["invent"])),
        ("/entries/0/when/intents", json!(["implement", "implement"])),
        ("/entries/0/when", json!({"languages": ["java"]})),
        ("/entries/0/value/evidence", json!(" ")),
        (
            "/entries/0/references",
            json!(["craft-engineering:foundations/../../secret.md"]),
        ),
        ("/entries/0/skills", json!(["invalid"])),
        ("/layerExtensions/.tsx", json!("react")),
        ("/runtimeImplications/python", json!("missing")),
        ("/runtimeImplications/java", json!("missing")),
        ("/extensions/java", json!("java")),
        ("/entries/0/value/permission", json!("publish")),
        ("/entries/0/references", json!(["other:missing"])),
        (
            "/entries/0/fallbackSkills",
            json!({"unknown:primary": "craft-delivery:review-code"}),
        ),
    ] {
        let mut changed = original.clone();
        // Mappings need not already contain the test key.
        let (parent, key) = pointer.rsplit_once('/').unwrap();
        changed.pointer_mut(parent).unwrap()[key] = value;
        assert!(catalog::validate(&changed).is_err(), "{pointer}");
    }
    fn entry<'a>(catalog: &'a mut Value, id: &str) -> &'a mut Value {
        catalog["entries"]
            .as_array_mut()
            .unwrap()
            .iter_mut()
            .find(|entry| entry["id"] == id)
            .unwrap()
    }
    let mut duplicate = original.clone();
    let first = duplicate["entries"][0].clone();
    duplicate["entries"].as_array_mut().unwrap().push(first);
    let mut no_inputs = original.clone();
    no_inputs["entries"][0]["value"]
        .as_object_mut()
        .unwrap()
        .shift_remove("inputs");
    let mut story_runtime = original.clone();
    entry(&mut story_runtime, "runtime:jvm")["when"]["intents"] = json!(["story", "implement"]);
    let mut loads_nothing = original.clone();
    entry(&mut loads_nothing, "runtime:node")
        .as_object_mut()
        .unwrap()
        .shift_remove("contextOnly");
    for (name, changed) in [
        ("duplicate entry", duplicate),
        ("missing value field", no_inputs),
        ("runtime entry for a non-stack intent", story_runtime),
        ("entry without load targets", loads_nothing),
    ] {
        assert!(catalog::validate(&changed).is_err(), "{name}");
    }
    for scope in [".", "a", ".github/workflows", "automation/release.yml"] {
        let mut changed = original.clone();
        let entry = changed["entries"]
            .as_array_mut()
            .unwrap()
            .iter_mut()
            .find(|entry| entry["id"] == "practice:github-actions-path")
            .unwrap();
        entry["when"] = json!({"paths": [scope]});
        assert!(catalog::validate(&changed).is_ok(), "{scope}");
    }
    for scope in [
        "",
        "..",
        "./ci",
        "ci/../release",
        "ci/./release",
        "ci//release",
        "ci\\release",
        "ci/**",
        "ci\0file",
        "ci/",
        "/ci",
        "C:/ci",
        "ci/*.yml",
        "ci/[ab]",
        "ci/{a,b}",
        "ci?",
    ] {
        let mut changed = original.clone();
        let entry = changed["entries"]
            .as_array_mut()
            .unwrap()
            .iter_mut()
            .find(|entry| entry["id"] == "practice:github-actions-path")
            .unwrap();
        entry["when"] = json!({"paths": [scope]});
        assert!(catalog::validate(&changed).is_err(), "{scope}");
    }
}

#[test]
fn path_selectors_cannot_borrow_provider_evidence_from_another_module() {
    let settings = settings(&json!({"formatVersion": 1, "modules": [
        {"name": "ci", "path": ".github", "providers": []},
        {"name": "agent", "path": "agent", "providers": ["bedrock"]}]}))
    .unwrap();
    let files = vec![
        ".github/workflows/ci.yml".to_string(),
        "agent/main.py".to_string(),
    ];
    let modules = matching_modules(&settings, &files);
    let practices = |catalog: &Value| {
        let request = loading::Request {
            intent: "review",
            files: &files,
            modules: &modules,
            settings: &settings,
            risks: &[],
            verification_mode: None,
        };
        loading::plan(&request, catalog)["practices"].clone()
    };
    let id = json!("practice:github-actions-path");
    let mut catalog = catalog::catalog().unwrap().clone();
    assert!(practices(&catalog).as_array().unwrap().contains(&id));
    let entry = catalog["entries"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|entry| entry["id"] == id)
        .unwrap();
    entry["when"]["providers"] = json!(["bedrock"]);
    assert!(!practices(&catalog).as_array().unwrap().contains(&id));
}
