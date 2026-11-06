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
            json!([{"id": "x", "anchor": "Arc 42"}]),
            "kebab anchor names",
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
fn anchors_resolve_against_the_project_catalogs_when_settings_load() {
    let with = |standards: Value, loaded: Value| {
        let mut settings = settings(standards);
        settings["anchors"]["loaded"] = loaded;
        validate_anchors(&settings)
    };
    let standard = |anchor: &str| json!([{"id": "x", "anchor": anchor}]);
    let refused = with(standard("clean-code"), json!([])).unwrap_err();
    assert!(
        refused.starts_with("standard x: anchor clean-code is not offered: "),
        "a refused name says why: {refused}"
    );
    assert_eq!(
        with(standard("no-such-anchor"), json!([])).unwrap_err(),
        "standard x: unknown anchor no-such-anchor",
        "an unknown name is refused, not guessed"
    );
    for name in ["arc42", "adr-according-to-nygard", "nygard-adr", "semver"] {
        assert_eq!(with(standard(name), json!([])), Ok(()), "{name}");
    }
    assert!(
        with(
            json!([{"id": "x", "anchor": "arc42", "level": "check"}]),
            json!([])
        )
        .unwrap_err()
        .contains("has no check"),
        "a catalog name has nothing to check"
    );
    let own = json!([{"id": "clean-code", "summary": "Ours", "source": "a.json"}]);
    assert_eq!(
        with(standard("clean-code"), own),
        Ok(()),
        "a project may define a name Craft refuses"
    );
    let mut disabled = settings(json!([]));
    disabled["anchors"]["disables"] = json!(["testing-pyramid", "nope"]);
    assert_eq!(
        validate_anchors(&disabled).unwrap_err(),
        "anchors.disables: unknown anchor nope"
    );
}

#[test]
fn levels_follow_the_form_of_the_declaration() {
    let mut project = settings(json!([]));
    project["anchors"]["loaded"] = json!([
        {"id": "our-adr", "summary": "Owner named", "checklist": ["Owner"], "source": "a.json"}
    ]);
    let level = |standard: Value| level(&project, &standard).to_owned();
    assert_eq!(level(json!({"anchor": "semver"})), "check");
    assert_eq!(level(json!({"anchor": "arc42"})), "advise");
    assert_eq!(level(json!({"anchor": "our-adr"})), "review");
    assert_eq!(level(json!({"file": "a.md"})), "advise");
    assert_eq!(level(json!({"run": {"command": "x"}})), "check");
    assert_eq!(level(json!({"checklist": ["a"]})), "review");
    assert_eq!(
        level(json!({"anchor": "semver", "level": "advise"})),
        "advise"
    );
}

