use crate::contracts::effective_settings;
use crate::services::{plan, prepare};
use serde_json::{Value, json};

fn settings(tasks: Value) -> Result<Value, String> {
    let project = json!({"formatVersion": 1, "services": {"tasks": tasks}});
    Ok(effective_settings(&project, &json!({}), &json!({}))?["settings"].clone())
}

fn run(tasks: Value, operation: &str, params: Value) -> Result<Value, String> {
    plan(&settings(tasks)?, operation, &params)
}

fn project() -> Value {
    json!({"setup": "github-project", "repository": "acme/shop",
           "project": {"owner": "acme", "number": 3}})
}

#[test]
fn github_issues_read_and_search_through_gh() {
    let tasks = json!({"setup": "github", "repository": "acme/shop"});
    let read = run(tasks.clone(), "task.read", json!({"id": "134"})).unwrap();
    assert_eq!(read["command"], "gh");
    assert_eq!(read["provider"], "github");
    assert_eq!(read["shell"], false);
    assert_eq!(
        read["args"],
        json!([
            "issue",
            "view",
            "134",
            "--repo",
            "acme/shop",
            "--json",
            "number,title,body,state,labels,milestone,url"
        ])
    );
    assert_eq!(read["response"]["fields"]["release"], "milestone.title");
    assert!(read["response"].get("list").is_none());

    // A query that looks like an option stays the value of one argument.
    let search = run(tasks, "task.search", json!({"query": "--repo other/x"})).unwrap();
    assert_eq!(search["args"][4], "--search=--repo other/x");
    assert_eq!(search["args"].as_array().unwrap().len(), 9);
    assert_eq!(search["response"]["list"], "[]");
}

