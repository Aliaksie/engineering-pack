use super::*;

fn facts(remotes: &[(&str, &str)], trackers: Value, commands: &[&str], tools: &[&str]) -> Value {
    let remotes: Vec<Value> = remotes
        .iter()
        .map(|(name, url)| json!({"name": name, "url": url}))
        .collect();
    json!({"remotes": remotes, "trackers": trackers, "commands": commands, "tools": tools})
}

fn settings() -> Value {
    json!({"formatVersion": 1})
}

#[test]
fn remote_urls_give_host_and_repository_without_credentials() {
    for (url, expected) in [
        (
            "https://github.com/acme/shop.git",
            Some(("github.com", "acme/shop")),
        ),
        (
            "https://user:s3cret@github.com/acme/shop",
            Some(("github.com", "acme/shop")),
        ),
        (
            "git@github.com:acme/shop.git",
            Some(("github.com", "acme/shop")),
        ),
        (
            "ssh://git@gitlab.example.com:2222/group/sub/app.git",
            Some(("gitlab.example.com", "group/sub/app")),
        ),
        (
            "https://gitlab.example.com:8443/group/app/",
            Some(("gitlab.example.com:8443", "group/app")),
        ),
        ("/srv/git/shop.git", None),
        ("file:///srv/git/shop.git", None),
        ("C:/repos/shop", None),
        ("https://github.com/acme", None),
        ("https://github.com/acme/../shop", None),
    ] {
        let found = remote(url);
        assert_eq!(
            found.as_ref().map(|(h, p)| (h.as_str(), p.as_str())),
            expected,
            "{url}"
        );
    }
}

#[test]
fn tracker_urls_keep_identity_only() {
    for (url, expected) in [
        (
            "https://github.com/acme/shop/issues/12",
            json!({"provider": "github", "host": "github.com", "repository": "acme/shop"}),
        ),
        (
            "https://github.com/users/acme/projects/3/views/1?pane=issue",
            json!({"provider": "github-project", "owner": "acme", "number": 3}),
        ),
        (
            "https://github.com/orgs/acme/projects/7",
            json!({"provider": "github-project", "owner": "acme", "number": 7}),
        ),
        (
            "https://gitlab.com/group/sub/app/-/issues/9",
            json!({"provider": "gitlab", "host": "gitlab.com", "repository": "group/sub/app"}),
        ),
        (
            "https://git.acme.io/team/app/-/work_items/4",
            json!({"provider": "gitlab", "host": "git.acme.io", "repository": "team/app"}),
        ),
        (
            "https://acme.atlassian.net/browse/SHOP-12",
            json!({"provider": "jira", "site": "acme.atlassian.net"}),
        ),
    ] {
        assert_eq!(tracker(url), Some(expected), "{url}");
    }
    for url in [
        "https://github.com/acme/shop/pull/12",
        "https://gitea.acme.io/acme/shop/issues/5",
        "https://acme.atlassian.net/browse/shop",
        "https://github.com/users/acme/projects/0",
        // The same path shapes elsewhere are not known to be GitHub issues or Jira.
        "https://github.acme.io/acme/shop/issues/5",
        "https://example.com/browse/ABC-123",
    ] {
        assert_eq!(tracker(url), None, "{url}");
    }
}

#[test]
fn trackers_in_text_are_each_named_once() {
    let text = "see (https://github.com/acme/shop/issues/1), and https://github.com/acme/shop/issues/2. Also <https://acme.atlassian.net/browse/SHOP-1>";
    assert_eq!(
        trackers(text),
        vec![
            json!({"provider": "github", "host": "github.com", "repository": "acme/shop"}),
            json!({"provider": "jira", "site": "acme.atlassian.net"}),
        ]
    );
    assert!(trackers("fix #12 please").is_empty());
}

