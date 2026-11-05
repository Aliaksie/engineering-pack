//! Resume probe: a task worked to a recorded step, then its context compacted or its session
//! replaced. What the model receives must carry the goal, current decisions, finished and
//! open steps with their evidence and the next step, once, within its budget, and must say
//! when source changed so passed checks are rerun only then.
mod support;
use craft_engine::session::resume::RESUME_BYTES;
use serde_json::{Value, json};
use support::{Repo, bundle};

/// A delivery hook event as the native `host` handler receives it.
fn hook(repo: &Repo, host: &str, lifecycle: &str, session: &str, extra: Value) -> Value {
    let context_supported = !matches!(
        (host, lifecycle),
        (_, "before-compact" | "turn-end") | ("cursor" | "copilot", "before-prompt")
    );
    let mut facts = json!({
        "handler": "deliver", "host": host, "package": "craft-delivery", "lifecycle": lifecycle,
        "scope": "any", "inputStatus": "complete", "tool": "read", "contextSupported": context_supported,
        "cwd": repo.path(), "session": session, "file": "", "prompt": "", "pattern": null,
    });
    for (key, value) in extra.as_object().unwrap() {
        facts[key] = value.clone();
    }
    craft_application::host::run(&facts).unwrap()
}

fn resumed(decision: &Value) -> Option<&Value> {
    (decision["context"]["kind"] == "resume").then(|| &decision["context"]["data"])
}

/// A task with two decisions (one superseded), a passed and a failed step.
fn worked(repo: &Repo, session: &str) {
    let ok = |args: &[&str]| repo.cli(args).unwrap();
    ok(&[
        "task-start",
        "--session",
        session,
        "--task",
        "fix-value",
        "--goal",
        "Correct the value",
        "--file",
        "value.mjs",
    ]);
    repo.write("value.mjs", "export const value = 2;\n");
    ok(&[
        "task-decide",
        "--session",
        session,
        "--decision",
        "Rename to amount",
        "--by",
        "agent",
    ]);
    ok(&[
        "task-decide",
        "--session",
        session,
        "--decision",
        "Keep the exported name",
        "--why",
        "Two callers import it",
        "--rejected",
        "Rename to amount",
        "--by",
        "user",
        "--supersedes",
        "d1",
    ]);
    ok(&[
        "task-decide",
        "--session",
        session,
        "--decision",
        "Treat 0 as valid",
        "--by",
        "agent",
    ]);
    let bundle = bundle();
    ok(&[
        "task-step",
        "--session",
        session,
        "--bundle",
        &bundle,
        "--stage",
        "implement",
        "--target",
        "acceptance:value",
        "--approach",
        "bounded-fix",
        "--evidence",
        "checks/value-1",
        "--outcome",
        "passed",
        "--result",
        "Value corrected",
        "--next",
        "Verify acceptance",
    ]);
    ok(&[
        "task-step",
        "--session",
        session,
        "--bundle",
        &bundle,
        "--stage",
        "verify",
        "--target",
        "acceptance:zero",
        "--approach",
        "unit-test",
        "--evidence",
        "checks/value-2",
        "--outcome",
        "quality-failure",
        "--result",
        "Edge case 0 fails",
        "--next",
        "Handle zero",
    ]);
}

fn assert_resume(resume: &Value) {
    assert_eq!(resume["goal"], "Correct the value");
    assert_eq!(resume["done"], "Value corrected");
    assert_eq!(resume["open"], "Edge case 0 fails");
    assert_eq!(resume["next"], "Handle zero");
    let decisions: Vec<&str> = resume["decisions"]
        .as_array()
        .unwrap()
        .iter()
        .map(|d| d["text"].as_str().unwrap())
        .collect();
    assert_eq!(decisions, ["Keep the exported name", "Treat 0 as valid"]);
    assert_eq!(resume["decisions"][0]["why"], "Two callers import it");
    assert_eq!(resume["decisions"][0]["by"], "user");
    assert_eq!(
        resume["steps"],
        json!([
            "implement acceptance:value passed (checks/value-1)",
            "verify acceptance:zero quality-failure (checks/value-2)"
        ])
    );
    assert!(resume.to_string().len() <= RESUME_BYTES);
}

