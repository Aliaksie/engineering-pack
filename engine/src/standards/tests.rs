use super::*;

const INTENTS: &[&str] = &["implement", "plan", "review", "story", "verify", "explore"];

fn settings(standards: Value) -> Value {
    json!({
        "modules": [
            {"name": "orders", "path": "services/orders", "layers": ["backend"]},
            {"name": "console", "path": "ui", "layers": ["ui"]}
        ],
        "standards": standards,
    })
}

fn work<'a>(intent: &'a str, files: &'a [String], modules: &'a [Value]) -> Work<'a> {
    Work {
        intent,
        files,
        modules,
        languages: &[],
        layers: &[],
    }
}

fn ids(selected: &[Value]) -> Vec<&str> {
    selected.iter().filter_map(|s| s["id"].as_str()).collect()
}

#[test]
fn declarations_are_validated_before_anything_routes_them() {
    let modules = ["orders", "console"];
    let ok = json!([
        {"id": "commits", "anchor": "conventional-commits"},
        {"id": "api", "file": "docs/api.md", "summary": "REST naming", "scope": {"layers": ["backend"]}},
        {"id": "ui-docs", "folder": "docs/ui", "scope": {"modules": ["console"]}},
        {"id": "lint", "run": {"command": "pnpm", "args": ["lint"], "cwd": "ui"}},
        {"id": "migrations", "checklist": ["Backward compatible"], "scope": {"paths": ["db/"]}},
        {"id": "stories", "anchor": "connextra", "scope": {"paths": ["docs/stories"], "intents": ["story"]}}
    ]);
    assert_eq!(validate(&ok, &modules, INTENTS), Ok(()));
    for (bad, why) in [
        (
            json!([{"id": "x", "anchor": "clean-code"}]),
            "unknown anchor is refused, not guessed",
        ),
        (
            json!([{"id": "x", "file": "a.md", "anchor": "semver"}]),
            "one source",
        ),
        (json!([{"id": "x"}]), "a source or content"),
        (json!([{"id": "x", "file": "../a.md"}]), "project-relative"),
        (
            json!([{"id": "x", "file": "a.md"}, {"id": "x", "file": "b.md"}]),
            "unique ids",
        ),
        (
            json!([{"id": "x", "file": "a.md", "scope": {"modules": ["billing"]}}]),
            "declared modules",
        ),
        (
            json!([{"id": "x", "file": "a.md", "scope": {"intents": ["deploy"]}}]),
            "known intents",
        ),
        (
            json!([{"id": "x", "file": "a.md", "level": "check"}]),
            "check needs something to check",
        ),
        (
            json!([{"id": "x", "file": "a.md", "level": "error"}]),
            "known levels",
        ),
        (
            json!([{"id": "x", "file": "a.md", "scope": {"languages": []}}]),
            "non-empty scope lists",
        ),
        (json!([{"id": "X", "file": "a.md"}]), "kebab ids"),
    ] {
        assert!(validate(&bad, &modules, INTENTS).is_err(), "{why}");
    }
}

#[test]
fn levels_follow_the_form_of_the_declaration() {
    assert_eq!(level(&json!({"anchor": "semver"})), "check");
    assert_eq!(level(&json!({"file": "a.md"})), "advise");
    assert_eq!(level(&json!({"run": {"command": "x"}})), "check");
    assert_eq!(level(&json!({"checklist": ["a"]})), "review");
    assert_eq!(
        level(&json!({"anchor": "semver", "level": "advise"})),
        "advise"
    );
}

#[test]
fn a_route_shows_only_the_standards_its_work_falls_under() {
    let settings = settings(json!([
        {"id": "everywhere", "file": "docs/standards.md"},
        {"id": "ui-only", "file": "docs/ui.md", "scope": {"modules": ["console"]}},
        {"id": "migrations", "checklist": ["Backward compatible"], "scope": {"paths": ["services/orders/db"]}},
        {"id": "stories", "anchor": "connextra", "scope": {"paths": ["docs/stories"]}},
        {"id": "adr", "anchor": "nygard-adr", "scope": {"paths": ["docs/adr"]}}
    ]));
    let console = [json!({"name": "console"})];
    let files = ["ui/app.ts".to_string()];
    let selected = select(&settings, &work("implement", &files, &console));
    assert_eq!(ids(&selected), ["everywhere", "ui-only"]);

    let files = ["services/orders/db/001.sql".to_string()];
    let selected = select(&settings, &work("implement", &files, &[]));
    assert_eq!(ids(&selected), ["everywhere", "migrations"]);
    assert_eq!(selected[1]["checklist"], json!(["Backward compatible"]));
    assert_eq!(selected[1]["level"], "review");
}

