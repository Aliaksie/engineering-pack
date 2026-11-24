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