#[test]
fn keep_adds_new_identities_only_and_stays_bounded() {
    let one = json!({"provider": "jira", "site": "a.atlassian.net"});
    let kept = keep(&Value::Null, std::slice::from_ref(&one)).unwrap();
    assert_eq!(kept, json!({"formatVersion": 1, "trackers": [one]}));
    assert_eq!(keep(&kept, &[one]), None);
    let many: Vec<Value> = (0..KEPT + 3)
        .map(|n| json!({"provider": "jira", "site": format!("s{n}.atlassian.net")}))
        .collect();
    let bounded = keep(&kept, &many).unwrap();
    let list = bounded["trackers"].as_array().unwrap();
    assert_eq!(list.len(), KEPT);
    assert_eq!(list.last(), many.last());
}

#[test]
fn github_remote_and_issue_urls_propose_github_for_both_services() {
    let trackers = json!([{"provider": "github", "host": "github.com", "repository": "acme/shop"}]);
    let found = propose(
        &settings(),
        &facts(
            &[("origin", "git@github.com:acme/shop.git")],
            trackers,
            &["gh"],
            &[],
        ),
    );
    assert_eq!(
        found["merge"],
        json!({"services": {
            "codehost": {"setup": "github", "repository": "acme/shop"},
            "tasks": {"setup": "github", "repository": "acme/shop"},
        }})
    );
    assert_eq!(found["services"]["codehost"]["facts"][2], "gh is on PATH");
    assert!(found["services"]["codehost"].get("missing").is_none());
}

#[test]
fn credentials_in_a_remote_never_reach_the_proposal() {
    let found = propose(
        &settings(),
        &facts(
            &[("origin", "https://bot:ghp_token@github.com/acme/shop.git")],
            json!([]),
            &[],
            &[],
        ),
    );
    assert!(!found.to_string().contains("ghp_token"));
    assert_eq!(found["services"]["codehost"]["status"], "proposed");
}

#[test]
fn an_unknown_host_is_learned_from_a_gitlab_issue_url_on_it() {
    let trackers = json!([{"provider": "gitlab", "host": "git.acme.io", "repository": "team/app"}]);
    let found = propose(
        &settings(),
        &facts(
            &[("origin", "https://git.acme.io/team/app.git")],
            trackers,
            &[],
            &[],
        ),
    );
    let codehost = &found["services"]["codehost"];
    assert_eq!(codehost["binding"]["setup"], "gitlab");
    assert!(codehost["missing"].as_str().unwrap().contains("glab"));
    assert_eq!(found["merge"]["services"]["tasks"]["host"], "git.acme.io");
}

#[test]
fn a_github_project_url_proposes_the_project_with_the_remote_repository() {
    let trackers = json!([
        {"provider": "github", "host": "github.com", "repository": "acme/shop"},
        {"provider": "github-project", "owner": "acme", "number": 3},
    ]);
    let found = propose(
        &settings(),
        &facts(
            &[("origin", "https://github.com/acme/shop")],
            trackers,
            &["gh"],
            &[],
        ),
    );
    assert_eq!(
        found["merge"]["services"]["tasks"],
        json!({"setup": "github-project", "repository": "acme/shop", "project": {"owner": "acme", "number": 3}})
    );
}

#[test]
fn jira_over_mcp_when_only_the_host_tool_is_seen() {
    let trackers = json!([{"provider": "jira", "site": "acme.atlassian.net"}]);
    let found = propose(
        &settings(),
        &facts(&[], trackers, &[], &["mcp__atlassian__getJiraIssue"]),
    );
    assert_eq!(
        found["merge"]["services"]["tasks"],
        json!({"setup": "jira", "site": "acme.atlassian.net", "transport": "mcp"})
    );
    assert_eq!(found["services"]["codehost"]["status"], "none");
}

#[test]
fn no_remote_and_no_tracker_propose_nothing() {
    let found = propose(&settings(), &facts(&[], json!([]), &["gh", "glab"], &[]));
    assert!(found.get("merge").is_none());
    assert_eq!(
        found["services"]["codehost"]["reason"],
        "no Git remote is configured"
    );
    assert_eq!(found["services"]["tasks"]["status"], "none");
    assert_eq!(line(&found), None);
}