#[test]
fn product_and_architecture_work_reach_their_standards_without_files() {
    let settings = settings(json!([
        {"id": "ui-only", "file": "docs/ui.md", "scope": {"modules": ["console"]}},
        {"id": "stories", "anchor": "connextra", "scope": {"paths": ["docs/stories"]}},
        {"id": "adr", "anchor": "nygard-adr", "scope": {"paths": ["docs/adr"]}},
        {"id": "qa", "file": "docs/testing.md", "scope": {"intents": ["verify", "explore"]}}
    ]));
    let story = select(&settings, &work("story", &[], &[]));
    assert_eq!(ids(&story), ["stories"]);
    assert!(
        story[0]["summary"].as_str().is_some_and(|s| !s.is_empty()),
        "an anchor summarizes itself"
    );
    assert_eq!(ids(&select(&settings, &work("plan", &[], &[]))), ["adr"]);
    assert_eq!(ids(&select(&settings, &work("explore", &[], &[]))), ["qa"]);
    assert!(
        select(&settings, &work("implement", &[], &[])).is_empty(),
        "a place-scoped rule is not every request's"
    );
}

#[test]
fn anchor_checks_read_the_files_their_standard_names() {
    let settings = settings(json!([
        {"id": "changelog", "anchor": "keep-a-changelog"},
        {"id": "adr", "anchor": "nygard-adr", "scope": {"paths": ["docs/adr/"]}},
        {"id": "requirements", "anchor": "ears"},
        {"id": "commits", "anchor": "conventional-commits"},
        {"id": "advice-only", "anchor": "gherkin", "level": "advise"}
    ]));
    assert_eq!(
        file_checks(&settings, "CHANGELOG.md"),
        [("changelog", "keep-a-changelog")]
    );
    assert_eq!(
        file_checks(&settings, "packages/x/CHANGELOG.md"),
        [("changelog", "keep-a-changelog")]
    );
    assert_eq!(
        file_checks(&settings, "docs/adr/0001-store.md"),
        [("adr", "nygard-adr")]
    );
    assert!(
        file_checks(&settings, "docs/adr-notes.md").is_empty(),
        "whole path components"
    );
    assert!(
        file_checks(&settings, "docs/requirements.md").is_empty(),
        "no layout is assumed for EARS"
    );
    assert!(
        file_checks(&settings, "features/login.feature").is_empty(),
        "advise is not checked"
    );
    assert_eq!(
        commit_checks(&settings),
        [("commits", "conventional-commits")]
    );

    let off = json!({"routing": {"standardChecks": false}, "standards": settings["standards"]});
    assert!(file_checks(&off, "CHANGELOG.md").is_empty());
    assert!(commit_checks(&off).is_empty());
}

#[test]
fn only_failing_standards_are_reported() {
    let checks = [("commits", "conventional-commits")];
    assert!(verdicts(&checks, "fix(parser): keep quoted keys", None).is_empty());
    let found = verdicts(&checks, "Fixed the parser", None);
    assert_eq!(found.len(), 1);
    assert_eq!(found[0]["standard"], "commits");
    assert!(
        found[0]["findings"]
            .as_array()
            .is_some_and(|f| !f.is_empty())
    );
}

#[test]
fn shell_commands_that_commit_are_recognized() {
    for command in [
        "git commit -m 'fix: x'",
        "git add . && git commit -F msg.txt",
        "cd repo; /usr/bin/git -C sub -c user.name=x commit --amend",
        "git --no-pager commit",
    ] {
        assert!(commits(command), "{command}");
    }
    for command in [
        "git log --grep commit",
        "echo git commit",
        "gitk",
        "git status | grep commit",
    ] {
        assert!(!commits(command), "{command}");
    }
}
