//! A subagent the host starts hears the standards of the task its delegating session is bound
//! to: the files a started task names, and the owned paths of a plan task.
mod support;
use serde_json::{Value, json};
use support::Repo;

const PROJECT: &str = r#"{"formatVersion":1,"standards":[
  {"id":"ui-copy","file":"docs/copy.md","scope":{"paths":["ui"]}},
  {"id":"api","file":"docs/api.md","scope":{"paths":["src/a"]}}
]}"#;

fn agent_start(repo: &Repo, session: &str) -> Value {
    craft_application::host::run(&json!({
        "handler": "deliver", "host": "claude", "package": "craft-delivery", "lifecycle": "agent-start",
        "scope": "any", "inputStatus": "complete", "tool": "other", "contextSupported": true,
        "cwd": repo.path(), "session": session, "file": null, "prompt": "", "pattern": null,
    }))
    .unwrap()
}

fn ids(decision: &Value) -> Vec<&str> {
    decision["delegation"]["standards"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|standard| standard["id"].as_str())
        .collect()
}

#[test]
fn the_bound_task_scope_reaches_a_host_started_subagent() {
    let repo = Repo::new(
        "delegation",
        &[
            (".craft/project.json", PROJECT),
            ("ui/app.tsx", "export {};\n"),
            ("src/a/api.mjs", "export {};\n"),
        ],
    );
    let ok = |args: &[&str]| repo.cli(args).unwrap();

    // A started task without a plan: its files carry the scope, with no intent established.
    ok(&[
        "task-start",
        "--session",
        "one",
        "--task",
        "copy",
        "--goal",
        "Fix the copy",
        "--file",
        "ui/app.tsx",
    ]);
    let started = agent_start(&repo, "one");
    assert_eq!(ids(&started), vec!["ui-copy"]);

    // A plan task: its owned paths carry the scope.
    let plan = repo.input(
        "plan.json",
        &json!({"tasks": [{"id": "c1", "goal": "Add the API", "scope": {"paths": ["src/a/**"]}}]}),
    );
    ok(&[
        "plan-create",
        "--plan",
        "split",
        "--goal",
        "Split the value",
        "--data",
        &plan,
    ]);
    ok(&[
        "task-start",
        "--session",
        "two",
        "--plan",
        "split",
        "--task",
        "c1",
    ]);
    assert_eq!(ids(&agent_start(&repo, "two")), vec!["api"]);

    // An unbound session with no established work carries nothing.
    assert!(agent_start(&repo, "three").get("delegation").is_none());
}
