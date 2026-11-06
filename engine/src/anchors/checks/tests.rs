use super::*;

fn rules(name: &str, text: &str, path: Option<&str>) -> Vec<&'static str> {
    check(name, text, path)
        .into_iter()
        .map(|finding| finding.rule)
        .collect()
}

fn passes(name: &str, text: &str, path: Option<&str>) {
    let findings = check(name, text, path);
    assert!(findings.is_empty(), "{name} on {text:?}: {findings:?}");
}

fn lines(name: &str, text: &str, path: Option<&str>) -> Vec<Option<usize>> {
    check(name, text, path)
        .into_iter()
        .map(|finding| finding.line)
        .collect()
}

// Verbatim outputs from the R1 step-2 probes (Claude and Codex, anchor arm).
const R1_ADR: &str = r#"# 1. Persist the webhook replay job queue in embedded SQLite

Date: 2026-11-05

## Status

Accepted

## Context

relay receives webhooks and replays them to internal consumers. Each customer gets
one VM, and the service runs there as a single container.

Replay jobs are kept in memory right now, so any restart loses every pending job,
whether it is a deploy, a crash, or a VM reboot. We need jobs to survive restarts.

Adding Redis is the usual way to get a durable queue. The team does not want to run
another service. With one VM per customer, Redis would mean one more process on every
customer VM to deploy, configure, monitor and upgrade.

## Decision

We will persist the webhook replay job queue in an embedded SQLite database stored on
the service's local volume. The database will run in WAL (write-ahead logging) mode.

We will not add Redis or any other external queue service.

## Consequences

- Pending replay jobs survive container and VM restarts.
- No new service to operate. SQLite runs inside the relay process, so deployment stays
  a single container per customer VM.
- The queue is local to one instance. If we ever run several relay instances for the
  same customer, they cannot share this queue, and we will need to revisit this
  decision.
- The local volume now holds state we cannot afford to lose, so it needs backups. We
  must set up backups and restores for it, and they have to work with WAL mode. For
  example, use SQLite's online backup API or `VACUUM INTO` instead of copying the
  database file while it is being written.
"#;

const R1_RELEASE_CHANGELOG: &str = r#"# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [2.0.0] - 2026-11-05

### Added

- Support for the `d` (day) unit, e.g. `toMilliseconds("2d")` returns `172800000`.

### Changed

- **BREAKING:** Renamed `parseDuration` to `toMilliseconds`. Arguments and
  return value are unchanged; replace calls to `parseDuration(text)` with
  `toMilliseconds(text)`.

### Removed

- **BREAKING:** `parseDuration` is no longer exported.

[2.0.0]: https://example.com/timeparse/compare/v1.4.2...v2.0.0
"#;

const R1_RELEASE_COMMIT: &str = r#"feat!: rename parseDuration to toMilliseconds and add day unit

Rename the exported parseDuration function to toMilliseconds, which
states what it returns.

Add support for the "d" (day) unit, so strings like "2d" now parse
to 172800000 milliseconds.

BREAKING CHANGE: parseDuration has been removed from the public API.
Use toMilliseconds instead. Arguments and return value are unchanged.
"#;

const R1_PACKAGE: &str = r#"{
  "name": "timeparse",
  "version": "2.0.0",
  "type": "module",
  "exports": "./src/duration.js"
}
"#;

const R1_FIX_CHANGELOG: &str = r#"# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).

## [Unreleased]

### Fixed

- Corrected duration parsing to convert seconds to milliseconds using a factor of 1000 instead of 100.
- Allowed leading and trailing whitespace in duration input.
"#;

const R1_FIX_COMMIT: &str = r#"fix(duration): correct seconds conversion and trim input

Convert seconds to milliseconds using a factor of 1000 and accept
surrounding whitespace when parsing durations.
"#;

#[test]
fn an_unknown_name_is_a_finding_not_a_panic() {
    assert_eq!(rules("clean-code", "anything", None), ["unknown-anchor"]);
}

#[test]
fn r1_outputs_pass_their_anchors() {
    passes("conventional-commits", R1_RELEASE_COMMIT, None);
    passes("conventional-commits", R1_FIX_COMMIT, None);
    passes(
        "keep-a-changelog",
        R1_RELEASE_CHANGELOG,
        Some("CHANGELOG.md"),
    );
    passes("keep-a-changelog", R1_FIX_CHANGELOG, Some("CHANGELOG.md"));
    passes("semver", R1_RELEASE_CHANGELOG, Some("CHANGELOG.md"));
    passes("semver", R1_PACKAGE, Some("package.json"));
    // Status before Context: section order is not part of the check.
    passes(
        "nygard-adr",
        R1_ADR,
        Some("docs/adr/0001-job-queue-storage.md"),
    );
}

