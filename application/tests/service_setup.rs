//! Service setup discovery end to end: the Git remote and the tracker URLs a project's prompts
//! name become a previewed `services` binding, written only with `--apply`, said once at a
//! session start and reported by `doctor`. A configured binding is never replaced.
mod support;
use serde_json::{Value, json};
use std::fs;
use support::Repo;

fn hook(repo: &Repo, lifecycle: &str, session: &str, extra: Value) -> Value {
    let mut input = json!({
        "handler": "deliver", "host": "claude", "package": "craft-delivery", "lifecycle": lifecycle,
        "scope": "any", "inputStatus": "complete", "tool": "other", "contextSupported": true,
        "cwd": repo.path(), "session": session,
    });
    for (key, value) in extra.as_object().unwrap() {
        input[key] = value.clone();
    }
    craft_application::hook::host::run(&input).unwrap()
}

fn setup(repo: &Repo, extra: &[&str]) -> Value {
    let mut args = vec!["service-setup"];
    args.extend(extra);
    repo.cli(&args).unwrap()
}

fn project(repo: &Repo) -> Option<Value> {
    fs::read_to_string(repo.root.join(".craft/project.json"))
        .ok()
        .map(|text| serde_json::from_str(&text).unwrap())
}

#[test]
fn a_github_remote_and_a_project_url_named_in_a_prompt_propose_the_github_project_setup() {
    let repo = Repo::new("service-setup-github", &[("README.md", "# shop\n")]);
    repo.git(&[
        "remote",
        "add",
        "origin",
        "https://bot:secret-token@github.com/acme/shop.git",
    ]);
    let preview = setup(&repo, &[]);
    assert_eq!(
        preview["merge"]["services"],
        json!({"codehost": {"setup": "github", "repository": "acme/shop"}})
    );
    assert!(!preview.to_string().contains("secret-token"), "{preview}");
    assert_eq!(preview["services"]["tasks"]["status"], "none");

    hook(
        &repo,
        "before-prompt",
        "S-1",
        json!({"prompt": "the board is https://github.com/users/acme/projects/3/views/1 — pick the next item"}),
    );
    let kept = fs::read_to_string(repo.root.join(".craft/state/notices/trackers.json")).unwrap();
    assert!(!kept.contains("pick the next item"), "{kept}");
    let preview = setup(&repo, &[]);
    assert_eq!(
        preview["merge"]["services"]["tasks"],
        json!({"setup": "github-project", "repository": "acme/shop", "project": {"owner": "acme", "number": 3}})
    );
    assert_eq!(project(&repo), None, "a preview writes nothing");

    let applied = setup(&repo, &["--apply"]);
    assert_eq!(applied["written"], json!(["codehost", "tasks"]));
    let written = project(&repo).unwrap();
    assert_eq!(written["services"], preview["merge"]["services"]);
    // The written file loads, and its bindings plan a real read.
    let plan = repo
        .cli(&[
            "operation",
            "--operation",
            "task.read",
            "--params",
            r#"{"id":12}"#,
        ])
        .unwrap();
    assert_eq!(plan["command"], "gh", "{plan}");

    let again = setup(&repo, &["--apply"]);
    assert_eq!(again["written"], json!([]));
    assert_eq!(
        again["services"]["codehost"],
        json!({"status": "configured"})
    );
}

#[test]
fn a_configured_binding_is_kept_while_the_missing_one_is_added() {
    let repo = Repo::new(
        "service-setup-configured",
        &[(
            ".craft/project.json",
            r#"{"formatVersion":1,"services":{"codehost":{"setup":"gitlab","repository":"team/app","host":"git.acme.io"}}}"#,
        )],
    );
    repo.git(&["remote", "add", "origin", "git@github.com:acme/shop.git"]);
    hook(
        &repo,
        "before-prompt",
        "S-1",
        json!({"prompt": "start on https://acme.atlassian.net/browse/SHOP-12"}),
    );
    let applied = setup(&repo, &["--apply"]);
    assert_eq!(applied["written"], json!(["tasks"]));
    let written = project(&repo).unwrap();
    assert_eq!(
        written["services"]["codehost"],
        json!({"setup": "gitlab", "repository": "team/app", "host": "git.acme.io"})
    );
    assert_eq!(written["services"]["tasks"]["setup"], "jira");
    assert_eq!(written["services"]["tasks"]["site"], "acme.atlassian.net");
}

