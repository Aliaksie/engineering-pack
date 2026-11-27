//! A change carries the issue it implements: the ticket intake names the configured read, the
//! agent hands Craft what the tracker returned, and the task and its review show the issue.
mod support;
use serde_json::{Value, json};
use std::fs;
use support::Repo;

const PROJECT: &str = r#"{"formatVersion":1,
  "services":{"tasks":{"setup":"github","repository":"acme/shop"}}}"#;

fn issue(number: u64, title: &str) -> Value {
    json!({"number": number, "title": title, "state": "OPEN", "body": "Acceptance: …",
           "labels": [], "milestone": null,
           "url": format!("https://github.com/acme/shop/issues/{number}")})
}

fn intake(repo: &Repo, prompt: &str) -> String {
    let payload = json!({"session_id": "S1", "cwd": repo.path(), "prompt": prompt,
                         "hook_event_name": "UserPromptSubmit"});
    let args: Vec<String> = [
        "claude",
        "before-prompt",
        "--package",
        "craft-delivery",
        "--handler",
        "deliver",
    ]
    .iter()
    .map(|a| a.to_string())
    .collect();
    let (wire, _) = craft_application::hook::run(&args, &mut payload.to_string().as_bytes());
    assert_eq!(wire.exit_code, 0, "{}", wire.stderr);
    let output: Value = serde_json::from_str(&wire.stdout).unwrap_or_default();
    output["hookSpecificOutput"]["additionalContext"]
        .as_str()
        .unwrap_or_default()
        .to_owned()
}

#[test]
fn the_intake_names_the_configured_read_and_the_ticket_keeps_what_it_returned() {
    let repo = Repo::new("issue-intake", &[(".craft/project.json", PROJECT)]);
    let said = intake(&repo, "go ahead with #145");
    assert!(
        said.contains("read the ticket with `gh issue view 145 --repo acme/shop --json number,title,body,state,labels,milestone,url`"),
        "{said}"
    );
    assert!(said.contains("--issue-data <file>"), "{said}");

    let response = repo.input("issue.json", &issue(145, "Checkout fails"));
    let linked = repo
        .cli(&[
            "ticket",
            "--ticket",
            "#145",
            "--issue-data",
            &response,
            "--session",
            "S1",
        ])
        .unwrap();
    assert_eq!(linked["linked"], true);
    assert_eq!(linked["issue"]["id"], "145");
    assert_eq!(linked["issue"]["title"], "Checkout fails");
    assert_eq!(
        linked["issue"]["url"],
        "https://github.com/acme/shop/issues/145"
    );

    // The decision takes the response too, and refuses another issue's.
    let decided = repo
        .cli(&[
            "ticket",
            "--ticket",
            "#145",
            "--criteria",
            "clear",
            "--issue-data",
            &response,
            "--session",
            "S1",
        ])
        .unwrap();
    assert_eq!(
        (decided["mode"].as_str(), decided["issue"]["id"].as_str()),
        (Some("direct"), Some("145"))
    );
    let other = repo.input("other.json", &issue(146, "Other"));
    let refused = repo.cli(&[
        "ticket",
        "--ticket",
        "#145",
        "--issue-data",
        &other,
        "--session",
        "S1",
    ]);
    assert!(refused.unwrap_err().contains("issue 146, not 145"));
    let unbound = repo.cli(&["ticket", "--ticket", "#145", "--issue-data", &response]);
    assert!(unbound.unwrap_err().contains("--session"));
}

