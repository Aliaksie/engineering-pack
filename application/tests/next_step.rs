//! The turn-end notice names one next step when Craft holds a fact that calls for it: a
//! declared check covering files the turn changed that has not run since they changed, or a
//! context window past its compaction threshold with where the checkpoint stands. Each is said
//! once while it applies, and nothing is said without such a fact.
mod support;
use serde_json::{Value, json};
use std::path::Path;
use support::Repo;

const PROJECT: &str = r#"{"formatVersion":1,
  "modules":[{"name":"api","path":"api","checks":[{"command":"git","args":["--version"],"id":"api-build"}]}]}"#;

fn hook(lifecycle: &str, repo: &Repo, extra: Value) -> String {
    let mut payload = json!({"session_id": "S1", "cwd": repo.path()});
    for (key, value) in extra.as_object().unwrap() {
        payload[key] = value.clone();
    }
    let args: Vec<String> = [
        "claude",
        lifecycle,
        "--package",
        "craft-delivery",
        "--handler",
        "deliver",
    ]
    .iter()
    .map(|a| a.to_string())
    .collect();
    let (wire, _) = craft_application::hook::run(&args, &mut payload.to_string().as_bytes());
    assert_eq!(wire.exit_code, 0, "{lifecycle}: {}", wire.stderr);
    wire.stdout
}

/// What the turn's end shows the user, `None` when it says nothing.
fn notice(repo: &Repo) -> Option<String> {
    let said = hook("turn-end", repo, json!({}));
    if said.is_empty() {
        return None;
    }
    let output: Value = serde_json::from_str(&said).unwrap();
    assert!(output.get("decision").is_none(), "stop held: {said}");
    output["systemMessage"].as_str().map(str::to_owned)
}

fn started(name: &str, files: &[(&str, &str)]) -> Repo {
    let repo = Repo::new(name, files);
    hook("session-start", &repo, json!({"source": "startup"}));
    repo
}

/// A context observation from the host, as its status line records it.
fn observed(repo: &Repo, tokens: u64) {
    let file = craft_platform::state::file(
        Path::new(repo.path()),
        "host-context.json",
        true,
        Some("S1"),
    )
    .unwrap();
    let at = support::now();
    let record = json!({"formatVersion": 1, "host": "claude", "sessionId": "S1", "observedAt": at,
        "usage": {"kind": "current-context", "currentTokens": tokens, "capacityTokens": 1000,
            "source": "claude-statusline", "observedAt": at, "sessionId": "S1"}});
    std::fs::write(file, record.to_string()).unwrap();
}

/// A file the session writes in a tool call: the host reports the call before and after.
fn wrote(repo: &Repo, file: &str, text: &str) {
    let call = json!({"tool_name": "Bash", "tool_input": {"command": "sh ./write.sh"}});
    hook("before-tool", repo, call.clone());
    repo.write(file, text);
    hook("after-tool", repo, call);
}

const CHECK: &str = "Craft next: craft check --stage commit (1 declared check covers files changed since it last ran)";

#[test]
fn a_changed_file_a_check_covers_names_the_check_until_it_runs() {
    let repo = started(
        "next-check",
        &[
            (".craft/project.json", PROJECT),
            ("api/a.txt", "a\n"),
            ("docs/a.md", "a\n"),
        ],
    );
    assert_eq!(notice(&repo), None, "nothing changed");
    wrote(&repo, "docs/a.md", "b\n");
    assert_eq!(notice(&repo), None, "no check covers the change");
    wrote(&repo, "api/a.txt", "b\n");
    assert_eq!(notice(&repo).as_deref(), Some(CHECK));
    assert_eq!(notice(&repo), None, "said once");
    let ran = repo.cli(&["check", "--session", "S1", "--json"]).unwrap();
    assert_eq!(ran["result"], "passed", "{ran}");
    std::thread::sleep(std::time::Duration::from_millis(20));
    wrote(&repo, "api/a.txt", "c\n");
    assert_eq!(
        notice(&repo).as_deref(),
        Some(CHECK),
        "changed again after the run, said again"
    );
    repo.cli(&["check", "--session", "S1", "--json"]).unwrap();
    assert_eq!(notice(&repo), None, "ran on the change");
}

