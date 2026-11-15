use super::*;

fn scan(settings: &Value, files: &[(&str, Option<&str>)]) -> Value {
    let sources: Vec<Source> = files
        .iter()
        .map(|(path, text)| Source { path, text: *text })
        .collect();
    propose(settings, &sources, &Practice::default()).expect("proposal")
}

fn ids(result: &Value) -> Vec<&str> {
    result["proposals"]
        .as_array()
        .expect("proposals")
        .iter()
        .map(|proposal| proposal["standard"]["id"].as_str().expect("id"))
        .collect()
}

#[test]
fn every_named_convention_is_an_anchor_craft_checks() {
    for named in &NAMED {
        let anchor = anchors::find(named.anchor).expect(named.anchor);
        assert!(anchor.check.is_some(), "{} has no check", named.anchor);
        assert_eq!(anchor.id, named.anchor, "canonical id");
    }
}

#[test]
fn a_convention_named_in_prose_becomes_a_checked_anchor_with_its_line() {
    let agents = "# Rules\n\nCommits follow Conventional Commits.\nWe use SemVer.\n";
    let result = scan(&json!({}), &[("AGENTS.md", Some(agents))]);
    assert_eq!(ids(&result), ["commits", "versioning"]);
    let first = &result["proposals"][0];
    assert_eq!(
        first["standard"],
        json!({"id": "commits", "anchor": "conventional-commits"})
    );
    assert_eq!(first["level"], "check");
    assert_eq!(first["evidence"], json!([{"file": "AGENTS.md", "line": 3}]));
}

#[test]
fn a_changelog_states_its_format_only_in_its_preamble() {
    let log = "# Changelog\n\nThe format is based on [Keep a Changelog](https://keepachangelog.com),\nand this project adheres to Semantic Versioning.\n\n## [Unreleased]\n\n- Gherkin files are checked.\n\n\n\n\n\n- Conventional Commits and semver notes.\n";
    let result = scan(&json!({}), &[("CHANGELOG.md", Some(log))]);
    assert_eq!(ids(&result), ["changelog", "versioning"]);
    let late = format!("# Changelog\n{}Keep a Changelog\n", "\n".repeat(12));
    assert!(ids(&scan(&json!({}), &[("CHANGELOG.md", Some(&late))])).is_empty());
}

#[test]
fn a_decision_record_convention_is_scoped_to_the_records_folder_or_not_proposed() {
    let text = "Decisions are recorded as ADRs in the Nygard format.";
    assert!(ids(&scan(&json!({}), &[("AGENTS.md", Some(text))])).is_empty());
    let result = scan(&json!({}), &[("AGENTS.md", Some(text)), ("docs/adr", None)]);
    assert_eq!(
        result["proposals"][0]["standard"],
        json!({"id": "nygard-adr", "anchor": "adr-according-to-nygard", "scope": {"paths": ["docs/adr"]}})
    );
}

#[test]
fn a_word_inside_another_word_names_nothing() {
    let text = "The semverish parser and nygardian notes.\n";
    assert!(ids(&scan(&json!({}), &[("CLAUDE.md", Some(text))])).is_empty());
}

#[test]
fn instruction_files_are_never_proposed_as_standards() {
    let result = scan(
        &json!({}),
        &[("AGENTS.md", Some("Be kind.")), ("CLAUDE.md", Some("x"))],
    );
    assert!(ids(&result).is_empty());
}

#[test]
fn commitlint_configuration_names_conventional_commits() {
    let config = "export default {extends: ['@commitlint/config-conventional']};";
    let result = scan(&json!({}), &[("commitlint.config.js", Some(config))]);
    assert_eq!(ids(&result), ["commits"]);
    assert_eq!(
        result["proposals"][0]["evidence"],
        json!([{"file": "commitlint.config.js"}])
    );
}