#[test]
fn commit_headers_follow_the_grammar() {
    for header in [
        "fix: handle empty input",
        "feat(api)!: drop the v1 endpoints",
        "chore(deps-dev): bump the toolchain",
        "docs!: rewrite the install guide",
    ] {
        passes("conventional-commits", header, None);
    }
    for header in [
        "Fix: handle empty input",
        "fix:handle empty input",
        "fix:  handle empty input",
        "fix: ",
        "fix(): handle empty input",
        "fix(api: handle empty input",
        "fix handle empty input",
        "[fix] handle empty input",
        "",
    ] {
        assert_eq!(
            rules("conventional-commits", header, None),
            ["header"],
            "{header:?}"
        );
    }
}

#[test]
fn a_commit_body_starts_after_one_blank_line() {
    let text = "fix: handle empty input\nThe parser returned NaN.";
    assert_eq!(rules("conventional-commits", text, None), ["blank-line"]);
    assert_eq!(lines("conventional-commits", text, None), [Some(2)]);
}

#[test]
fn git_comments_and_the_scissors_diff_are_not_the_message() {
    let text = "\nfix: handle empty input\n\n# Please enter the commit message for your changes.\n# ------------------------ >8 ------------------------\ndiff --git a/x b/x\n+Acked by: nobody\n";
    passes("conventional-commits", text, None);
}

#[test]
fn footer_tokens_use_hyphens_except_breaking_change() {
    passes(
        "conventional-commits",
        "feat!: drop v1\n\nBREAKING-CHANGE: v1 is gone\nRefs #12\nReviewed-by: Ann",
        None,
    );
    let text = "fix: x\n\nbody\n\nReviewed-by: Ann\nAcked by: Bob";
    assert_eq!(rules("conventional-commits", text, None), ["footer-token"]);
    assert_eq!(lines("conventional-commits", text, None), [Some(6)]);
    assert_eq!(
        rules(
            "conventional-commits",
            "fix: x\n\nBreaking change: y is gone",
            None
        ),
        ["footer-token"]
    );
    // Body prose with a colon is not a footer.
    passes(
        "conventional-commits",
        "fix: x\n\nIn short: the cache was cold.",
        None,
    );
    passes(
        "conventional-commits",
        "fix: x\n\nThe cause was simple: a stale cache.\nNothing else changed.",
        None,
    );
}

#[test]
fn changelog_headings_are_releases_and_change_groups() {
    passes(
        "keep-a-changelog",
        "# Changelog\n\n## [Unreleased]\n\n## [1.0.10] - 2026-03-01\n\n### Security\n\n- x\n\n## [1.0.9] - 2026-02-01 [YANKED]\n\n## [1.0.0] - 2026-01-01\n\n## [1.0.0-rc.1] - 2025-12-01\n\n```\n## Not a heading\n```\n",
        None,
    );
    for (text, rule) in [
        ("## 1.0.0 - 2026-01-01", "heading"),
        ("## Unreleased", "heading"),
        ("## [1.0.0] - 05/01/2026", "heading"),
        ("## [1.0.0] - 2026-13-01", "heading"),
        ("## [1.0.0]", "heading"),
        ("## [latest] - 2026-01-01", "heading"),
        ("## [1.0.0] - 2026-01-01\n### Improvements", "heading"),
        ("## [1.0.0] - 2026-01-01\n## [1.1.0] - 2026-02-01", "order"),
        ("## [1.0.0] - 2026-01-01\n## [1.0.0] - 2026-01-01", "order"),
        ("## [1.0.0] - 2026-01-01\n## [Unreleased]", "order"),
        ("## [Unreleased]\n## [Unreleased]", "order"),
    ] {
        assert_eq!(rules("keep-a-changelog", text, None), [rule], "{text:?}");
    }
    assert_eq!(
        lines(
            "keep-a-changelog",
            "## [1.0.0] - 2026-01-01\n## [1.1.0] - 2026-02-01",
            None
        ),
        [Some(2)]
    );
}