#[test]
fn the_switches_of_session_advice_and_project_checks_silence_it() {
    for switch in [
        r#""efficiency":{"advice":{"nextStep":false}}"#,
        r#""routing":{"projectChecks":false}"#,
    ] {
        let project = PROJECT.replacen(
            "{\"formatVersion\":1,",
            &format!("{{\"formatVersion\":1,{switch},"),
            1,
        );
        let repo = started(
            "next-off",
            &[(".craft/project.json", &project), ("api/a.txt", "a\n")],
        );
        wrote(&repo, "api/a.txt", "b\n");
        assert_eq!(notice(&repo), None, "{switch}");
    }
}

#[test]
fn a_full_context_names_compaction_only_with_a_checkpoint_on_this_commit() {
    let repo = started("next-context", &[("a.txt", "a\n")]);
    observed(&repo, 700);
    assert_eq!(notice(&repo), None, "below the threshold");
    observed(&repo, 800);
    assert_eq!(
        notice(&repo).as_deref(),
        Some("Craft next: context 80 %: save a checkpoint before compacting")
    );
    let continuation: serde_json::Map<String, Value> = craft_engine::checkpoint::CONTINUATION
        .iter()
        .map(|field| (field.to_string(), json!(format!("the {field}"))))
        .collect();
    let data = repo.input(
        "handoff.json",
        &json!({"task": "next step", "continuation": continuation}),
    );
    repo.cli(&["checkpoint-save", "--session", "S1", "--data", &data])
        .unwrap();
    observed(&repo, 810);
    assert_eq!(
        notice(&repo).as_deref(),
        Some(
            "Craft next: context 81 %: compact or start a new session; the session's checkpoint is on this commit"
        )
    );
    repo.write("a.txt", "b\n");
    repo.git(&["commit", "-qam", "move on"]);
    assert_eq!(
        notice(&repo).as_deref(),
        Some(
            "Craft next: context 81 %: save a checkpoint before compacting; the last one is not on this commit"
        )
    );
}

#[test]
fn an_open_task_names_its_first_required_criterion_without_evidence() {
    let repo = started(
        "next-acceptance",
        &[("src/value.mjs", "export const value = 1;\n")],
    );
    let contract = repo.input(
        "contract.json",
        &serde_json::from_str::<Value>(include_str!(
            "../../tests/fixtures/acceptance/contract.json"
        ))
        .unwrap(),
    );
    repo.cli(&[
        "task-start",
        "--session",
        "S1",
        "--task",
        "fix",
        "--goal",
        "Fix it",
        "--file",
        "src",
        "--contract",
        &contract,
    ])
    .unwrap();
    assert_eq!(
        notice(&repo).as_deref(),
        Some("Craft next: acceptance \"value\" of task fix has no evidence yet")
    );
    assert_eq!(notice(&repo), None, "said once");
}

#[test]
fn a_completed_plan_task_names_the_next_ready_one() {
    let repo = started("next-plan", &[("a.txt", "a\n")]);
    let plan = repo.input(
        "plan.json",
        &json!({"tasks": [{"id": "first", "goal": "Add the API"},
            {"id": "second", "goal": "Document the API", "dependsOn": ["first"]}]}),
    );
    repo.cli(&[
        "plan-create",
        "--plan",
        "api",
        "--goal",
        "Ship the API",
        "--data",
        &plan,
    ])
    .unwrap();
    repo.cli(&[
        "task-start",
        "--session",
        "S1",
        "--plan",
        "api",
        "--task",
        "first",
    ])
    .unwrap();
    assert_eq!(notice(&repo), None, "the held task is open");
    let bundle = support::bundle();
    let done = repo.cli(&[
        "task-step",
        "--session",
        "S1",
        "--stage",
        "implement",
        "--target",
        "task:first",
        "--approach",
        "direct",
        "--evidence",
        "e1",
        "--outcome",
        "passed",
        "--result",
        "Added",
        "--next",
        "Document it",
        "--complete",
        "--bundle",
        &bundle,
    ]);
    assert!(done.is_ok(), "{done:?}");
    assert_eq!(
        notice(&repo).as_deref(),
        Some(
            "Craft next: plan task second (Document the API) is ready: craft task-start --plan api --ready"
        )
    );
}