#[test]
fn a_gitlab_remote_proposes_gitlab_with_its_host() {
    let repo = Repo::new("service-setup-gitlab", &[("README.md", "# app\n")]);
    repo.git(&[
        "remote",
        "add",
        "origin",
        "ssh://git@gitlab.acme.io:2222/team/sub/app.git",
    ]);
    // The host's name alone does not say GitLab.
    assert!(setup(&repo, &[]).get("merge").is_none());
    hook(
        &repo,
        "before-prompt",
        "S-1",
        json!({"prompt": "look at https://gitlab.acme.io/team/sub/app/-/issues/4"}),
    );
    let preview = setup(&repo, &[]);
    assert_eq!(
        preview["merge"]["services"]["codehost"],
        json!({"setup": "gitlab", "repository": "team/sub/app", "host": "gitlab.acme.io"})
    );
    assert_eq!(
        preview["merge"]["services"]["tasks"],
        preview["merge"]["services"]["codehost"]
    );
}

#[test]
fn apply_writes_atomically_and_a_newer_tracker_record_is_kept() {
    let repo = Repo::new(
        "service-setup-newer",
        &[(".craft/project.json", r#"{"formatVersion":1}"#)],
    );
    repo.git(&["remote", "add", "origin", "git@github.com:acme/shop.git"]);
    let newer = r#"{"formatVersion":2,"trackers":[]}"#;
    repo.write(".craft/state/notices/trackers.json", newer);
    hook(
        &repo,
        "before-prompt",
        "S-1",
        json!({"prompt": "fix https://github.com/acme/shop/issues/3"}),
    );
    assert_eq!(
        fs::read_to_string(repo.root.join(".craft/state/notices/trackers.json")).unwrap(),
        newer
    );
    let applied = setup(&repo, &["--apply"]);
    assert_eq!(applied["written"], json!(["codehost"]));
    let leftovers: Vec<String> = fs::read_dir(repo.root.join(".craft"))
        .unwrap()
        .flatten()
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .filter(|name| name.ends_with(".tmp"))
        .collect();
    assert!(leftovers.is_empty(), "{leftovers:?}");
}

#[test]
fn a_project_with_no_remote_and_no_tracker_gets_no_proposal() {
    let repo = Repo::new("service-setup-none", &[("README.md", "# local\n")]);
    let applied = setup(&repo, &["--apply"]);
    assert!(applied.get("merge").is_none(), "{applied}");
    assert_eq!(applied["written"], json!([]));
    assert_eq!(project(&repo), None);
    let started = hook(&repo, "session-start", "S-1", json!({}));
    assert!(started.get("serviceSetup").is_none(), "{started}");
}

#[test]
fn a_session_start_says_a_proposal_once_until_it_changes() {
    let repo = Repo::new("service-setup-once", &[("README.md", "# shop\n")]);
    repo.git(&["remote", "add", "origin", "git@github.com:acme/shop.git"]);
    let first = hook(&repo, "session-start", "S-1", json!({}));
    let notice = first["notice"].as_str().unwrap_or_default();
    assert!(notice.contains("codehost github acme/shop"), "{first}");
    assert!(notice.contains("craft service-setup --apply"), "{notice}");
    let second = hook(&repo, "session-start", "S-2", json!({}));
    assert!(second.get("serviceSetup").is_none(), "{second}");

    // A tracker named later changes the proposal: said once more.
    hook(
        &repo,
        "before-prompt",
        "S-2",
        json!({"prompt": "fix https://github.com/acme/shop/issues/7"}),
    );
    let third = hook(&repo, "session-start", "S-3", json!({}));
    assert!(
        third["notice"]
            .as_str()
            .unwrap_or_default()
            .contains("tasks github acme/shop"),
        "{third}"
    );

    // Once both services are bound nothing is said.
    setup(&repo, &["--apply"]);
    fs::remove_file(repo.root.join(".craft/state/notices/service-setup.json")).unwrap();
    let bound = hook(&repo, "session-start", "S-4", json!({}));
    assert!(bound.get("serviceSetup").is_none(), "{bound}");
}

#[test]
fn doctor_reports_the_proposal_with_the_command_that_applies_it() {
    let repo = Repo::new("service-setup-doctor", &[("README.md", "# shop\n")]);
    repo.git(&["remote", "add", "origin", "git@github.com:acme/shop.git"]);
    let homes = support::homes(&repo);
    let (_, report) = support::doctor(&repo, &homes);
    let services: Vec<&Value> = report["configuration"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|check| check["check"] == "services")
        .collect();
    assert_eq!(services.len(), 1, "{report}");
    assert!(
        services[0]["detail"]
            .as_str()
            .unwrap()
            .contains("codehost github acme/shop")
    );
    assert!(
        report["next"].as_array().unwrap().iter().any(|next| next
            .as_str()
            .unwrap()
            .contains("craft service-setup --apply")),
        "{report}"
    );
    assert_eq!(project(&repo), None, "doctor writes nothing");
}