#[test]
fn versions_follow_the_semver_grammar() {
    for version in [
        "0.0.4",
        "1.0.0-alpha",
        "1.0.0-alpha.1",
        "1.0.0-0.3.7",
        "1.0.0-x.7.z.92",
        "1.0.0-x-y-z.--",
        "1.0.0+20130313144700",
        "1.0.0-beta+exp.sha.5114f85",
        "1.0.0+21AF26D3----117B344092BD",
        "99999999999999999999999.0.0",
    ] {
        passes("semver", version, None);
    }
    for version in [
        "1",
        "1.2",
        "1.2.3.4",
        "01.2.3",
        "1.02.3",
        "1.2.3-01",
        "1.2.3-",
        "1.2.3+",
        "1.2.3-alpha..1",
        "1.2.3+build..1",
        "1.2.3-al_pha",
        "v1.2.3",
        "-1.2.3",
    ] {
        assert_eq!(rules("semver", version, None), ["version"], "{version:?}");
    }
    assert_eq!(lines("semver", "1.2.3\n\n v1.2.3 \n", None), [Some(3)]);
}

#[test]
fn semver_precedence_matches_the_specification() {
    let ordered = [
        "1.0.0-alpha",
        "1.0.0-alpha.1",
        "1.0.0-alpha.beta",
        "1.0.0-beta",
        "1.0.0-beta.2",
        "1.0.0-beta.11",
        "1.0.0-rc.1",
        "1.0.0",
        "1.0.1",
        "1.1.0",
        "2.0.0",
        "10.0.0",
    ];
    for pair in ordered.windows(2) {
        let (a, b) = (
            parse_version(pair[0]).unwrap(),
            parse_version(pair[1]).unwrap(),
        );
        assert_eq!(precedence(&a, &b), Ordering::Less, "{pair:?}");
        assert_eq!(precedence(&b, &a), Ordering::Greater, "{pair:?}");
    }
    let (a, b) = (
        parse_version("1.0.0+a").unwrap(),
        parse_version("1.0.0+b").unwrap(),
    );
    assert_eq!(precedence(&a, &b), Ordering::Equal);
}

#[test]
fn semver_reads_the_version_where_each_manifest_keeps_it() {
    let package = "{\n  \"name\": \"x\",\n  \"version\": \"2.0\"\n}\n";
    assert_eq!(
        rules("semver", package, Some("web/package.json")),
        ["version"]
    );
    assert_eq!(
        lines("semver", package, Some("web/package.json")),
        [Some(3)]
    );
    passes("semver", "{\"private\": true}", Some("package.json"));
    assert_eq!(
        rules("semver", "{\"version\":", Some("package.json")),
        ["parse"]
    );
    assert_eq!(
        rules("semver", "{\"version\": 2}", Some("package.json")),
        ["version"]
    );

    let cargo = "[package]\nname = \"x\"\nversion = \"1.02.0\"\n\n[dependencies]\nserde = { version = \"1\" }\n\n[dependencies.foo]\nversion = \"1\"\n";
    assert_eq!(rules("semver", cargo, Some("Cargo.toml")), ["version"]);
    assert_eq!(lines("semver", cargo, Some("Cargo.toml")), [Some(3)]);
    passes(
        "semver",
        "[package]\nname = \"x\"\nversion.workspace = true\n",
        Some("engine/Cargo.toml"),
    );
    assert_eq!(
        rules(
            "semver",
            "[workspace.package]\nversion = \"0.4\" # pinned\n",
            Some("Cargo.toml")
        ),
        ["version"]
    );

    passes(
        "semver",
        "[project]\nname = 'x'\nversion = '0.1.0'\n",
        Some("pyproject.toml"),
    );
    assert_eq!(
        rules(
            "semver",
            "[project]\nversion = \"0.1\"\n",
            Some("pyproject.toml")
        ),
        ["version"]
    );

    let changelog = "## [Unreleased]\n\n## [1.0] - 2026-01-01\n";
    assert_eq!(lines("semver", changelog, Some("CHANGELOG.md")), [Some(3)]);
    passes("semver", "not a version", Some("src/lib.rs"));
}

fn nygard_adr(title: &str, status: &str) -> String {
    format!(
        "# {title}\n\n## Context\n\nJobs are lost on restart.\n\n## Decision\n\nWe will use SQLite.\n\n## Status\n\n{status}\n\n## Consequences\n\nThe volume needs backups.\n"
    )
}