#[test]
fn a_github_project_reads_its_own_status_priority_and_release() {
    let read = run(project(), "task.read", json!({"id": 134})).unwrap();
    let args: Vec<&str> = read["args"]
        .as_array()
        .unwrap()
        .iter()
        .map(|a| a.as_str().unwrap())
        .collect();
    assert_eq!(&args[..2], ["api", "graphql"]);
    assert!(args.contains(&"owner=acme") && args.contains(&"repo=shop"));
    assert!(args.contains(&"number=134"));
    let filter = args.last().unwrap();
    assert!(filter.contains(r#".project.number == 3 and .project.owner.login == "acme""#));
    assert!(
        filter
            .contains(r#"status: $f["Status"], priority: $f["Priority"], release: $f["Release"]"#)
    );
    assert_eq!(read["response"]["fields"]["priority"], "priority");

    // Renamed fields are jq string literals; a quote or interpolation stays text.
    let mut renamed = project();
    renamed["project"]["fields"] = json!({"release": r#"Ship "\(x)""#});
    let search = run(renamed, "task.search", json!({"query": "status:Ready"})).unwrap();
    let filter = search["args"]
        .as_array()
        .unwrap()
        .last()
        .unwrap()
        .as_str()
        .unwrap();
    assert!(
        filter.contains(r#"release: .["Ship \"\\(x)\"" | ascii_downcase]"#),
        "{filter}"
    );
    assert_eq!(
        search["args"].as_array().unwrap()[..3],
        json!(["project", "item-list", "3"]).as_array().unwrap()[..]
    );
    assert!(
        search["args"]
            .as_array()
            .unwrap()
            .contains(&json!("--query=status:Ready"))
    );
    // A draft item has no issue to read or link.
    assert!(
        filter.starts_with(r#"[.items[] | select(.content.type != "DraftIssue") |"#),
        "{filter}"
    );
}

#[test]
fn a_task_read_id_that_is_not_an_issue_number_says_what_it_takes() {
    for tasks in [
        project(),
        json!({"setup": "github", "repository": "acme/shop"}),
        json!({"setup": "github", "repository": "acme/shop", "transport": "mcp"}),
        json!({"setup": "gitlab", "repository": "acme/shop"}),
    ] {
        assert_eq!(
            run(tasks.clone(), "task.read", json!({"id": "abc"})).unwrap_err(),
            "task.read takes an issue number as its id: a positive integer such as 145",
            "{tasks}"
        );
    }
}

#[test]
fn github_mcp_names_the_server_tool_and_typed_arguments() {
    let settings = json!({"formatVersion": 1, "services": {
        "codehost": {"setup": "github", "transport": "mcp", "repository": "acme/shop"},
        "tasks": {"setup": "github", "transport": "mcp", "repository": "acme/shop",
                  "server": "gh-remote"}}});
    let settings =
        effective_settings(&settings, &json!({}), &json!({})).unwrap()["settings"].clone();
    let diff = plan(&settings, "change.diff", &json!({"id": "7"})).unwrap();
    assert_eq!(diff["server"], "github");
    assert_eq!(diff["tool"], "pull_request_read");
    assert_eq!(
        diff["arguments"],
        json!({"method": "get_diff", "owner": "acme", "repo": "shop", "pullNumber": 7})
    );
    assert!(diff.get("command").is_none() && diff.get("args").is_none());
    let read = plan(&settings, "task.read", &json!({"id": 12})).unwrap();
    assert_eq!(read["server"], "gh-remote");
    assert_eq!(read["arguments"]["issue_number"], 12);
    assert_eq!(read["response"]["fields"]["url"], "html_url");
}

#[test]
fn gitlab_and_jira_plan_their_own_clis() {
    let gitlab = json!({"setup": "gitlab", "host": "gitlab.example.com",
                        "repository": "group/sub/app"});
    let read = run(gitlab, "task.read", json!({"id": 5})).unwrap();
    assert_eq!(
        read["args"],
        json!([
            "issue",
            "view",
            "5",
            "--repo",
            "https://gitlab.example.com/group/sub/app",
            "--output",
            "json"
        ])
    );
    assert_eq!(read["response"]["fields"]["body"], "description");

    let jira = json!({"setup": "jira"});
    let read = run(jira.clone(), "task.read", json!({"id": "TEAM-123"})).unwrap();
    assert_eq!(read["command"], "acli");
    assert_eq!(
        read["args"].as_array().unwrap()[..4],
        json!(["jira", "workitem", "view", "TEAM-123"])
            .as_array()
            .unwrap()[..]
    );
    assert_eq!(
        read["response"]["fields"]["release"],
        "fields.fixVersions[].name"
    );
    let search = run(
        jira.clone(),
        "task.search",
        json!({"query": "project = TEAM"}),
    )
    .unwrap();
    assert_eq!(search["args"][3], "--jql=project = TEAM");
    for bad in [
        json!("team-1"),
        json!("TEAM-0"),
        json!("-TEAM-1"),
        json!(12),
    ] {
        assert!(run(jira.clone(), "task.read", json!({"id": bad})).is_err());
    }

    let mcp = json!({"setup": "jira", "transport": "mcp", "site": "acme.atlassian.net"});
    let read = run(mcp, "task.read", json!({"id": "TEAM-1"})).unwrap();
    assert_eq!(read["server"], "atlassian");
    assert_eq!(read["tool"], "getJiraIssue");
    assert_eq!(
        read["arguments"],
        json!({"cloudId": "acme.atlassian.net", "issueIdOrKey": "TEAM-1"})
    );
}

#[test]
fn switching_tracker_setups_keeps_the_same_operations() {
    let setups = [
        json!({"setup": "github", "repository": "acme/shop"}),
        project(),
        json!({"setup": "gitlab", "repository": "acme/shop"}),
        json!({"setup": "jira"}),
        json!({"setup": "jira", "transport": "mcp", "site": "acme.atlassian.net"}),
        json!({"setup": "github", "transport": "mcp", "repository": "acme/shop"}),
    ];
    for tasks in setups {
        let summary = prepare(&settings(tasks.clone()).unwrap(), None, None).unwrap();
        let task = summary["services"]
            .as_array()
            .unwrap()
            .iter()
            .find(|service| service["service"] == "tasks")
            .unwrap();
        assert_eq!(task["setup"], tasks["setup"]);
        let reads: Vec<_> = task["operations"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|op| op["effect"] == "read")
            .map(|op| {
                (
                    op["operation"].clone(),
                    op["mapping"].clone(),
                    op["requiredParameters"].clone(),
                )
            })
            .collect();
        assert_eq!(
            reads,
            [
                (json!("task.read"), json!("setup"), json!(["id"])),
                (json!("task.search"), json!("setup"), json!(["query"])),
            ],
            "{tasks}"
        );
        for (operation, params) in [
            ("task.read", json!({"id": "TEAM-1"})),
            ("task.search", json!({"query": "q"})),
        ] {
            let params = if tasks["setup"] == "jira" || operation == "task.search" {
                params
            } else {
                json!({"id": 1})
            };
            let result = run(tasks.clone(), operation, params).unwrap();
            for field in ["id", "title", "status"] {
                assert!(
                    result["response"]["fields"][field].is_string(),
                    "{tasks} {operation} {field}"
                );
            }
        }
    }
}

#[test]
fn setup_bindings_are_checked_before_planning() {
    let error = |tasks: Value| settings(tasks).unwrap_err();
    assert_eq!(
        error(json!({"setup": "trello"})),
        "unknown service setup: trello"
    );
    assert_eq!(
        error(json!({"setup": "github"})),
        "setup github requires repository owner/name"
    );
    assert_eq!(
        error(json!({"setup": "github-project", "repository": "acme/shop"})),
        "setup github-project requires project owner and number"
    );
    let mut bad_number = project();
    bad_number["project"]["number"] = json!(0);
    assert_eq!(
        error(bad_number),
        "project number must be a positive integer"
    );
    let mut bad_field = project();
    bad_field["project"]["fields"] = json!({"area": "Area"});
    assert_eq!(error(bad_field), "unknown project fields field: area");
    assert_eq!(
        error(json!({"setup": "gitlab", "transport": "mcp", "repository": "a/b"})),
        "setup gitlab has no built-in MCP mapping; use an explicit MCP binding"
    );
    assert_eq!(
        error(json!({"setup": "jira", "transport": "mcp"})),
        "setup jira over MCP requires site"
    );
    assert_eq!(
        error(json!({"setup": "jira", "provider": "jira"})),
        "unknown setup binding field: provider"
    );
    assert_eq!(
        error(json!({"setup": "jira", "project": {}})),
        "setup jira takes no project"
    );
    let codehost = json!({"formatVersion": 1, "services": {"codehost": {"setup": "jira"}}});
    assert_eq!(
        effective_settings(&codehost, &json!({}), &json!({})).unwrap_err(),
        "setup jira does not serve codehost"
    );
}

#[test]
fn bindings_without_a_setup_keep_explicit_task_mappings() {
    let tasks = json!({"provider": "github", "transport": "cli", "command": "gh",
                       "repository": "acme/shop"});
    assert_eq!(
        run(tasks, "task.read", json!({"id": 1})).unwrap_err(),
        "binding has no mapping for task.read"
    );
    // An explicit mapping still wins over the setup's built-in one.
    let tasks = json!({"setup": "jira", "operations": {"task.read": {"args": ["show", "{id}"]}}});
    let read = run(tasks, "task.read", json!({"id": "X-1"})).unwrap();
    assert_eq!(read["args"], json!(["show", "X-1"]));
    assert_eq!(read["command"], "acli");
}

#[test]
fn a_setup_supplies_no_writes() {
    let project = json!({"formatVersion": 1,
        "services": {"tasks": {"setup": "jira"}},
        "permissions": {"writeOperations": ["task.comment"]}});
    let settings =
        effective_settings(&project, &json!({}), &json!({})).unwrap()["settings"].clone();
    assert_eq!(
        plan(&settings, "task.comment", &json!({"id": "X-1"})).unwrap_err(),
        "binding has no mapping for task.comment"
    );
}

#[test]
fn host_qualified_mcp_tool_names_count_as_observed() {
    let tasks = json!({"setup": "github", "transport": "mcp", "repository": "acme/shop"});
    let availability = |inventory: Value| {
        let summary = prepare(&settings(tasks.clone()).unwrap(), None, Some(&inventory)).unwrap();
        summary["services"]
            .as_array()
            .unwrap()
            .iter()
            .find(|service| service["service"] == "tasks")
            .unwrap()["operations"]
            .as_array()
            .unwrap()
            .iter()
            .take(2)
            .map(|op| op["availability"].as_str().unwrap().to_string())
            .collect::<Vec<_>>()
    };
    let seen = ["observed-available", "observed-available"];
    assert_eq!(
        availability(json!([
            "mcp__github__issue_read",
            "mcp__github__search_issues"
        ])),
        seen
    );
    assert_eq!(
        availability(json!(["github.issue_read", "search_issues"])),
        seen
    );
    // Another server's tool of the same name, or a longer tool name, is not this one.
    let unseen = ["not-observed", "not-observed"];
    assert_eq!(
        availability(json!([
            "mcp__gitlab__issue_read",
            "mcp__xgithub__search_issues"
        ])),
        unseen
    );
    assert_eq!(availability(json!(["mcp__github__issue_read_all"])), unseen);
}

#[test]
fn a_github_project_refuses_a_host_its_search_cannot_reach() {
    let mut enterprise = project();
    enterprise["host"] = json!("ghe.example.com");
    assert_eq!(
        settings(enterprise).unwrap_err(),
        "setup github-project reads github.com only; gh project commands take no host"
    );
    let read = run(project(), "task.read", json!({"id": 1})).unwrap();
    assert!(
        !read["args"]
            .as_array()
            .unwrap()
            .contains(&json!("--hostname"))
    );
}

fn linked(tasks: Value, reference: &str, response: Value) -> Result<Value, String> {
    crate::services::issue(&settings(tasks)?, reference, &response)
}

#[test]
fn each_setup_reads_an_issue_identity_from_its_own_response() {
    let github = linked(
        json!({"setup": "github", "repository": "acme/shop"}),
        "#135",
        json!({"number": 135, "title": "Link an issue", "state": "OPEN", "body": "…",
               "labels": [], "milestone": null, "url": "https://github.com/acme/shop/issues/135"}),
    )
    .unwrap();
    assert_eq!(
        github,
        json!({"reference": "#135", "service": "github", "id": "135", "title": "Link an issue",
               "status": "OPEN", "url": "https://github.com/acme/shop/issues/135"})
    );
    let mcp = linked(
        json!({"setup": "github", "repository": "acme/shop", "transport": "mcp"}),
        "135",
        json!({"number": 135, "title": "Link", "state": "open",
               "html_url": "https://github.com/acme/shop/issues/135"}),
    )
    .unwrap();
    assert_eq!(mcp["url"], "https://github.com/acme/shop/issues/135");
    let board = linked(
        project(),
        "https://github.com/acme/shop/issues/135",
        json!({"number": 135, "title": "Link", "state": "OPEN", "status": "In Progress",
               "url": "https://github.com/acme/shop/issues/135", "labels": []}),
    )
    .unwrap();
    assert_eq!(
        (board["service"].as_str(), board["status"].as_str()),
        (Some("github-project"), Some("In Progress"))
    );
    let gitlab = linked(
        json!({"setup": "gitlab", "repository": "acme/shop"}),
        "7",
        json!({"iid": 7, "id": 9001, "title": "Fix", "state": "opened",
               "web_url": "https://gitlab.com/acme/shop/-/issues/7"}),
    )
    .unwrap();
    assert_eq!(
        (gitlab["id"].as_str(), gitlab["url"].as_str()),
        (Some("7"), Some("https://gitlab.com/acme/shop/-/issues/7"))
    );
    let jira = linked(
        json!({"setup": "jira", "site": "acme.atlassian.net"}),
        "SHOP-12",
        json!({"key": "SHOP-12", "fields": {"summary": "Checkout\nfails", "status": {"name": "To Do"}}}),
    )
    .unwrap();
    assert_eq!(
        jira,
        json!({"reference": "SHOP-12", "service": "jira", "id": "SHOP-12", "title": "Checkout fails",
               "status": "To Do", "url": "https://acme.atlassian.net/browse/SHOP-12"})
    );
    let siteless = linked(
        json!({"setup": "jira"}),
        "SHOP-12",
        json!({"key": "SHOP-12", "fields": {}}),
    )
    .unwrap();
    assert_eq!(
        (siteless["url"].is_null(), siteless["title"].is_null()),
        (true, true)
    );
}

#[test]
fn an_issue_link_refuses_another_issue_work_and_bindings_without_a_setup() {
    let tasks = json!({"setup": "github", "repository": "acme/shop"});
    let other = linked(
        tasks.clone(),
        "#135",
        json!({"number": 134, "title": "Other"}),
    );
    assert!(other.unwrap_err().contains("issue 134, not 135"));
    assert!(linked(tasks.clone(), "work:cleanup", json!({"number": 1})).is_err());
    assert!(
        linked(tasks.clone(), "#135", json!([{"number": 135}]))
            .unwrap_err()
            .contains("no task id")
    );
    let plain = json!({"provider": "github", "transport": "cli", "command": "gh",
                       "operations": {"task.read": {"args": ["issue", "view", "{id}"]}}});
    assert!(
        linked(plain, "#135", json!({"number": 135}))
            .unwrap_err()
            .contains("names no setup")
    );
    let settings = settings(json!({"setup": "jira"})).unwrap();
    let unconfigured = crate::services::issue(&json!({"services": {}}), "#1", &json!({}));
    assert!(unconfigured.unwrap_err().contains("no tasks service"));
    assert!(crate::services::issue(&settings, "SHOP-12", &json!({"key": "SHOP-13"})).is_err());
}

fn translated(tasks: Value, response: Value) -> Result<Value, String> {
    crate::services::translate(&settings(tasks)?, "task.read", &response)
}

#[test]
fn every_setup_reads_a_task_into_one_craft_shape() {
    let url = "https://github.com/acme/shop/issues/135";
    let expected = |status: &str, priority: Value, url: &str| {
        json!({"id": "135", "title": "Link an issue", "body": "Acceptance:\n- one", "status": status,
               "labels": ["feature"], "priority": priority, "release": "0.0.9", "url": url})
    };
    let github = translated(
        json!({"setup": "github", "repository": "acme/shop"}),
        json!({"number": 135, "title": "Link an issue", "body": "Acceptance:\n- one", "state": "OPEN",
               "labels": [{"name": "feature"}], "milestone": {"title": "0.0.9"}, "url": url}),
    )
    .unwrap();
    assert_eq!(
        (&github["operation"], &github["setup"]),
        (&json!("task.read"), &json!("github"))
    );
    assert_eq!(github["task"], expected("OPEN", Value::Null, url));
    let mcp = translated(
        json!({"setup": "github", "repository": "acme/shop", "transport": "mcp"}),
        json!({"number": 135, "title": "Link an issue", "body": "Acceptance:\n- one", "state": "open",
               "labels": [{"name": "feature"}], "milestone": {"title": "0.0.9"}, "html_url": url}),
    )
    .unwrap();
    assert_eq!(mcp["task"], expected("open", Value::Null, url));
    let board = translated(
        project(),
        json!({"number": 135, "title": "Link an issue", "body": "Acceptance:\n- one", "state": "OPEN",
               "status": "Ready", "priority": "P0", "release": "0.0.9", "labels": ["feature"],
               "url": url}),
    )
    .unwrap();
    assert_eq!(board["task"], expected("Ready", json!("P0"), url));
    let gitlab_url = "https://gitlab.com/acme/shop/-/issues/135";
    let gitlab = translated(
        json!({"setup": "gitlab", "repository": "acme/shop"}),
        json!({"iid": 135, "id": 9001, "title": "Link an issue", "description": "Acceptance:\n- one",
               "state": "opened", "labels": ["feature"], "milestone": {"title": "0.0.9"},
               "web_url": gitlab_url}),
    )
    .unwrap();
    assert_eq!(gitlab["task"], expected("opened", Value::Null, gitlab_url));
    let jira_response = json!({"key": "SHOP-12", "fields": {"summary": "Link an issue",
        "description": "Acceptance:\n- one", "status": {"name": "To Do"},
        "priority": {"name": "High"}, "fixVersions": [{"name": "0.0.9"}], "labels": ["feature"]}});
    for transport in ["cli", "mcp"] {
        let jira = translated(
            json!({"setup": "jira", "site": "acme.atlassian.net", "transport": transport}),
            jira_response.clone(),
        )
        .unwrap();
        let mut task = expected(
            "To Do",
            json!("High"),
            "https://acme.atlassian.net/browse/SHOP-12",
        );
        task["id"] = json!("SHOP-12");
        assert_eq!(jira["task"], task, "{transport}");
    }
}

#[test]
fn a_translated_task_leaves_what_the_response_lacks_null_and_guesses_nothing() {
    let sparse = translated(
        json!({"setup": "github", "repository": "acme/shop"}),
        json!({"number": 7, "title": "Fix", "labels": [], "milestone": null}),
    )
    .unwrap();
    assert_eq!(
        sparse["task"],
        json!({"id": "7", "title": "Fix", "body": null, "status": null, "labels": [],
               "priority": null, "release": null, "url": null})
    );
    let jira = translated(
        json!({"setup": "jira"}),
        json!({"key": "SHOP-12", "fields": {
            "description": {"type": "doc", "version": 1, "content": []},
            "fixVersions": [{"name": "2.0"}, {"name": "2.1"}]}}),
    )
    .unwrap();
    assert_eq!(
        (
            &jira["task"]["body"],
            &jira["task"]["release"],
            &jira["task"]["labels"],
            &jira["task"]["url"]
        ),
        (
            &Value::Null,
            &json!(["2.0", "2.1"]),
            &Value::Null,
            &Value::Null
        )
    );
    let board = translated(
        project(),
        json!({"number": 7, "title": "Fix", "status": null, "url": "http://example.test/7"}),
    )
    .unwrap();
    assert_eq!(
        (&board["task"]["status"], &board["task"]["url"]),
        (&Value::Null, &Value::Null)
    );
}

#[test]
fn translation_needs_a_task_read_setup_and_one_task() {
    let github = json!({"setup": "github", "repository": "acme/shop"});
    let other = crate::services::translate(
        &settings(github.clone()).unwrap(),
        "change.read",
        &json!({}),
    );
    assert!(other.unwrap_err().contains("translates task.read"));
    assert!(
        translated(github, json!([{"number": 1}]))
            .unwrap_err()
            .contains("no task id")
    );
    let explicit = json!({"provider": "github", "transport": "cli", "command": "gh",
        "operations": {"task.read": {"args": ["issue", "view", "{id}"]}}});
    assert!(
        translated(explicit, json!({"number": 1}))
            .unwrap_err()
            .contains("names no setup")
    );
    let unconfigured =
        crate::services::translate(&json!({"services": {}}), "task.read", &json!({}));
    assert!(unconfigured.unwrap_err().contains("no tasks service"));
}
