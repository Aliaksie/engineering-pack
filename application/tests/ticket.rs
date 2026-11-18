//! Work that starts from a ticket, as a session meets it: the prompt naming the ticket tells
//! the agent to state its facts, `craft ticket` decides direct, plan-first or ask and words
//! the next step for the host, and a planned change the user accepted ends with a review offer.
mod support;
use serde_json::{Value, json};
use support::Repo;

const PROJECT: &str = r#"{"formatVersion":1,"modules":[
  {"name":"engine","path":"engine"},{"name":"application","path":"application"}]}"#;
const ISSUE: &str = "https://github.com/acme/shop/issues/145";

fn hook(host: &str, lifecycle: &str, repo: &Repo, extra: Value) -> Value {
    let mut payload = json!({"session_id": "S1", "cwd": repo.path()});
    for (key, value) in extra.as_object().unwrap() {
        payload[key] = value.clone();
    }
    let args: Vec<String> = [
        host,
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
    if wire.stdout.is_empty() {
        return Value::Null;
    }
    serde_json::from_str(&wire.stdout).unwrap()
}

fn context(output: &Value) -> String {
    output["hookSpecificOutput"]["additionalContext"]
        .as_str()
        .unwrap_or_default()
        .to_owned()
}

fn prompted(repo: &Repo, prompt: &str) -> String {
    context(&hook(
        "claude",
        "before-prompt",
        repo,
        json!({"prompt": prompt, "hook_event_name": "UserPromptSubmit"}),
    ))
}

#[test]
fn a_prompt_that_names_a_ticket_asks_for_its_facts_before_any_edit() {
    let repo = Repo::new("ticket-intake", &[(".craft/project.json", PROJECT)]);
    hook(
        "claude",
        "session-start",
        &repo,
        json!({"source": "startup"}),
    );
    let said = prompted(&repo, &format!("let's implement {ISSUE}"));
    assert!(said.contains("Craft ticket intake"), "{said}");
    assert!(
        said.contains(&format!("craft ticket --ticket {ISSUE} --criteria")),
        "{said}"
    );
    assert!(said.contains("--host claude --session S1"), "{said}");
    assert!(!prompted(&repo, "fix the parser").contains("ticket intake"));

    // Turned off by the project.
    let off = Repo::new(
        "ticket-off",
        &[(
            ".craft/project.json",
            r#"{"formatVersion":1,"routing":{"ticketOffer":false}}"#,
        )],
    );
    hook(
        "claude",
        "session-start",
        &off,
        json!({"source": "startup"}),
    );
    assert!(!prompted(&off, &format!("implement {ISSUE}")).contains("ticket intake"));
}

#[test]
fn the_decision_follows_the_tickets_facts_and_the_hosts_question_tool() {
    let repo = Repo::new("ticket-decide", &[(".craft/project.json", PROJECT)]);
    let decide = |host: &str, extra: &[&str]| {
        let mut args = vec![
            "ticket",
            "--ticket",
            ISSUE,
            "--criteria",
            "clear",
            "--host",
            host,
            "--session",
            "S1",
        ];
        args.extend_from_slice(extra);
        repo.cli(&args).unwrap()
    };
    // One module, no contract, clear criteria: direct, one line for the user.
    let direct = decide("claude", &["--path", "application/src/hook.rs"]);
    assert_eq!(direct["mode"], "direct", "{direct}");
    assert_eq!(
        direct["line"],
        "Craft: direct delivery (1 module, no contract change, clear acceptance criteria)"
    );
    // Two modules and the published schema: plan first, asked with the host's question tool.
    let plan = decide(
        "claude",
        &[
            "--path",
            "engine/src/contracts.rs",
            "--path",
            "application/src/project.rs",
            "--path",
            "schemas/project.schema.json",
        ],
    );
    assert_eq!(plan["mode"], "plan", "{plan}");
    assert_eq!(
        plan["line"],
        "Craft suggests: plan → implement → review (2 modules, schema change)"
    );
    let next = plan["next"].as_str().unwrap();
    assert!(next.contains("Ask the user with AskUserQuestion"), "{next}");
    assert!(
        next.contains(&format!(
            "craft ticket --ticket {ISSUE} --accept plan --session S1"
        )),
        "{next}"
    );
    // A host without a question tool: a visible line, asked in the reply.
    let codex = decide(
        "codex",
        &["--path", "engine/a.rs", "--path", "application/b.rs"],
    );
    assert!(
        codex["next"]
            .as_str()
            .unwrap()
            .starts_with("Tell the user this line as is"),
        "{codex}"
    );
    // Open questions come first; a state format the agent names is a contract too.
    let ask = decide("claude", &["--questions", "2"]);
    assert_eq!(ask["mode"], "ask", "{ask}");
    let stated = decide("claude", &["--affects", "state", "--path", "engine/a.rs"]);
    assert_eq!(stated["reasons"], json!(["state change"]), "{stated}");

    let missing = repo.cli(&["ticket", "--ticket", ISSUE]).unwrap_err();
    assert!(missing.contains("--criteria"), "{missing}");
    let elsewhere = repo.cli(&["route", "--criteria", "clear"]).unwrap_err();
    assert!(elsewhere.contains("supported by ticket"), "{elsewhere}");
}

#[test]
fn an_accepted_plan_ends_the_change_with_a_review_offer_said_once() {
    let repo = Repo::new(
        "ticket-review",
        &[
            (".craft/project.json", PROJECT),
            ("engine/a.rs", "fn a() {}\n"),
        ],
    );
    hook(
        "claude",
        "session-start",
        &repo,
        json!({"source": "startup"}),
    );
    let accepted = repo
        .cli(&[
            "ticket",
            "--ticket",
            ISSUE,
            "--accept",
            "plan",
            "--session",
            "S1",
        ])
        .unwrap();
    assert_eq!(accepted["delivery"], "plan", "{accepted}");
    assert!(
        accepted["next"]
            .as_str()
            .unwrap()
            .contains("craft-delivery:review-code"),
        "{accepted}"
    );
    // The same ticket in a later prompt is not taken in again.
    assert!(!prompted(&repo, &format!("now comment on {ISSUE}")).contains("ticket intake"));

    let notice = |repo: &Repo| {
        hook("claude", "turn-end", repo, json!({}))["systemMessage"]
            .as_str()
            .map(str::to_owned)
    };
    assert_eq!(notice(&repo), None, "nothing changed yet");
    let call = json!({"tool_name": "Bash", "tool_input": {"command": "sh ./write.sh"}});
    hook("claude", "before-tool", &repo, call.clone());
    repo.write("engine/a.rs", "fn a() { b() }\n");
    hook("claude", "after-tool", &repo, call);
    assert_eq!(
        notice(&repo).as_deref(),
        Some(
            "Craft next: offer the user a review of https://github.com/acme/shop/issues/145 before reporting it done (craft-delivery:review-code)"
        )
    );
    assert_eq!(notice(&repo), None, "said once");

    let refused = repo
        .cli(&["ticket", "--ticket", ISSUE, "--accept", "plan"])
        .unwrap_err();
    assert!(refused.contains("--session"), "{refused}");
}

/// A stop that first names a declared check to run keeps the review due: the next stop names
/// it, though no file changed since.
#[test]
fn a_review_offer_waits_for_a_check_reminder_and_comes_next() {
    let project = r#"{"formatVersion":1,"modules":[{"name":"engine","path":"engine",
      "checks":[{"command":"git","args":["--version"],"id":"engine-build"}]}]}"#;
    let repo = Repo::new(
        "ticket-review-later",
        &[
            (".craft/project.json", project),
            ("engine/a.rs", "fn a() {}\n"),
        ],
    );
    hook(
        "claude",
        "session-start",
        &repo,
        json!({"source": "startup"}),
    );
    repo.cli(&[
        "ticket",
        "--ticket",
        ISSUE,
        "--accept",
        "plan",
        "--session",
        "S1",
    ])
    .unwrap();
    let notice = |repo: &Repo| {
        hook("claude", "turn-end", repo, json!({}))["systemMessage"]
            .as_str()
            .map(str::to_owned)
    };
    let call = json!({"tool_name": "Bash", "tool_input": {"command": "sh ./write.sh"}});
    hook("claude", "before-tool", &repo, call.clone());
    repo.write("engine/a.rs", "fn a() { b() }\n");
    hook("claude", "after-tool", &repo, call);
    let first = notice(&repo).unwrap_or_default();
    assert!(first.contains("craft check --stage commit"), "{first}");
    repo.cli(&["check", "--session", "S1", "--json"]).unwrap();
    let second = notice(&repo).unwrap_or_default();
    assert!(
        second.contains(&format!("offer the user a review of {ISSUE}")),
        "{second}"
    );
    assert_eq!(notice(&repo), None, "said once");
}
