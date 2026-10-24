//! Replays `tests/fixtures/runtime/context-contracts.json`, captured from the JS use case,
//! through the `context-report` command over rebuilt bundle trees.
use craft_application::cli;
use serde_json::{Value, json};
use std::fs;
use std::path::{Path, PathBuf};

const CORPUS: &str = include_str!("../../tests/fixtures/runtime/context-contracts.json");

struct Scratch(PathBuf);
impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn materialize(root: &Path, files: &Value) {
    fs::create_dir_all(root).unwrap();
    for (file, content) in files.as_object().unwrap() {
        let path = root.join(file);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        let text = match content {
            Value::String(text) => text.clone(),
            repeated => repeated["repeat"]
                .as_str()
                .unwrap()
                .repeat(repeated["count"].as_u64().unwrap() as usize),
        };
        fs::write(path, text).unwrap();
    }
}

/// Replace `<bundle:N>` placeholders with the physical roots of this run.
fn substitute(value: &mut Value, roots: &[String]) {
    match value {
        Value::String(text) => {
            if let Some(index) = text
                .strip_prefix("<bundle:")
                .and_then(|rest| rest.strip_suffix('>'))
            {
                *text = roots[index.parse::<usize>().unwrap()].clone();
            }
        }
        Value::Array(items) => items.iter_mut().for_each(|item| substitute(item, roots)),
        Value::Object(map) => map.values_mut().for_each(|item| substitute(item, roots)),
        _ => {}
    }
}

fn argv(request: &Value, project: &Path, bundles: &[String]) -> Vec<String> {
    let options = &request["options"];
    let mut argv = vec![
        "context-report".to_string(),
        "--root".into(),
        project.display().to_string(),
    ];
    argv.extend([
        "--preferences".into(),
        project.join("absent.json").display().to_string(),
    ]);
    for (option, flag) in [
        ("intent", "--intent"),
        ("host", "--host"),
        ("verificationMode", "--verification-mode"),
        ("budgetBytes", "--budget-bytes"),
    ] {
        if let Some(value) = options[option].as_str() {
            argv.extend([flag.to_string(), value.to_string()]);
        }
    }
    for (option, flag) in [("file", "--file"), ("risk", "--risk")] {
        for value in options[option].as_array().into_iter().flatten() {
            argv.extend([flag.to_string(), value.as_str().unwrap().to_string()]);
        }
    }
    for index in request["roots"].as_array().unwrap() {
        argv.extend([
            "--bundle".into(),
            bundles[index.as_u64().unwrap() as usize].clone(),
        ]);
    }
    argv
}

#[test]
fn frozen_context_cases_replay_through_the_command() {
    let scratch = Scratch(
        std::env::temp_dir().join(format!("craft-context-contracts-{}", std::process::id())),
    );
    let corpus: Value = serde_json::from_str(CORPUS).unwrap();
    let cases = corpus["cases"].as_array().unwrap();
    let mut failures = Vec::new();
    for (number, case) in cases.iter().enumerate() {
        let id = case["id"].as_str().unwrap();
        let request = &case["request"];
        let base = scratch.0.join(number.to_string());
        let project = base.join("project");
        fs::create_dir_all(project.join(".craft")).unwrap();
        fs::write(
            project.join(".craft/project.json"),
            request["project"].to_string(),
        )
        .unwrap();
        fs::write(project.join(".craft/telemetry.json"), r#"{"local":false}"#).unwrap();
        let bundles: Vec<String> = request["bundles"]
            .as_array()
            .unwrap()
            .iter()
            .enumerate()
            .map(|(index, files)| {
                let root = base.join(format!("bundle-{index}"));
                materialize(&root, files);
                craft_platform::state::physical(&root)
                    .unwrap()
                    .display()
                    .to_string()
            })
            .collect();
        let outcome = cli::run(&argv(request, &project, &bundles));
        let response = &case["response"];
        if response["ok"] == true {
            let mut expected = response["result"].clone();
            substitute(&mut expected, &bundles);
            let actual: Option<Value> = serde_json::from_str(&outcome.stdout).ok();
            if outcome.exit_code != 0
                || actual.as_ref().map(Value::to_string) != Some(expected.to_string())
            {
                failures.push(format!(
                    "{id}: exit {} {}\n  expected {expected}\n  actual   {}",
                    outcome.exit_code, outcome.stderr, outcome.stdout
                ));
            } else if outcome.activity != Some(response["activity"].clone()) {
                failures.push(format!("{id}: activity {:?}", outcome.activity));
            }
        } else if outcome.exit_code != 2 || !outcome.stdout.is_empty() {
            failures.push(format!(
                "{id}: expected failure, got exit {}",
                outcome.exit_code
            ));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
    assert_eq!(cases.len(), 54);
}

#[test]
fn direct_invocation_requires_an_explicit_bundle() {
    let scratch =
        Scratch(std::env::temp_dir().join(format!("craft-context-direct-{}", std::process::id())));
    fs::create_dir_all(scratch.0.join(".craft")).unwrap();
    fs::write(
        scratch.0.join(".craft/telemetry.json"),
        r#"{"local":false}"#,
    )
    .unwrap();
    let root = scratch.0.display().to_string();
    let outcome = cli::run(&[
        "context-report".into(),
        "--root".into(),
        root,
        "--intent".into(),
        "locate".into(),
    ]);
    assert_eq!(outcome.exit_code, 2);
    assert!(outcome.stderr.contains("1 to 16 explicit bundle roots"));
    assert_eq!(json!(outcome.activity), Value::Null);
}