#[test]
fn a_nygard_adr_has_a_numbered_title_and_four_sections() {
    for (title, status) in [
        ("1. Use SQLite", "Accepted"),
        ("0007 Use SQLite", "proposed"),
        ("ADR-12: Use SQLite", "**Deprecated**"),
        (
            "ADR 3 Use SQLite",
            "Superseded by [ADR-4](0004-use-postgres.md)",
        ),
        ("2. Use SQLite", "- Accepted on 2026-11-05"),
    ] {
        passes("nygard-adr", &nygard_adr(title, status), None);
    }
    assert_eq!(
        rules("nygard-adr", &nygard_adr("Use SQLite", "Accepted"), None),
        ["title"]
    );
    assert_eq!(
        rules("nygard-adr", &nygard_adr("1. Use SQLite", "Draft"), None),
        ["status"]
    );
    assert_eq!(
        lines("nygard-adr", &nygard_adr("1. Use SQLite", "Draft"), None),
        [Some(13)]
    );
    let lowercase = nygard_adr("1. Use SQLite", "Accepted").replace("## Context", "## context:");
    passes("nygard-adr", &lowercase, None);
}

#[test]
fn nygard_sections_must_exist_and_hold_text() {
    let without = "# 1. Use SQLite\n\n## Context\n\nx\n\n## Decision\n\ny\n\n## Status\n\nAccepted\n\n```\n## Consequences\n```\n";
    assert_eq!(rules("nygard-adr", without, None), ["missing-section"]);
    let empty = nygard_adr("1. Use SQLite", "Accepted")
        .replace("We will use SQLite.\n", "<!-- what we decided -->\n");
    assert_eq!(rules("nygard-adr", &empty, None), ["empty-section"]);
    assert_eq!(
        rules("nygard-adr", "Notes only.", None),
        [
            "title",
            "missing-section",
            "missing-section",
            "missing-section",
            "missing-section"
        ]
    );
    // A subsection is part of its section's body.
    let nested = nygard_adr("1. Use SQLite", "Accepted").replace(
        "## Consequences\n\nThe volume needs backups.\n",
        "## Consequences\n\n### Negative\n\nThe volume needs backups.\n",
    );
    passes("nygard-adr", &nested, None);
}

const MADR: &str = r#"# Use SQLite for the replay queue

## Context and Problem Statement

Replay jobs are lost when the service restarts.

## Considered Options

* SQLite
* Redis

## Decision Outcome

Chosen option: "SQLite", because it needs no new service.

### Consequences

* Good, because jobs survive restarts.
"#;

#[test]
fn a_madr_record_has_its_required_sections_and_file_name() {
    passes("madr", MADR, Some("docs/decisions/0001-use-sqlite.md"));
    passes("madr", MADR, None);
    for path in [
        "docs/use-sqlite.md",
        "docs/1-use-sqlite.md",
        "0001_Use_SQLite.md",
        "0001-.md",
    ] {
        assert_eq!(rules("madr", MADR, Some(path)), ["file-name"], "{path}");
    }
    let prose_options = MADR.replace("* SQLite\n* Redis\n", "SQLite or Redis.\n");
    assert_eq!(rules("madr", &prose_options, None), ["options"]);
    let numbered = MADR.replace("* SQLite\n* Redis\n", "1. SQLite\n2. Redis\n");
    passes("madr", &numbered, None);
    let undecided = MADR.replace("## Decision Outcome", "## Notes");
    assert_eq!(rules("madr", &undecided, None), ["missing-section"]);
    let unfilled = MADR.replace(
        "Replay jobs are lost when the service restarts.",
        "<!-- Describe the context and problem statement. -->",
    );
    assert_eq!(rules("madr", &unfilled, None), ["empty-section"]);
    assert_eq!(
        rules("madr", &MADR.replace("# Use", "Use"), None),
        ["title"]
    );
}

const FEATURE: &str = r##"# language: en
@billing
Feature: Refunds
  Customers get their money back.

  Background:
    Given a paid order

  # The common path.
  @smoke
  Scenario: Full refund
    When the customer asks for a refund
    Then the order is refunded
    And the customer receives:
      """
      Feature: this is doc string text, not a keyword
      Scenario: neither is this
      """

  Rule: Partial refunds

    Example: Half refund
      Given the order total is 10
      When the customer returns items:
        | item  | price |
        | shirt | 5     |
        | pipe \| tee | 5 |
      Then 5 is refunded
      * the ledger balances

    Scenario Outline: Refund by method
      Given the customer paid by <method>
      When a refund of <amount> is requested
      Then the refund goes to <method>

      Examples: Cards
        | method | amount |
        | card   | 10     |

      Examples: Cash
        | amount | method |
        | 5      | cash   |
"##;

#[test]
fn a_well_formed_feature_passes() {
    passes("gherkin", FEATURE, Some("features/refunds.feature"));
}