#[test]
fn mixed_trackers_resolve_to_the_code_host_repository() {
    let trackers = json!([
        {"provider": "github", "host": "github.com", "repository": "acme/shop"},
        {"provider": "github", "host": "github.com", "repository": "other/lib"},
    ]);
    let found = propose(
        &settings(),
        &facts(
            &[("origin", "git@github.com:acme/shop.git")],
            trackers,
            &["gh"],
            &[],
        ),
    );
    assert_eq!(
        found["merge"]["services"]["tasks"]["repository"],
        "acme/shop"
    );
}

#[test]
fn a_configured_binding_is_kept_and_never_replaced() {
    let configured = json!({"formatVersion": 1, "services": {"codehost": {"setup": "github", "repository": "acme/shop"}}});
    let trackers = json!([{"provider": "github-project", "owner": "acme", "number": 3}]);
    let found = propose(
        &configured,
        &facts(
            &[("origin", "git@github.com:other/fork.git")],
            trackers,
            &["gh"],
            &[],
        ),
    );
    assert_eq!(
        found["services"]["codehost"],
        json!({"status": "configured"})
    );
    // The project's issues live in the configured repository, not the remote's.
    assert_eq!(
        found["merge"]["services"]["tasks"]["repository"],
        "acme/shop"
    );
    let project = json!({"formatVersion": 1, "standards": [], "services": {"codehost": {"setup": "gitlab", "repository": "x/y"}}});
    let proposal = json!({"merge": {"services": {"codehost": {"setup": "github", "repository": "a/b"}, "tasks": {"setup": "jira", "site": "a.atlassian.net"}}}});
    let written = apply(&project, &proposal).unwrap();
    assert_eq!(written["services"]["codehost"]["setup"], "gitlab");
    assert_eq!(written["services"]["tasks"]["setup"], "jira");
    let keys: Vec<&String> = written.as_object().unwrap().keys().collect();
    assert_eq!(keys, ["formatVersion", "standards", "services"]);
    assert_eq!(apply(&written, &proposal), None);
}

#[test]
fn every_proposed_binding_is_a_valid_setup() {
    let cases = [
        facts(
            &[("origin", "git@github.com:acme/shop.git")],
            json!([{"provider": "github-project", "owner": "acme", "number": 3}]),
            &[],
            &["mcp__github__issue_read"],
        ),
        facts(
            &[("origin", "https://gitlab.com/g/s/app")],
            json!([{"provider": "gitlab", "host": "gitlab.com", "repository": "g/s/app"}]),
            &["glab"],
            &[],
        ),
        facts(
            &[],
            json!([{"provider": "jira", "site": "acme.atlassian.net"}]),
            &["acli"],
            &[],
        ),
    ];
    for case in cases {
        let found = propose(&settings(), &case);
        for (service, binding) in found["merge"]["services"].as_object().unwrap() {
            assert!(setups::validate(service, binding).is_ok(), "{binding}");
        }
    }
}

#[test]
fn a_proposal_is_offered_once_until_it_changes() {
    let one = propose(
        &settings(),
        &facts(
            &[("origin", "git@github.com:acme/shop.git")],
            json!([]),
            &["gh"],
            &[],
        ),
    );
    let shown = offer(&one, &Value::Null).unwrap();
    assert_eq!(offer(&one, &shown), None);
    assert!(line(&one).unwrap().contains("codehost github acme/shop"));
    let two = propose(
        &settings(),
        &facts(
            &[("origin", "git@github.com:acme/shop.git")],
            json!([{"provider": "github", "host": "github.com", "repository": "acme/shop"}]),
            &["gh"],
            &[],
        ),
    );
    assert!(offer(&two, &shown).is_some());
    assert_eq!(
        offer(
            &propose(&settings(), &facts(&[], json!([]), &[], &[])),
            &Value::Null
        ),
        None
    );
}

#[test]
fn a_host_is_not_taken_for_gitlab_or_github_by_its_name() {
    let found = propose(
        &settings(),
        &facts(
            &[("origin", "git@gitlab.acme.io:team/app.git")],
            json!([]),
            &["glab"],
            &[],
        ),
    );
    assert!(found.get("merge").is_none(), "{found}");
    let reason = found["services"]["codehost"]["reason"].as_str().unwrap();
    assert!(reason.contains("not known as GitHub or GitLab"), "{reason}");
}