#[test]
fn a_project_entry_replaces_the_built_in_one_with_its_id() {
    let mut project = settings(json!([
        {"id": "changelog", "anchor": "keep-a-changelog"},
        {"id": "versions", "anchor": "semver"},
        {"id": "ddd", "anchor": "domain-driven-design"}
    ]));
    project["anchors"]["loaded"] = json!([
        {"id": "keep-a-changelog", "summary": "Our variant", "check": "keep-a-changelog", "source": "a.json"},
        {"id": "semantic-versioning", "summary": "Calendar versions", "source": "a.json"}
    ]);
    let shown = select(&project, &work("implement", &[], &[]));
    assert_eq!(shown[0]["summary"], "Our variant");
    assert_eq!(
        shown[1]["summary"], "Calendar versions",
        "an alias resolves to the project entry"
    );
    assert_eq!(
        shown[1]["level"], "advise",
        "the whole entry is replaced, check included"
    );
    assert_eq!(
        shown[2]["summary"],
        "Domain-Driven Design according to Evans"
    );
    let checks = file_checks(&project, "CHANGELOG.md");
    assert_eq!(
        checks
            .iter()
            .map(|c| (c.standard, c.check.as_str()))
            .collect::<Vec<_>>(),
        [("changelog", "keep-a-changelog")],
        "a reused check brings its files; the replaced semver no longer checks"
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
fn craft_defaults_are_opt_in_advised_and_yield_to_the_project() {
    let mut project = settings(json!([]));
    assert!(
        select(&project, &work("story", &[], &[])).is_empty(),
        "no defaults unless the project turns them on"
    );
    project["anchors"]["defaults"] = json!(true);
    let story = select(&project, &work("story", &[], &[]));
    assert_eq!(ids(&story), ["invest", "moscow"]);
    assert!(
        story
            .iter()
            .all(|s| s["level"] == "advise" && s["default"] == true)
    );
    assert!(
        story
            .iter()
            .all(|s| s["summary"].as_str().is_some_and(|s| !s.is_empty()))
    );
    assert_eq!(
        ids(&select(&project, &work("plan", &[], &[]))),
        ["adr-according-to-nygard"]
    );
    assert_eq!(
        ids(&select(&project, &work("review", &[], &[]))),
        ["testing-pyramid", "kiss-principle"]
    );
    assert!(select(&project, &work("explore", &[], &[])).is_empty());

    project["anchors"]["disables"] = json!(["moscow", "nygard-adr"]);
    assert_eq!(ids(&select(&project, &work("story", &[], &[]))), ["invest"]);
    assert!(
        select(&project, &work("plan", &[], &[])).is_empty(),
        "an alias disables its canonical id"
    );

    project["standards"] =
        json!([{"id": "decisions", "anchor": "madr", "scope": {"paths": ["docs/decisions"]}}]);
    project["anchors"]["disables"] = json!([]);
    assert_eq!(
        ids(&select(&project, &work("plan", &[], &[]))),
        ["decisions"],
        "a declared standard for the kind of work replaces the defaults: no Nygard in a MADR repo"
    );
    project["standards"] =
        json!([{"id": "pyramid", "anchor": "testing-pyramid", "scope": {"intents": ["verify"]}}]);
    assert_eq!(
        ids(&select(&project, &work("review", &[], &[]))),
        ["kiss-principle"],
        "an anchor the project already names is not added again"
    );
    project["standards"] = json!([]);
    project["anchors"]["loaded"] =
        json!([{"id": "invest", "summary": "Our story rules", "source": "a.json"}]);
    assert_eq!(
        select(&project, &work("story", &[], &[]))[0]["summary"],
        "Our story rules"
    );

    project["anchors"]["loaded"] = json!([]);
    project["routing"]["standardChecks"] = json!(true);
    assert!(
        commit_checks(&project).is_empty()
            && file_checks(&project, "docs/adr/0001-x.md").is_empty(),
        "a default is never checked"
    );
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
    let pairs = |checks: Vec<Check>| {
        checks
            .into_iter()
            .map(|check| (check.standard.to_owned(), check.check))
            .collect::<Vec<_>>()
    };
    let pair = |standard: &str, check: &str| vec![(standard.to_owned(), check.to_owned())];
    assert_eq!(
        pairs(file_checks(&settings, "CHANGELOG.md")),
        pair("changelog", "keep-a-changelog")
    );
    assert_eq!(
        pairs(file_checks(&settings, "packages/x/CHANGELOG.md")),
        pair("changelog", "keep-a-changelog")
    );
    assert_eq!(
        pairs(file_checks(&settings, "docs/adr/0001-store.md")),
        pair("adr", "nygard-adr")
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
        pairs(commit_checks(&settings)),
        pair("commits", "conventional-commits")
    );

    let off = json!({"routing": {"standardChecks": false}, "standards": settings["standards"]});
    assert!(file_checks(&off, "CHANGELOG.md").is_empty());
    assert!(commit_checks(&off).is_empty());
}

#[test]
fn only_failing_standards_are_reported() {
    let checks = [Check {
        standard: "commits",
        anchor: "conventional-commits",
        check: "conventional-commits".into(),
    }];
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