#[test]
fn a_feature_file_has_one_feature_and_scenarios_with_steps() {
    assert_eq!(
        rules("gherkin", "Scenario: x\n  Given y\n", None),
        ["feature"]
    );
    assert_eq!(
        rules(
            "gherkin",
            "Feature: a\nScenario: x\n  Given y\nFeature: b\n",
            None
        ),
        ["feature"]
    );
    assert_eq!(
        lines(
            "gherkin",
            "Feature: a\nScenario: x\n  Given y\nFeature: b\n",
            None
        ),
        [Some(4)]
    );
    assert_eq!(
        rules("gherkin", "Feature: a\n  Background:\n    Given y\n", None),
        ["scenario"]
    );
    assert_eq!(
        rules(
            "gherkin",
            "Feature: a\nScenario: x\n  The user refunds.\nScenario: y\n  Given z\n",
            None
        ),
        ["steps"]
    );
    assert_eq!(
        lines(
            "gherkin",
            "Feature: a\nScenario: x\n  The user refunds.\nScenario: y\n  Given z\n",
            None
        ),
        [Some(2)]
    );
}

#[test]
fn outlines_need_examples_naming_every_placeholder() {
    let outline = "Feature: a\nScenario Outline: x\n  Given <user> pays <amount>\n";
    assert_eq!(rules("gherkin", outline, None), ["examples"]);
    let partial = format!("{outline}  Examples:\n    | user |\n    | ann  |\n");
    assert_eq!(rules("gherkin", &partial, None), ["examples"]);
    assert_eq!(lines("gherkin", &partial, None), [Some(4)]);
    let headless = format!("{outline}  Examples:\n");
    assert_eq!(rules("gherkin", &headless, None), ["examples"]);
    let complete = format!("{outline}  Examples:\n    | user | amount |\n    | ann  | 5      |\n");
    passes("gherkin", &complete, None);
    // Angle brackets in a plain scenario are literal text.
    passes(
        "gherkin",
        "Feature: a\nScenario: x\n  Given 1 < 2 and <literal>\n",
        None,
    );
}

#[test]
fn table_rows_have_equal_cell_counts() {
    let text = "Feature: a\nScenario: x\n  Given rows:\n    | a | b |\n    | 1 |\n    | 1 | 2 |\n  When a table follows:\n    | c |\n";
    assert_eq!(rules("gherkin", text, None), ["table"]);
    assert_eq!(lines("gherkin", text, None), [Some(5)]);
}

#[test]
fn ears_requirements_follow_one_of_the_patterns() {
    passes(
        "ears",
        "# Requirements the system shall meet\n\n- The parser shall reject empty input.\n1. When a job fails, the queue shall retry it.\n**While** the device is offline, the app shall queue writes.\nWhere telemetry is enabled, the service shall export spans.\nIf the token is expired, then the API shall return 401.\nWhere caching is on, while the cache is warm, when a read arrives, the service shall answer from memory.\nREQ-7: The system shall log every login.\n| R1 | The exporter shall write one file per host. |\nThe system should be fast.\n",
        None,
    );
    for text in [
        "When a job fails the queue shall retry it.",
        "If the token is expired, the API shall return 401.",
        "System shall start.",
        "Users shall be notified.",
        "The system shall.",
        "While , the app shall queue writes.",
        "| R2 | Exports shall be daily. |",
    ] {
        assert_eq!(rules("ears", text, None), ["pattern"], "{text:?}");
    }
    assert_eq!(
        lines(
            "ears",
            "Intro.\n\nThe app shall start.\nApp shall stop.\n",
            None
        ),
        [Some(4)]
    );
}

#[test]
fn connextra_stories_name_role_goal_and_benefit() {
    passes(
        "connextra",
        "# Stories\n\n- As a support agent, I want to search tickets by customer, so that I answer faster.\n- **As an** admin, **I want** audit logs so that I can trace changes.\n\nAs a buyer,\nI want saved carts,\nso that I can finish later.\n\nAs a result, the team ships weekly.\n",
        None,
    );
    for text in [
        "As a user I want exports so that I can share reports.",
        "As a user, I want exports.",
        "- As an admin, I need audit logs so that I can trace changes.",
        "As a, I want x so that y.",
    ] {
        assert_eq!(rules("connextra", text, None), ["pattern"], "{text:?}");
    }
    assert_eq!(
        lines(
            "connextra",
            "- As a user, I want x, so that y.\n- As a user, I want z\n",
            None
        ),
        [Some(2)]
    );
}