#[test]
fn a_gitlab_issue_on_its_own_host_keeps_the_host_for_both_services() {
    let trackers =
        json!([{"provider": "gitlab", "host": "gitlab.acme.io", "repository": "team/app"}]);
    let found = propose(
        &settings(),
        &facts(
            &[("origin", "git@gitlab.acme.io:team/app.git")],
            trackers,
            &["glab"],
            &[],
        ),
    );
    let binding = json!({"setup": "gitlab", "repository": "team/app", "host": "gitlab.acme.io"});
    assert_eq!(found["merge"]["services"]["codehost"], binding);
    assert_eq!(found["merge"]["services"]["tasks"], binding);
}

#[test]
fn an_issue_of_another_repository_is_not_this_projects_tracker() {
    let trackers = json!([{"provider": "github", "host": "github.com", "repository": "other/lib"}]);
    let found = propose(
        &settings(),
        &facts(
            &[("origin", "git@github.com:acme/shop.git")],
            trackers.clone(),
            &["gh"],
            &[],
        ),
    );
    assert!(found["merge"]["services"].get("tasks").is_none(), "{found}");
    let reason = found["services"]["tasks"]["reason"].as_str().unwrap();
    assert!(reason.contains("not this project's repository"), "{reason}");
    // Without a code host nothing shows whose issues they are.
    let found = propose(&settings(), &facts(&[], trackers, &["gh"], &[]));
    assert!(found.get("merge").is_none(), "{found}");
    assert!(
        found["services"]["tasks"]["reason"]
            .as_str()
            .unwrap()
            .contains("no code host"),
        "{found}"
    );
}

#[test]
fn trackers_of_different_kinds_are_not_chosen_between() {
    let trackers = json!([
        {"provider": "github", "host": "github.com", "repository": "acme/shop"},
        {"provider": "jira", "site": "acme.atlassian.net"},
    ]);
    let found = propose(
        &settings(),
        &facts(
            &[("origin", "git@github.com:acme/shop.git")],
            trackers,
            &["gh"],
            &[],
        ),
    );
    assert_eq!(found["services"]["codehost"]["status"], "proposed");
    let reason = found["services"]["tasks"]["reason"].as_str().unwrap();
    assert!(reason.contains("several trackers"), "{reason}");
    assert!(reason.contains("Jira acme.atlassian.net"), "{reason}");
}

#[test]
fn a_github_project_counts_only_when_its_owner_owns_the_repository() {
    let trackers = json!([{"provider": "github-project", "owner": "someone", "number": 9}]);
    let found = propose(
        &settings(),
        &facts(
            &[("origin", "git@github.com:acme/shop.git")],
            trackers.clone(),
            &["gh"],
            &[],
        ),
    );
    assert!(found["merge"]["services"].get("tasks").is_none(), "{found}");
    assert!(
        found["services"]["tasks"]["reason"]
            .as_str()
            .unwrap()
            .contains("does not own acme/shop"),
        "{found}"
    );
    let found = propose(&settings(), &facts(&[], trackers, &["gh"], &[]));
    assert!(found.get("merge").is_none(), "{found}");
}

#[test]
fn a_record_from_a_newer_build_is_left_as_it_is() {
    let newer =
        json!({"formatVersion": 2, "trackers": [{"provider": "jira", "site": "a.atlassian.net"}]});
    let found = [json!({"provider": "jira", "site": "b.atlassian.net"})];
    assert_eq!(keep(&newer, &found), None);
    assert!(kept(&newer).is_empty());
    let proposal = propose(
        &settings(),
        &facts(
            &[("origin", "git@github.com:acme/shop.git")],
            json!([]),
            &[],
            &[],
        ),
    );
    assert_eq!(
        offer(&proposal, &json!({"formatVersion": 2, "digest": "x"})),
        None
    );
}