#[test]
fn a_tool_configuration_becomes_a_run_standard_scoped_to_its_language() {
    let result = scan(
        &json!({}),
        &[
            ("rustfmt.toml", None),
            (
                "Cargo.toml",
                Some("[workspace.lints.clippy]\nall = \"deny\"\n"),
            ),
            ("pyproject.toml", Some("[tool.ruff]\nline-length = 100\n")),
        ],
    );
    assert_eq!(ids(&result), ["rustfmt", "clippy", "ruff"]);
    let rustfmt = &result["proposals"][0];
    assert_eq!(rustfmt["form"], "run");
    assert_eq!(rustfmt["level"], "check");
    assert_eq!(
        rustfmt["standard"]["run"],
        json!({"command": "cargo", "args": ["fmt", "--all", "--", "--check"]})
    );
    assert_eq!(rustfmt["standard"]["scope"], json!({"languages": ["rust"]}));
}

#[test]
fn the_projects_own_script_replaces_the_bare_tool_command() {
    let package = r#"{"scripts":{"lint":"eslint src","format:check":"prettier -c ."}}"#;
    let result = scan(
        &json!({}),
        &[
            ("package.json", Some(package)),
            ("eslint.config.js", None),
            (".prettierrc", None),
            ("pnpm-lock.yaml", None),
        ],
    );
    assert_eq!(ids(&result), ["lint", "format"]);
    assert_eq!(
        result["proposals"][0]["standard"]["run"],
        json!({"command": "pnpm", "args": ["run", "lint"]})
    );
}

#[test]
fn a_tool_without_a_script_keeps_its_own_command() {
    let result = scan(
        &json!({}),
        &[("package.json", Some("{}")), ("biome.json", None)],
    );
    assert_eq!(ids(&result), ["biome"]);
}

#[test]
fn contributor_document_and_editorconfig_become_file_standards() {
    let editor = "root = true\n\n[*]\nindent_style = space\nindent_size = 2\nend_of_line = lf\ninsert_final_newline = true\n\n[*.md]\nindent_size = 4\n";
    let result = scan(
        &json!({}),
        &[
            ("docs/CONTRIBUTING.md", Some("# Contributing\n")),
            (".editorconfig", Some(editor)),
        ],
    );
    assert_eq!(ids(&result), ["contributing", "editorconfig"]);
    assert_eq!(
        result["proposals"][0]["standard"]["file"],
        "docs/CONTRIBUTING.md"
    );
    assert_eq!(result["proposals"][0]["level"], "advise");
    assert_eq!(
        result["proposals"][1]["standard"]["summary"],
        "Formatting follows .editorconfig: 2-space indentation, LF line endings, final newline."
    );
}

#[test]
fn what_the_project_declares_is_listed_not_proposed_again() {
    let settings = json!({"standards": [
        {"id": "semver-rule", "anchor": "semver"},
        {"id": "fmt", "run": {"command": "cargo", "args": ["fmt", "--all", "--", "--check"]}},
        {"id": "commits", "file": "docs/commits.md"}
    ]});
    let result = scan(
        &settings,
        &[
            (
                "AGENTS.md",
                Some("Semantic Versioning and Conventional Commits."),
            ),
            ("rustfmt.toml", None),
        ],
    );
    // The declared id `commits` belongs to another standard, so the proposal takes a new one.
    assert_eq!(ids(&result), ["commits-2"]);
    let declared: Vec<&str> = result["declared"]
        .as_array()
        .expect("declared")
        .iter()
        .map(|entry| entry["standard"].as_str().expect("id"))
        .collect();
    assert_eq!(declared, ["semver-rule", "fmt"]);
}

#[test]
fn a_disabled_anchor_is_not_proposed() {
    let settings = json!({"anchors": {"disables": ["semver"]}});
    let result = scan(&settings, &[("AGENTS.md", Some("We follow semver."))]);
    assert!(ids(&result).is_empty());
}

