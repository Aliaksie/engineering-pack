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

/// The ticket's plan recorded with one task and approved by the user, so files may change.
fn approved_plan(repo: &Repo) {
    let tasks = repo.input(
        "approved.json",
        &json!({"tasks": [{"id": "a", "goal": "Change a"}]}),
    );
    repo.cli(&[
        "plan-create",
        "--plan",
        "issue-145",
        "--goal",
        "Ship",
        "--data",
        &tasks,
    ])
    .unwrap();
    repo.cli(&["ticket", "--ticket", ISSUE, "--approve", "--session", "S1"])
        .unwrap();
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
            .contains("--approve --session S1"),
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
    approved_plan(&repo);
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
    approved_plan(&repo);
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

/// One host's tool call before it runs, as the deny it answers with or its plain output.
fn before(host: &str, repo: &Repo, tool: &str, input: Value) -> String {
    let args: Vec<String> = [
        host,
        "before-tool",
        "--package",
        "craft-delivery",
        "--handler",
        "deliver",
    ]
    .iter()
    .map(|a| a.to_string())
    .collect();
    let payload = json!({"session_id": "S1", "cwd": repo.path(), "hook_event_name": "PreToolUse",
        "tool_name": tool, "tool_input": input});
    let (wire, _) = craft_application::hook::run(&args, &mut payload.to_string().as_bytes());
    match wire.exit_code {
        2 => format!("deny {}", wire.stderr),
        0 if wire.stdout.contains(r#""deny""#) => format!("deny {}", wire.stdout),
        0 => wire.stdout,
        code => panic!("exit {code}: {}", wire.stderr),
    }
}

#[test]
fn a_planned_ticket_holds_file_edits_until_the_user_approves_its_plan() {
    let repo = Repo::new(
        "ticket-approval",
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
    let ticket = |extra: &[&str]| {
        let mut args = vec!["ticket", "--ticket", ISSUE, "--session", "S1"];
        args.extend(extra);
        repo.cli(&args)
    };
    let accepted = ticket(&["--accept", "plan", "--host", "claude"]).unwrap();
    assert_eq!(accepted["plan"], "issue-145", "{accepted}");
    let next = accepted["next"].as_str().unwrap();
    assert!(
        next.contains(&format!(
            "craft plan-create --plan issue-145 --reference link:{ISSUE} --goal"
        )),
        "{next}"
    );
    assert!(
        next.contains("ask the user with AskUserQuestion to approve the plan"),
        "{next}"
    );

    let edit = json!({"file_path": "engine/a.rs", "old_string": "a", "new_string": "b"});
    let held = before("claude", &repo, "Edit", edit.clone());
    assert!(held.starts_with("deny "), "{held}");
    assert!(
        held.contains("plan issue-145 is not recorded yet"),
        "{held}"
    );
    assert!(held.contains("--accept direct"), "{held}");
    // Shell commands are not held.
    let shell = before("claude", &repo, "Bash", json!({"command": "git status"}));
    assert!(!shell.starts_with("deny "), "{shell}");
    // The plan's own task list outside the repository is not held; another worktree of the
    // repository is.
    let outside = repo.root.with_extension("inputs");
    let scratch = outside.join("scratch/tasks.json");
    let list = json!({"file_path": scratch, "content": "{}"});
    let written = before("claude", &repo, "Write", list);
    assert!(!written.starts_with("deny "), "{written}");
    let worktree = outside.join("wt");
    repo.git(&[
        "worktree",
        "add",
        "-q",
        "-b",
        "work",
        worktree.to_str().unwrap(),
    ]);
    let there =
        json!({"file_path": worktree.join("engine/a.rs"), "old_string": "a", "new_string": "b"});
    let held_there = before("claude", &repo, "Edit", there);
    assert!(held_there.starts_with("deny "), "{held_there}");

    let missing = ticket(&["--approve"]).unwrap_err();
    assert!(missing.contains("no plan issue-145"), "{missing}");
    let empty = repo.input("empty.json", &json!({"tasks": []}));
    repo.cli(&[
        "plan-create",
        "--plan",
        "issue-145",
        "--goal",
        "Ship",
        "--data",
        &empty,
    ])
    .unwrap();
    assert!(before("claude", &repo, "Edit", edit.clone()).contains("has no tasks yet"));
    let bare = ticket(&["--approve"]).unwrap_err();
    assert!(bare.contains("no tasks to approve"), "{bare}");

    let tasks = repo.input(
        "tasks.json",
        &json!({"tasks": [{"id": "a", "goal": "Change a"}, {"id": "b", "goal": "Test a", "dependsOn": ["a"]}]}),
    );
    repo.cli(&[
        "plan-update",
        "--plan",
        "issue-145",
        "--expected-revision",
        "1",
        "--data",
        &tasks,
    ])
    .unwrap();
    assert!(before("claude", &repo, "Edit", edit.clone()).contains("has not approved plan"));
    let approved = ticket(&["--approve"]).unwrap();
    assert_eq!(approved["approved"]["revision"], 2, "{approved}");
    assert!(
        approved["next"]
            .as_str()
            .unwrap()
            .contains("craft task-start --session S1 --plan issue-145 --ready"),
        "{approved}"
    );
    assert!(!before("claude", &repo, "Edit", edit.clone()).starts_with("deny "));

    // Progress keeps the approval; a changed plan needs it again.
    repo.cli(&[
        "task-start",
        "--session",
        "S1",
        "--task",
        "a",
        "--plan",
        "issue-145",
    ])
    .unwrap();
    assert!(!before("claude", &repo, "Edit", edit.clone()).starts_with("deny "));
    let added = repo.input(
        "added.json",
        &json!({"tasks": [{"id": "c", "goal": "Document a"}]}),
    );
    repo.cli(&[
        "plan-update",
        "--plan",
        "issue-145",
        "--expected-revision",
        "3",
        "--data",
        &added,
    ])
    .unwrap();
    let changed = before("claude", &repo, "Edit", edit.clone());
    assert!(
        changed.contains("changed since the user approved it"),
        "{changed}"
    );

    // Every host whose hooks can refuse a tool refuses its own edit tool the same way.
    for (host, tool, input) in [
        (
            "codex",
            "apply_patch",
            json!({"command": "*** Begin Patch\n*** Update File: engine/a.rs\n@@\n-a\n+b\n*** End Patch"}),
        ),
        (
            "cursor",
            "Write",
            json!({"file_path": "engine/a.rs", "content": "b"}),
        ),
        (
            "copilot",
            "edit",
            json!({"path": "engine/a.rs", "old_str": "a", "new_str": "b"}),
        ),
        (
            "gemini",
            "replace",
            json!({"file_path": "engine/a.rs", "old_string": "a", "new_string": "b"}),
        ),
    ] {
        let output = before(host, &repo, tool, input);
        assert!(output.starts_with("deny "), "{host} {tool}: {output}");
        assert!(
            output.contains("changed since the user approved it"),
            "{host}: {output}"
        );
    }
    ticket(&["--approve"]).unwrap();
    assert!(!before("codex", &repo, "apply_patch", json!({"command": "*** Begin Patch\n*** Update File: engine/a.rs\n@@\n-a\n+b\n*** End Patch"})).starts_with("deny "));

    // Implementing directly holds nothing.
    ticket(&["--accept", "direct"]).unwrap();
    hook(
        "claude",
        "session-start",
        &repo,
        json!({"source": "startup"}),
    );
    assert!(!before("claude", &repo, "Edit", edit).starts_with("deny "));
}

/// The reason a turn-end hook holds the stop with, `None` when it lets the turn end. Claude
/// Code continues the turn with it as context, Cursor with a follow-up message, Codex with a
/// blocking reason.
fn held_stop(host: &str, repo: &Repo) -> Option<String> {
    let output = hook(host, "turn-end", repo, json!({}));
    let reason = match output["decision"].as_str() {
        Some("block") => output["reason"].as_str(),
        _ => output["hookSpecificOutput"]["additionalContext"]
            .as_str()
            .or(output["followup_message"].as_str()),
    };
    reason
        .filter(|reason| reason.contains("Craft stop review"))
        .map(str::to_owned)
}

/// A shell command passes the hold on edit tools: the stop names what it changed in the
/// repository while the plan waits for approval, once, and nothing once the plan is approved,
/// for files elsewhere, or for files that were already changed when the plan was chosen.
#[test]
fn a_stop_names_repository_changes_made_while_the_plan_awaits_approval() {
    let repo = Repo::new(
        "ticket-unapproved",
        &[
            (".craft/project.json", PROJECT),
            ("engine/a.rs", "fn a() {}\n"),
            ("engine/b.rs", "fn b() {}\n"),
        ],
    );
    for host in ["claude", "codex"] {
        hook(host, "session-start", &repo, json!({"source": "startup"}));
    }
    // Already changed before the user chose to plan first: not the plan's change.
    repo.write("engine/b.rs", "fn b() { 1 }\n");
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
    // Cursor tells no session's writes from others': only the baseline keeps engine/b.rs out.
    assert_eq!(held_stop("cursor", &repo), None, "nothing changed since");

    let call =
        json!({"tool_name": "Bash", "tool_input": {"command": "sed -i '' s/a/c/ engine/a.rs"}});
    hook("claude", "before-tool", &repo, call.clone());
    repo.write("engine/a.rs", "fn c() {}\n");
    let outside = repo.root.with_extension("inputs").join("scratch.txt");
    std::fs::create_dir_all(outside.parent().unwrap()).unwrap();
    std::fs::write(&outside, "notes").unwrap();
    hook("claude", "after-tool", &repo, call);
    let reason = held_stop("claude", &repo).expect("the stop is held");
    assert!(
        reason.contains("engine/a.rs changed while edits wait for approval"),
        "{reason}"
    );
    assert!(
        reason.contains("plan issue-145 is not recorded yet"),
        "{reason}"
    );
    assert!(reason.contains("--accept direct"), "{reason}");
    assert!(!reason.contains("engine/b.rs"), "{reason}");
    assert!(!reason.contains("scratch"), "{reason}");
    assert_eq!(held_stop("claude", &repo), None, "said once per change");

    // Codex: a further change is named the same way.
    let call = json!({"tool_name": "shell", "tool_input": {"command": "./write.sh"}});
    hook("codex", "before-tool", &repo, call.clone());
    repo.write("engine/a.rs", "fn d() {}\n");
    hook("codex", "after-tool", &repo, call);
    let codex = held_stop("codex", &repo).expect("codex holds the stop");
    assert!(
        codex.contains("engine/a.rs changed while edits wait for approval"),
        "{codex}"
    );

    // Approved: a change goes unreported by this check.
    approved_plan(&repo);
    let call = json!({"tool_name": "Bash", "tool_input": {"command": "./write.sh"}});
    hook("claude", "before-tool", &repo, call.clone());
    repo.write("engine/a.rs", "fn e() {}\n");
    hook("claude", "after-tool", &repo, call);
    let after = held_stop("claude", &repo).unwrap_or_default();
    assert!(!after.contains("wait for approval"), "{after}");

    // Implementing directly: nothing waits.
    repo.cli(&[
        "ticket",
        "--ticket",
        ISSUE,
        "--accept",
        "direct",
        "--session",
        "S1",
    ])
    .unwrap();
    let call = json!({"tool_name": "Bash", "tool_input": {"command": "./write.sh"}});
    hook("claude", "before-tool", &repo, call.clone());
    repo.write("engine/a.rs", "fn f() {}\n");
    hook("claude", "after-tool", &repo, call);
    let direct = held_stop("claude", &repo).unwrap_or_default();
    assert!(!direct.contains("wait for approval"), "{direct}");
}