#[test]
fn compaction_resumes_the_task_once_with_its_decisions() {
    let repo = Repo::new(
        "resume-compact",
        &[("value.mjs", "export const value = 1;\n")],
    );
    worked(&repo, "S1");
    hook(&repo, "claude", "before-compact", "S1", json!({}));
    let restored = hook(&repo, "claude", "after-compact", "S1", json!({}));
    let resume = resumed(&restored).expect("resume after compaction");
    assert_resume(resume);
    assert_eq!(resume["source"], "unchanged since the last checkpoint");
    assert_eq!(restored["session"]["task"], "fix-value");
    // Raw fingerprints stay in checkpoint-read; the model gets the task.
    assert!(!restored["context"].to_string().contains("fingerprints"));
    let prompt = hook(
        &repo,
        "claude",
        "before-prompt",
        "S1",
        json!({"prompt": "continue"}),
    );
    assert!(resumed(&prompt).is_none(), "delivered once: {prompt}");
}

#[test]
fn a_host_without_a_restore_event_resumes_at_the_next_event_that_can_show_it() {
    // Cursor and Copilot cannot show prompt context, so the resume waits for a tool result.
    for host in ["cursor", "copilot"] {
        let repo = Repo::new(
            &format!("resume-{host}"),
            &[("value.mjs", "export const value = 1;\n")],
        );
        worked(&repo, "S1");
        hook(&repo, host, "before-compact", "S1", json!({}));
        let prompt = hook(
            &repo,
            host,
            "before-prompt",
            "S1",
            json!({"prompt": "continue"}),
        );
        assert!(resumed(&prompt).is_none(), "{host}");
        let tool = hook(&repo, host, "after-tool", "S1", json!({}));
        assert_resume(resumed(&tool).expect("resume at the first tool result"));
        let again = hook(&repo, host, "after-tool", "S1", json!({}));
        assert!(resumed(&again).is_none(), "{host} delivered once: {again}");
    }
}

#[test]
fn a_session_without_a_task_has_nothing_to_resume() {
    let repo = Repo::new("resume-none", &[("value.mjs", "export const value = 1;\n")]);
    hook(&repo, "gemini", "before-compact", "S1", json!({}));
    let prompt = hook(
        &repo,
        "gemini",
        "before-prompt",
        "S1",
        json!({"prompt": "continue"}),
    );
    assert!(resumed(&prompt).is_none());
}

#[test]
fn a_new_session_adopts_the_task_with_its_decisions_and_learns_when_source_changed() {
    let repo = Repo::new(
        "resume-adopt",
        &[("value.mjs", "export const value = 1;\n")],
    );
    worked(&repo, "S1");
    hook(&repo, "claude", "turn-end", "S1", json!({}));
    let started = hook(&repo, "claude", "session-start", "S2", json!({}));
    let offered = &started["session"]["handoff"][0];
    assert_eq!(offered["task"], "fix-value");
    let from = offered["from"].as_str().unwrap();
    let adopted = repo
        .cli(&["task-adopt", "--session", "S2", "--from", from])
        .unwrap();
    assert_resume(&adopted["resume"]);
    assert_eq!(
        adopted["resume"]["source"],
        "unchanged since the last checkpoint"
    );

    // Source edited after the steps were recorded: passed checks need rerunning.
    repo.write("value.mjs", "export const value = 3;\n");
    let restored = hook(&repo, "claude", "after-compact", "S2", json!({}));
    let resume = resumed(&restored).expect("resume after compaction");
    assert!(
        resume["source"].as_str().unwrap().contains("recheck"),
        "{resume}"
    );
}

#[test]
fn decisions_need_a_bound_task_and_refuse_options_elsewhere() {
    let repo = Repo::new(
        "resume-refuse",
        &[("value.mjs", "export const value = 1;\n")],
    );
    let unbound = repo.cli(&[
        "task-decide",
        "--session",
        "S1",
        "--decision",
        "X",
        "--by",
        "user",
    ]);
    assert!(unbound.unwrap_err().contains("bound task"));
    let elsewhere = repo.cli(&["route", "--why", "because"]);
    assert!(elsewhere.unwrap_err().contains("task-decide"));
}