#[test]
fn accepted_proposals_form_a_valid_configuration() {
    let package = r#"{"scripts":{"typecheck":"tsc --noEmit"}}"#;
    let result = scan(
        &json!({}),
        &[
            (
                "CONTRIBUTING.md",
                Some("Keep a Changelog; MADR records; Gherkin features."),
            ),
            ("docs/decisions", None),
            ("package.json", Some(package)),
            (".golangci.yml", None),
            (".rubocop.yml", None),
            (".flake8", None),
            (".editorconfig", Some("[*]\nindent_style = tab\n")),
        ],
    );
    let standards: Vec<Value> = result["proposals"]
        .as_array()
        .expect("proposals")
        .iter()
        .map(|proposal| proposal["standard"].clone())
        .collect();
    assert_eq!(standards.len(), 9);
    crate::contracts::validate_project(&json!({"formatVersion": 1, "standards": standards}))
        .expect("accepted proposals validate");
}

fn runs(result: &Value) -> Vec<(String, String)> {
    result["proposals"]
        .as_array()
        .expect("proposals")
        .iter()
        .filter(|proposal| proposal["form"] == "run")
        .map(|proposal| {
            let run = &proposal["standard"]["run"];
            let words: Vec<&str> = std::iter::once(run["command"].as_str().expect("command"))
                .chain(
                    run["args"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .filter_map(Value::as_str),
                )
                .collect();
            (
                proposal["standard"]["id"].as_str().expect("id").to_string(),
                words.join(" "),
            )
        })
        .collect()
}

#[test]
fn a_tool_is_proposed_with_the_command_of_the_build_the_project_has() {
    let gradle = scan(
        &json!({}),
        &[
            ("gradlew", None),
            (
                "build.gradle.kts",
                Some(
                    "plugins { id(\"com.diffplug.spotless\"); id(\"io.gitlab.arturbosch.detekt\") }\nktlint {}\n",
                ),
            ),
            ("config/checkstyle/checkstyle.xml", None),
        ],
    );
    let pairs = |list: &[(&str, &str)]| -> Vec<(String, String)> {
        list.iter()
            .map(|(id, command)| (id.to_string(), command.to_string()))
            .collect()
    };
    assert_eq!(
        runs(&gradle),
        pairs(&[
            ("checkstyle", "./gradlew checkstyleMain"),
            ("spotless", "./gradlew spotlessCheck"),
            ("ktlint", "./gradlew ktlintCheck"),
            ("detekt", "./gradlew detekt"),
        ])
    );
    let maven = scan(
        &json!({}),
        &[(
            "pom.xml",
            Some(
                "<artifactId>spotless-maven-plugin</artifactId><artifactId>maven-checkstyle-plugin</artifactId>",
            ),
        )],
    );
    assert_eq!(
        runs(&maven),
        pairs(&[
            ("checkstyle", "mvn -q checkstyle:check"),
            ("spotless", "mvn -q spotless:check"),
        ])
    );
    // A configuration without the build that runs it states no command.
    assert!(runs(&scan(&json!({}), &[("checkstyle.xml", None)])).is_empty());

    let others = scan(
        &json!({}),
        &[
            ("tsconfig.json", None),
            (
                "pyproject.toml",
                Some("[tool.mypy]\nstrict = true\n[tool.pyright]\n"),
            ),
            ("go.mod", None),
            ("Shop.sln", None),
            ("src/Nested.csproj", None),
        ],
    );
    assert_eq!(
        runs(&others),
        pairs(&[
            ("typescript", "npx tsc --noEmit"),
            ("mypy", "mypy ."),
            ("pyright", "pyright"),
            ("go-vet", "go vet ./..."),
            ("dotnet-format", "dotnet format --verify-no-changes"),
        ])
    );
    let solution = &others["proposals"][4];
    assert_eq!(solution["evidence"], json!([{"file": "Shop.sln"}]));
    // A toolchain file is no request for a check: its tool is advised, a configured one
    // checked.
    let levels: Vec<&str> = others["proposals"]
        .as_array()
        .expect("proposals")
        .iter()
        .filter_map(|proposal| proposal["level"].as_str())
        .collect();
    assert_eq!(levels, ["check", "check", "check", "advise", "advise"]);
    // The project's own script for the same job replaces the bare tool.
    let scripted = scan(
        &json!({}),
        &[
            ("tsconfig.json", None),
            (
                "package.json",
                Some(r#"{"scripts":{"typecheck":"tsc -p ."}}"#),
            ),
        ],
    );
    assert_eq!(
        runs(&scripted),
        pairs(&[("typecheck", "npm run typecheck")])
    );
    for result in [&gradle, &maven, &others] {
        let standards: Vec<&Value> = result["proposals"]
            .as_array()
            .expect("proposals")
            .iter()
            .map(|proposal| &proposal["standard"])
            .collect();
        crate::contracts::validate_project(&json!({"formatVersion": 1, "standards": standards}))
            .expect("valid");
    }
    assert!(endings().contains(&".sln"));
}

fn practised(subjects: &[String], files: &[(&str, Option<&str>)]) -> Value {
    let sources: Vec<Source> = files
        .iter()
        .map(|(path, text)| Source { path, text: *text })
        .collect();
    propose(&json!({}), &sources, &Practice { subjects }).expect("proposal")
}

fn subjects(kept: usize, other: usize) -> Vec<String> {
    (0..kept)
        .map(|n| format!("fix(api): correct case {n}"))
        .chain((0..other).map(|n| format!("Update things {n}")))
        .collect()
}

#[test]
fn a_commit_format_most_recent_commits_keep_is_proposed_without_being_named() {
    let result = practised(&subjects(16, 4), &[]);
    assert_eq!(ids(&result), ["commits"]);
    assert_eq!(
        result["proposals"][0]["evidence"],
        json!([{"observed": "16 of the last 20 commit subjects follow it"}])
    );
    // Under four in five, or too little history to judge: nothing.
    assert!(ids(&practised(&subjects(15, 5), &[])).is_empty());
    assert!(ids(&practised(&subjects(19, 0), &[])).is_empty());
    // Where a file names it, the file is the evidence.
    let named = practised(
        &subjects(20, 0),
        &[("AGENTS.md", Some("We use Conventional Commits.\n"))],
    );
    assert_eq!(
        named["proposals"][0]["evidence"],
        json!([{"file": "AGENTS.md", "line": 1}])
    );
}

#[test]
fn a_changelog_or_record_format_is_proposed_only_when_what_exists_passes_its_check() {
    let kept = "# Changelog\n\n## [Unreleased]\n\n## [1.0.0] - 2026-03-01\n\n### Added\n\n- x\n";
    let result = practised(&[], &[("CHANGELOG.md", Some(kept))]);
    assert_eq!(ids(&result), ["changelog"]);
    assert_eq!(
        result["proposals"][0]["evidence"][0]["file"],
        "CHANGELOG.md"
    );
    // Plain version headings are another format: proposing the check would only nag.
    let plain = "# Changelog\n\n## Unreleased\n\n## 1.0.0\n\n- x\n";
    assert!(ids(&practised(&[], &[("CHANGELOG.md", Some(plain))])).is_empty());

    let record = |number: usize| {
        format!(
            "# {number}. Use SQLite\n\n## Context\n\nJobs are lost on restart.\n\n## Decision\n\nWe will use SQLite.\n\n## Status\n\nAccepted\n\n## Consequences\n\nThe volume needs backups.\n"
        )
    };
    let (first, second) = (record(1), record(2));
    let records = practised(
        &[],
        &[
            ("docs/adr", None),
            ("docs/adr/0001-use-sqlite.md", Some(&first)),
            ("docs/adr/0002-use-sqlite.md", Some(&second)),
        ],
    );
    assert_eq!(ids(&records), ["nygard-adr"]);
    assert_eq!(
        records["proposals"][0]["standard"]["scope"],
        json!({"paths": ["docs/adr"]})
    );
    // One record that does not follow it, or a single record: no format to hold to.
    let free = "# Use SQLite\n\nBecause.\n";
    for files in [
        vec![
            ("docs/adr", None),
            ("docs/adr/0001-use-sqlite.md", Some(first.as_str())),
            ("docs/adr/0002-notes.md", Some(free)),
        ],
        vec![
            ("docs/adr", None),
            ("docs/adr/0001-use-sqlite.md", Some(first.as_str())),
        ],
    ] {
        assert!(ids(&practised(&[], &files)).is_empty());
    }
}

#[test]
fn a_workspace_holds_each_member_to_its_own_standards_within_its_folder() {
    let named = Some("We use Conventional Commits.\n");
    let api = scan(
        &json!({}),
        &[
            ("AGENTS.md", named),
            ("rustfmt.toml", None),
            ("CONTRIBUTING.md", Some("# Contributing\n")),
            ("docs/adr", None),
            ("docs/adr/0001-a.md", None),
        ],
    );
    let web = scan(&json!({}), &[("AGENTS.md", named), ("biome.json", None)]);
    let own = scan(
        &json!({}),
        &[(".editorconfig", Some("[*]\nindent_style = tab\n"))],
    );
    let members = [("Shop.API", api.clone()), ("web", web.clone())];
    let result = workspace(&json!({}), own.clone(), &members).expect("workspace");
    assert_eq!(
        ids(&result),
        [
            "editorconfig",
            "commits",
            "shop-api-rustfmt",
            "shop-api-contributing",
            "web-biome"
        ]
    );
    let standard = |id: &str| {
        result["proposals"]
            .as_array()
            .unwrap()
            .iter()
            .find(|proposal| proposal["standard"]["id"] == id)
            .unwrap_or_else(|| panic!("{id}"))
            .clone()
    };
    let rustfmt = standard("shop-api-rustfmt");
    assert_eq!(rustfmt["standard"]["run"]["cwd"], "Shop.API");
    assert_eq!(
        rustfmt["standard"]["scope"],
        json!({"languages": ["rust"], "paths": ["Shop.API"]})
    );
    assert_eq!(
        rustfmt["evidence"],
        json!([{"file": "Shop.API/rustfmt.toml"}])
    );
    assert_eq!(
        standard("shop-api-contributing")["standard"]["file"],
        "Shop.API/CONTRIBUTING.md"
    );
    assert_eq!(result["members"], json!(["Shop.API", "web"]));
    let layer = layer(&result).expect("layer");
    crate::contracts::validate_project(&layer).expect("valid at the workspace root");

    // A commit format one member does not keep is nobody's at the workspace.
    let plain = scan(&json!({}), &[("biome.json", None)]);
    let mixed = workspace(&json!({}), own, &[("api", api), ("web", plain)]).expect("mixed");
    assert!(!ids(&mixed).contains(&"commits"), "{:?}", ids(&mixed));
    // Two folders whose names give the same id: the second is left out and said.
    let twins = workspace(
        &json!({}),
        json!({"proposals": []}),
        &[("my-app", web.clone()), ("my_app", web.clone())],
    )
    .expect("twins");
    assert_eq!(ids(&twins), ["commits", "my-app-biome"]);
    assert_eq!(twins["skipped"][0]["member"], "my_app");
    assert_eq!(twins["skipped"][0]["standard"], "my-app-biome");
    // What the workspace declares itself is not brought in again.
    let declared = json!({"standards": [
        {"id": "format", "run": {"command": "npx", "args": ["biome", "check", "."], "cwd": "web"}}]});
    let covered = workspace(&declared, json!({"proposals": []}), &[("web", web)]).unwrap();
    assert!(!ids(&covered).contains(&"web-biome"), "{:?}", ids(&covered));
}

#[test]
fn what_the_files_state_is_a_layer_that_validates_with_the_files_it_names() {
    let proposed = scan(
        &json!({}),
        &[
            ("rustfmt.toml", None),
            ("CONTRIBUTING.md", Some("# Contributing\n")),
        ],
    );
    let document = layer(&proposed).expect("layer");
    assert_eq!(document["standards"].as_array().map(Vec::len), Some(2));
    assert_eq!(document["standards"][1]["file"], "CONTRIBUTING.md");
    let read = |origin| crate::layers::Layer {
        origin,
        label: origin.into(),
        document: &document,
    };
    crate::layers::validate(&read(crate::layers::DETECTED)).expect("the project's files");
    // The same entry in a layer written for many projects names a file it cannot know.
    assert!(crate::layers::validate(&read("organization")).is_err());
    assert!(layer(&scan(&json!({}), &[])).is_none());
    assert!(declares(
        &json!({"standards": [{"id": "a", "disabled": true}]})
    ));
    assert!(!declares(&json!({"standards": []})));
}

#[test]
fn a_standard_a_layer_declares_wins_over_the_one_read_from_the_files() {
    let proposed = scan(&json!({}), &[("rustfmt.toml", None), (".flake8", None)]);
    let detected = layer(&proposed).expect("layer");
    let organization = json!({"standards": [
        {"id": "rustfmt", "run": {"command": "cargo", "args": ["fmt", "--check"]}}]});
    let layers = [
        crate::layers::Layer {
            origin: crate::layers::DETECTED,
            label: "detected".into(),
            document: &detected,
        },
        crate::layers::Layer {
            origin: "organization",
            label: "organization".into(),
            document: &organization,
        },
    ];
    let resolved = crate::contracts::layered_settings(
        &json!({"formatVersion": 1}),
        &layers,
        &json!({}),
        &crate::contracts::Documents::default(),
        true,
    )
    .expect("settings");
    let standards = resolved["settings"]["standards"]
        .as_array()
        .expect("standards");
    assert_eq!(standards.len(), 2);
    assert_eq!(
        standards[0]["run"]["args"],
        json!(["fmt", "--check"]),
        "the organization's command, not the detected one"
    );
    assert_eq!(resolved["entries"]["standards"]["rustfmt"], "organization");
    assert_eq!(resolved["entries"]["standards"]["flake8"], "detected");
}

#[test]
fn an_undeclared_project_is_offered_its_proposals_once_per_set() {
    let proposed = scan(&json!({}), &[("rustfmt.toml", None), (".flake8", None)]);
    let (offer, record) = offer(&json!({}), &proposed, &Value::Null).expect("offer");
    assert_eq!(
        offer,
        json!({"count": 2, "standards": ["rustfmt", "flake8"]})
    );
    let line = offer_line(&offer);
    for part in ["2 standards", "in effect", "(rustfmt, flake8)", "--propose"] {
        assert!(line.contains(part), "{line}");
    }
    assert!(
        super::offer(&json!({}), &proposed, &record).is_none(),
        "same set"
    );
    let grown = scan(
        &json!({}),
        &[
            ("rustfmt.toml", None),
            (".flake8", None),
            ("biome.json", None),
        ],
    );
    assert!(
        super::offer(&json!({}), &grown, &record).is_some(),
        "a new set"
    );
    let declaring = json!({"standards": [{"id": "x", "file": "docs/x.md"}]});
    assert!(super::offer(&declaring, &proposed, &Value::Null).is_none());
    assert!(super::offer(&json!({}), &scan(&json!({}), &[]), &Value::Null).is_none());
}

#[test]
fn candidates_are_unique_relative_files() {
    let list = candidates();
    for (index, (path, _)) in list.iter().enumerate() {
        crate::contracts::relative_file(&json!(path)).expect("relative");
        assert!(
            !list[..index].iter().any(|(known, _)| known == path),
            "{path}"
        );
    }
}

#[test]
fn no_file_the_scan_reads_commonly_holds_secrets() {
    for (path, _) in candidates() {
        assert!(!crate::safety::secrets::secret_file(path), "{path}");
    }
}