#[test]
fn a_project_without_a_tasks_setup_reads_its_ticket_as_before() {
    let repo = Repo::new(
        "issue-none",
        &[(".craft/project.json", r#"{"formatVersion":1}"#)],
    );
    let said = intake(&repo, "go ahead with #145");
    assert!(said.contains("Craft ticket intake"), "{said}");
    assert!(!said.contains("--issue-data"), "{said}");
    let response = repo.input("issue.json", &issue(145, "Checkout fails"));
    let refused = repo.cli(&[
        "ticket",
        "--ticket",
        "#145",
        "--issue-data",
        &response,
        "--session",
        "S1",
    ]);
    assert!(refused.unwrap_err().contains("no tasks service"));
}

#[test]
fn the_task_and_the_review_show_the_linked_issue() {
    let repo = Repo::new(
        "issue-task",
        &[(".craft/project.json", PROJECT), ("src/a.txt", "a\n")],
    );
    let response = repo.input("issue.json", &issue(145, "Checkout fails"));
    repo.cli(&[
        "ticket",
        "--ticket",
        "#145",
        "--issue-data",
        &response,
        "--session",
        "S1",
    ])
    .unwrap();

    let started = repo
        .cli(&[
            "task-start",
            "--session",
            "S1",
            "--task",
            "fix",
            "--goal",
            "Fix checkout",
        ])
        .unwrap();
    assert_eq!(started["issue"]["id"], "145", "{started}");
    assert_eq!(
        repo.session_state("S1", "fix")["issue"]["title"],
        "Checkout fails"
    );
    let bundle = support::bundle();
    let status = repo
        .cli(&["session", "--session", "S1", "--bundle", &bundle, "--json"])
        .unwrap();
    assert_eq!(
        status["task"]["issue"]["url"], "https://github.com/acme/shop/issues/145",
        "{status}"
    );

    repo.write("src/a.txt", "b\n");
    repo.git(&["commit", "-qam", "fix"]);
    let out = std::path::Path::new(repo.path()).with_extension("snapshot.json");
    let _ = fs::remove_file(&out);
    let prepared = repo
        .cli(&[
            "review-prepare",
            "--base",
            "HEAD~1",
            "--head",
            "HEAD",
            "--output",
            out.to_str().unwrap(),
            "--session",
            "S1",
        ])
        .unwrap();
    assert_eq!(prepared["issue"]["id"], "145", "{prepared}");
    let snapshot: Value = serde_json::from_slice(&fs::read(&out).unwrap()).unwrap();
    assert_eq!(snapshot["issue"]["title"], "Checkout fails");

    // A task named for another issue carries that reference, without what was not read.
    let other = repo
        .cli(&[
            "task-start",
            "--session",
            "S2",
            "--task",
            "other",
            "--goal",
            "Other",
            "--issue",
            "#7",
        ])
        .unwrap();
    assert_eq!(other["issue"], json!({"reference": "#7"}));
    let named = repo.cli(&[
        "task-start",
        "--session",
        "S2",
        "--task",
        "x",
        "--goal",
        "X",
        "--issue",
        "soon",
    ]);
    assert!(named.unwrap_err().contains("URL, number or key"));
}

#[test]
fn work_on_a_finished_issue_is_confirmed_with_the_user_first() {
    let board = json!({"setup": "github-project", "repository": "acme/shop",
                       "project": {"owner": "acme", "number": 3}});
    // Each tracker setup with the response its read gives for one finished and one open issue.
    let cases = [
        (
            json!({"setup": "github", "repository": "acme/shop"}),
            "#7",
            json!({"number": 7, "title": "Fix", "state": "CLOSED", "url": "https://github.com/acme/shop/issues/7"}),
            json!({"number": 7, "title": "Fix", "state": "OPEN", "url": "https://github.com/acme/shop/issues/7"}),
            "CLOSED",
        ),
        (
            board,
            "#7",
            json!({"number": 7, "title": "Fix", "state": "CLOSED", "status": "Done", "labels": [],
                   "url": "https://github.com/acme/shop/issues/7"}),
            json!({"number": 7, "title": "Fix", "state": "OPEN", "status": "In Progress", "labels": [],
                   "url": "https://github.com/acme/shop/issues/7"}),
            "Done",
        ),
        (
            json!({"setup": "gitlab", "repository": "acme/shop"}),
            "7",
            json!({"iid": 7, "title": "Fix", "state": "closed", "web_url": "https://gitlab.com/acme/shop/-/issues/7"}),
            json!({"iid": 7, "title": "Fix", "state": "opened", "web_url": "https://gitlab.com/acme/shop/-/issues/7"}),
            "closed",
        ),
        (
            json!({"setup": "jira", "site": "acme.atlassian.net"}),
            "SHOP-7",
            json!({"key": "SHOP-7", "fields": {"summary": "Fix", "status": {"name": "Resolved"}}}),
            json!({"key": "SHOP-7", "fields": {"summary": "Fix", "status": {"name": "In Progress"}}}),
            "Resolved",
        ),
    ];
    for (tasks, reference, finished, open, status) in cases {
        let project = json!({"formatVersion": 1, "services": {"tasks": tasks}}).to_string();
        let repo = Repo::new("issue-finished", &[(".craft/project.json", &project)]);
        let decide = |response: &Value, data: bool| {
            let file = repo.input("issue.json", response);
            let mut args = vec![
                "ticket",
                "--ticket",
                reference,
                "--criteria",
                "clear",
                "--host",
                "claude",
                "--session",
                "S1",
            ];
            if data {
                args.extend(["--issue-data", &file]);
            }
            repo.cli(&args).unwrap()
        };
        let asked = decide(&finished, true);
        assert_eq!(asked["mode"], "ask", "{status}");
        assert_eq!(
            asked["line"],
            format!("Craft suggests: confirm the ticket is still to be done (issue {status})")
        );
        let next = asked["next"].as_str().unwrap();
        assert!(
            next.contains(
                "Ask the user with AskUserQuestion whether this work is still to be done"
            ),
            "{next}"
        );
        assert!(next.contains("without `--issue-data`"), "{next}");

        // The user goes on: deciding again without the read keeps the issue and decides as usual.
        let again = decide(&finished, false);
        assert_eq!(again["mode"], "direct", "{status}");
        let started = repo
            .cli(&[
                "task-start",
                "--session",
                "S1",
                "--task",
                "fix",
                "--goal",
                "Fix",
            ])
            .unwrap();
        assert_eq!(started["issue"]["status"], status, "{started}");

        // An open issue decides from the ticket's facts.
        assert_eq!(decide(&open, true)["mode"], "direct", "{status}");
    }
}

#[test]
fn operation_translates_a_saved_read_into_the_task_the_ticket_links() {
    let repo = Repo::new("issue-translate", &[(".craft/project.json", PROJECT)]);
    let response = repo.input("issue.json", &issue(145, "Checkout fails"));
    let translated = repo
        .cli(&[
            "operation",
            "--operation",
            "task.read",
            "--response",
            &response,
            "--json",
        ])
        .unwrap();
    assert_eq!(
        translated["task"],
        json!({"id": "145", "title": "Checkout fails", "body": "Acceptance: …", "status": "OPEN",
               "labels": [], "priority": null, "release": null,
               "url": "https://github.com/acme/shop/issues/145"})
    );
    let linked = repo
        .cli(&[
            "ticket",
            "--ticket",
            "#145",
            "--issue-data",
            &response,
            "--session",
            "S1",
        ])
        .unwrap();
    for field in ["id", "title", "status", "url"] {
        assert_eq!(linked["issue"][field], translated["task"][field], "{field}");
    }

    let planned = repo
        .cli(&[
            "operation",
            "--operation",
            "task.read",
            "--params",
            r#"{"id":145}"#,
            "--json",
        ])
        .unwrap();
    assert!(planned.get("task").is_none() && planned["command"] == "gh");
    let both = repo.cli(&[
        "operation",
        "--operation",
        "task.read",
        "--params",
        r#"{"id":145}"#,
        "--response",
        &response,
    ]);
    assert!(format!("{both:?}").contains("takes no --params"));
    let elsewhere = repo.cli(&["ticket", "--ticket", "#145", "--response", &response]);
    assert!(format!("{elsewhere:?}").contains("--response is supported by operation"));
}

const CRITERIA_BODY: &str = "Checkout fails on retry.\n\nAcceptance:\n- A retried payment charges once.\n- [ ] The receipt names\n  the retried order.\n\nFound in production.";

#[test]
fn the_criteria_an_issue_states_become_the_task_contract() {
    let repo = Repo::new(
        "issue-criteria",
        &[(".craft/project.json", PROJECT), ("src/a.txt", "a\n")],
    );
    let mut read = issue(145, "Checkout fails");
    read["body"] = json!(CRITERIA_BODY);
    let response = repo.input("issue.json", &read);
    let linked = repo
        .cli(&[
            "ticket",
            "--ticket",
            "#145",
            "--issue-data",
            &response,
            "--session",
            "S1",
        ])
        .unwrap();
    assert_eq!(
        linked["issue"]["criteria"],
        json!([
            "A retried payment charges once.",
            "The receipt names the retried order."
        ])
    );

    // Another task of the session only shares it: the issue's criteria are not its contract.
    let unrelated = repo
        .cli(&[
            "task-start",
            "--session",
            "S1",
            "--task",
            "docs",
            "--goal",
            "Fix docs",
        ])
        .unwrap();
    assert_eq!(unrelated["issue"]["id"], "145");
    assert!(
        repo.session_state("S1", "docs")["taskRecord"]["engineeringContract"].is_null(),
        "{unrelated}"
    );

    let started = repo
        .cli(&[
            "task-start",
            "--session",
            "S1",
            "--task",
            "fix",
            "--goal",
            "Fix checkout",
            "--issue",
            "#145",
        ])
        .unwrap();
    let criteria: Vec<&str> = started["acceptance"]["criteria"]
        .as_array()
        .unwrap_or_else(|| panic!("{started}"))
        .iter()
        .filter_map(|c| c["id"].as_str())
        .collect();
    assert_eq!(criteria, ["ac-1", "ac-2"], "{started}");
    assert_eq!(started["acceptance"]["complete"], false, "{started}");
    let state = repo.session_state("S1", "fix");
    let contract = &state["taskRecord"]["engineeringContract"];
    assert_eq!(contract["intent"], "Checkout fails");
    assert_eq!(
        contract["context"]["sources"][0]["reference"],
        "https://github.com/acme/shop/issues/145"
    );
    // The contract keeps the criteria; the task's issue keeps its identity.
    assert!(state["issue"].get("criteria").is_none(), "{state}");

    // Required criteria hold completion until evidence covers them.
    let bundle = support::bundle();
    let refused = repo.cli(&[
        "task-step",
        "--session",
        "S1",
        "--stage",
        "verify",
        "--target",
        "checkout",
        "--approach",
        "tests",
        "--evidence",
        "none",
        "--outcome",
        "passed",
        "--result",
        "done",
        "--complete",
        "--bundle",
        &bundle,
    ]);
    let refused = refused.unwrap_err();
    assert!(refused.contains("every required criterion"), "{refused}");

    // Every reader of the task sees the same criteria, read from the session alone.
    assert_eq!(
        started["views"],
        "craft acceptance-view --view qa|product|architecture --session S1"
    );
    for view in ["qa", "product", "architecture"] {
        let shown = repo
            .cli(&["acceptance-view", "--view", view, "--session", "S1"])
            .unwrap();
        let ids: Vec<&str> = shown["criteria"]
            .as_array()
            .unwrap_or_else(|| panic!("{view}: {shown}"))
            .iter()
            .filter_map(|c| c["id"].as_str())
            .collect();
        assert_eq!(ids, ["ac-1", "ac-2"], "{view}");
        assert_eq!(shown["intent"], "Checkout fails", "{view}");
        assert_eq!(shown["binding"]["kind"], "repository", "{view}");
        assert!(shown["binding"]["identity"].is_string(), "{view}: {shown}");
    }
    // A reader judging a deployed build names it.
    let deployed = repo
        .cli(&[
            "acceptance-view",
            "--view",
            "qa",
            "--session",
            "S1",
            "--current-kind",
            "environment",
            "--current-identity",
            "build-7",
        ])
        .unwrap();
    assert_eq!(
        deployed["binding"],
        json!({"kind": "environment", "identity": "build-7"})
    );
    let unnamed = repo.cli(&[
        "acceptance-view",
        "--view",
        "qa",
        "--session",
        "S1",
        "--current-identity",
        "build-7",
    ]);
    assert!(unnamed.unwrap_err().contains("requires --current-kind"));
    let mixed = repo.cli(&[
        "acceptance-view",
        "--view",
        "qa",
        "--session",
        "S1",
        "--contract",
        &response,
    ]);
    assert!(mixed.unwrap_err().contains("not both"));

    // Starting the task again keeps its contract, and so its evidence.
    let again = repo
        .cli(&[
            "task-start",
            "--session",
            "S1",
            "--task",
            "fix",
            "--goal",
            "Fix checkout",
            "--issue",
            "#145",
        ])
        .unwrap();
    assert_eq!(again["taskContract"], started["taskContract"]);

    // The ticket whose delivery the session recorded ties a task to the issue too.
    repo.cli(&[
        "ticket",
        "--ticket",
        "#145",
        "--issue-data",
        &response,
        "--session",
        "S4",
    ])
    .unwrap();
    repo.cli(&[
        "ticket",
        "--ticket",
        "#145",
        "--accept",
        "direct",
        "--session",
        "S4",
    ])
    .unwrap();
    repo.cli(&[
        "task-start",
        "--session",
        "S4",
        "--task",
        "fix",
        "--goal",
        "Fix checkout",
    ])
    .unwrap();
    assert_eq!(
        repo.session_state("S4", "fix")["taskRecord"]["engineeringContract"]["criteria"][0]["id"],
        "ac-1"
    );

    // A contract given to the task replaces the issue's.
    let given: Value = serde_json::from_str(include_str!(
        "../../tests/fixtures/acceptance/contract.json"
    ))
    .unwrap();
    let file = repo.input("contract.json", &given);
    let own = repo
        .cli(&[
            "task-start",
            "--session",
            "S1",
            "--task",
            "own",
            "--goal",
            "Own contract",
            "--contract",
            &file,
        ])
        .unwrap();
    assert_eq!(
        repo.session_state("S1", "own")["taskRecord"]["engineeringContract"],
        given,
        "{own}"
    );

    // A task without a contract has no view to show.
    repo.cli(&[
        "task-start",
        "--session",
        "S3",
        "--task",
        "bare",
        "--goal",
        "Bare",
    ])
    .unwrap();
    let bare = repo.cli(&["acceptance-view", "--view", "product", "--session", "S3"]);
    assert!(bare.unwrap_err().contains("no acceptance contract"));
}

#[test]
fn a_plan_task_with_its_own_acceptance_keeps_the_issue_criteria_out() {
    let repo = Repo::new(
        "issue-plan-criteria",
        &[(".craft/project.json", PROJECT), ("src/a.txt", "a\n")],
    );
    let mut read = issue(145, "Checkout fails");
    read["body"] = json!(CRITERIA_BODY);
    let response = repo.input("issue.json", &read);
    repo.cli(&[
        "ticket",
        "--ticket",
        "#145",
        "--issue-data",
        &response,
        "--session",
        "S1",
    ])
    .unwrap();
    let tasks = repo.input(
        "tasks.json",
        &json!({"tasks": [
            {"id": "part", "goal": "One part", "dependsOn": [], "acceptance": ["The part works"]},
            {"id": "whole", "goal": "The whole", "dependsOn": []}
        ]}),
    );
    repo.cli(&[
        "plan-create",
        "--plan",
        "p",
        "--reference",
        "link:https://github.com/acme/shop/issues/145",
        "--goal",
        "Fix checkout",
        "--data",
        &tasks,
    ])
    .unwrap();
    let contract = |task: &str| {
        repo.cli(&[
            "task-start",
            "--session",
            "S1",
            "--plan",
            "p",
            "--task",
            task,
        ])
        .unwrap();
        repo.session_state("S1", task)["taskRecord"]["engineeringContract"].clone()
    };
    assert!(contract("part").is_null());
    assert_eq!(contract("whole")["criteria"][1]["id"], "ac-2");

    // A plan task in a session that linked nothing carries the ticket its plan links.
    let other = repo
        .cli(&[
            "task-start",
            "--session",
            "S5",
            "--plan",
            "p",
            "--task",
            "part",
            "--take-over",
        ])
        .unwrap();
    assert_eq!(
        other["issue"],
        json!({"reference": "https://github.com/acme/shop/issues/145"})
    );
}

#[test]
fn operation_translates_a_saved_search_and_change_read() {
    let project = r#"{"formatVersion":1,
      "services":{"tasks":{"setup":"github","repository":"acme/shop"},
                  "codehost":{"setup":"github","repository":"acme/shop"}}}"#;
    let repo = Repo::new(
        "search-change-translate",
        &[(".craft/project.json", project)],
    );
    let translate = |operation: &str, response: &str| {
        repo.cli(&[
            "operation",
            "--operation",
            operation,
            "--response",
            response,
            "--json",
        ])
        .unwrap()
    };
    let search = repo.input(
        "search.json",
        &json!([issue(145, "Checkout fails"), issue(146, "Cart empty")]),
    );
    let found = translate("task.search", &search);
    assert_eq!(found["operation"], "task.search");
    assert_eq!(
        (&found["tasks"][0]["id"], &found["tasks"][1]["title"]),
        (&json!("145"), &json!("Cart empty"))
    );
    let pull = repo.input(
        "change.json",
        &json!({"number": 212, "title": "Fix hooks", "body": "", "state": "OPEN",
                "url": "https://github.com/acme/shop/pull/212",
                "baseRefName": "main", "baseRefOid": "e1b3598",
                "headRefName": "fix/hooks", "headRefOid": "9ac0f11"}),
    );
    assert_eq!(
        translate("change.read", &pull)["change"],
        json!({"id": "212", "title": "Fix hooks", "status": "OPEN",
               "base": {"ref": "main", "sha": "e1b3598"},
               "head": {"ref": "fix/hooks", "sha": "9ac0f11"},
               "url": "https://github.com/acme/shop/pull/212"})
    );
}