/// Create plan `id` with `tasks`, claim each in session S1 in order and complete it.
fn worked_through(repo: &Repo, id: &str, tasks: &[(&str, &str)]) {
    let authored: Vec<Value> = tasks
        .iter()
        .map(|(task, goal)| json!({"id": task, "goal": goal}))
        .collect();
    let data = repo.input(&format!("{id}.json"), &json!({"tasks": authored}));
    repo.cli(&[
        "plan-create",
        "--plan",
        id,
        "--goal",
        "Ship",
        "--data",
        &data,
    ])
    .unwrap();
    let bundle = support::bundle();
    for (task, goal) in tasks {
        let started = repo.cli(&[
            "task-start",
            "--session",
            "S1",
            "--plan",
            id,
            "--task",
            task,
        ]);
        assert!(started.is_ok(), "{id}/{task}: {started:?}");
        assert_eq!(repo.session_state("S1", task)["taskRecord"]["goal"], *goal);
        let done = repo.cli(&[
            "task-step",
            "--session",
            "S1",
            "--stage",
            "implement",
            "--target",
            &format!("task:{task}"),
            "--approach",
            "direct",
            "--evidence",
            "e1",
            "--outcome",
            "passed",
            "--result",
            "Done",
            "--complete",
            "--bundle",
            &bundle,
        ]);
        assert!(done.is_ok(), "{id}/{task}: {done:?}");
    }
}

#[test]
fn a_plan_task_is_claimed_whatever_ids_earlier_plans_in_the_session_used() {
    let repo = started("plan-ids", &[("a.txt", "a\n")]);
    worked_through(
        &repo,
        "first",
        &[("rule", "Add the rule"), ("docs", "Document the rule")],
    );
    worked_through(
        &repo,
        "second",
        &[
            ("rule", "Add the command"),
            ("docs", "Document the command"),
        ],
    );

    // An earlier plan's task still held by this session keeps its record.
    let data = repo.input(
        "open.json",
        &json!({"tasks": [{"id": "edit", "goal": "Edit"}]}),
    );
    repo.cli(&[
        "plan-create",
        "--plan",
        "third",
        "--goal",
        "Ship",
        "--data",
        &data,
    ])
    .unwrap();
    repo.cli(&[
        "task-start",
        "--session",
        "S1",
        "--plan",
        "third",
        "--task",
        "edit",
    ])
    .unwrap();
    let data = repo.input(
        "fourth.json",
        &json!({"tasks": [{"id": "edit", "goal": "Other"}]}),
    );
    repo.cli(&[
        "plan-create",
        "--plan",
        "fourth",
        "--goal",
        "Ship",
        "--data",
        &data,
    ])
    .unwrap();
    let refused = repo.cli(&[
        "task-start",
        "--session",
        "S1",
        "--plan",
        "fourth",
        "--task",
        "edit",
    ]);
    assert!(refused.unwrap_err().contains("different goal"));

    // A task started without a plan keeps refusing a changed goal.
    let plain = repo.cli(&[
        "task-start",
        "--session",
        "S1",
        "--task",
        "docs",
        "--goal",
        "Other",
    ]);
    assert!(plain.unwrap_err().contains("different goal"));
}
