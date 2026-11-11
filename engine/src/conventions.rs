//! Conventions a project already states, as ready `standards` declarations. The scan reads
//! fixed root files and recognizes three forms: a convention named in prose that Craft can
//! check, a lint or format tool with its configuration, and a contributor document. Nothing
//! is inferred from code and no command written in prose is parsed; a proposal is inactive
//! until the consumer copies it into the project configuration.
use crate::anchors::{self, Tier};
use crate::standards;
use serde_json::{Value, json};

/// The most one scanned file may hold; larger files are reported, not read.
pub const MAX_BYTES: usize = 256 * 1024;
/// How many standards a project declares at most; proposals fill only what is left.
const MAX_STANDARDS: usize = 64;

/// Agent instruction files: the host loads them itself, so they are read for named
/// conventions and never proposed as a standard.
const INSTRUCTIONS: [&str; 5] = [
    "AGENTS.md",
    "CLAUDE.md",
    "GEMINI.md",
    ".github/copilot-instructions.md",
    ".junie/guidelines.md",
];
/// Contributor documents, most specific location last; the first present one is proposed.
const CONTRIBUTING: [&str; 3] = [
    "CONTRIBUTING.md",
    ".github/CONTRIBUTING.md",
    "docs/CONTRIBUTING.md",
];
const CHANGELOG: &str = "CHANGELOG.md";
/// The conventions a changelog's preamble can state, and how many lines the preamble spans.
const CHANGELOG_NAMES: [&str; 2] = ["keep-a-changelog", "semantic-versioning"];
const CHANGELOG_PREAMBLE: usize = 12;
const EDITORCONFIG: &str = ".editorconfig";
const PACKAGE: &str = "package.json";
const COMMITLINT: [&str; 9] = [
    "commitlint.config.js",
    "commitlint.config.cjs",
    "commitlint.config.mjs",
    "commitlint.config.ts",
    ".commitlintrc",
    ".commitlintrc.json",
    ".commitlintrc.yml",
    ".commitlintrc.yaml",
    ".commitlintrc.js",
];
/// Where decision records live; a decision-record convention is checked on the first present.
const DECISIONS: [&str; 5] = [
    "docs/adr",
    "docs/decisions",
    "docs/architecture/decisions",
    "doc/adr",
    "adr",
];
/// Lock files name the package manager that runs a declared script; only presence is read.
const MANAGERS: [(&str, &str); 4] = [
    ("pnpm-lock.yaml", "pnpm"),
    ("yarn.lock", "yarn"),
    ("bun.lock", "bun"),
    ("bun.lockb", "bun"),
];

/// A convention by name: the anchor it stands for and how prose refers to it. Phrases match
/// anywhere, words only as whole words; both ignore case.
struct Named {
    anchor: &'static str,
    id: &'static str,
    phrases: &'static [&'static str],
    words: &'static [&'static str],
    /// The check reads decision records, so the standard is scoped to their folder.
    records: bool,
}

const NAMED: [Named; 6] = [
    Named {
        anchor: "conventional-commits",
        id: "commits",
        phrases: &["conventional commits", "conventionalcommits.org"],
        words: &[],
        records: false,
    },
    Named {
        anchor: "keep-a-changelog",
        id: "changelog",
        phrases: &["keep a changelog", "keepachangelog.com"],
        words: &[],
        records: false,
    },
    Named {
        anchor: "semantic-versioning",
        id: "versioning",
        phrases: &["semantic versioning", "semver.org"],
        words: &["semver"],
        records: false,
    },
    Named {
        anchor: "madr",
        id: "madr",
        phrases: &[
            "adr.github.io/madr",
            "markdown architectural decision record",
        ],
        words: &["madr"],
        records: true,
    },
    Named {
        anchor: "adr-according-to-nygard",
        id: "nygard-adr",
        phrases: &[],
        words: &["nygard"],
        records: true,
    },
    Named {
        anchor: "gherkin",
        id: "gherkin",
        phrases: &[],
        words: &["gherkin"],
        records: false,
    },
];

/// A lint or format tool: the configuration that shows the project uses it and the command
/// that checks it. `marker` is a file whose text must contain a needle (a shared manifest).
struct Tool {
    id: &'static str,
    title: &'static str,
    files: &'static [&'static str],
    marker: Option<(&'static str, &'static [&'static str])>,
    command: &'static str,
    args: &'static [&'static str],
    languages: &'static [&'static str],
    /// The package script group that states the project's own command for this tool.
    script: Option<&'static str>,
}

const TOOLS: [Tool; 10] = [
    Tool {
        id: "rustfmt",
        title: "rustfmt",
        files: &["rustfmt.toml", ".rustfmt.toml"],
        marker: None,
        command: "cargo",
        args: &["fmt", "--all", "--", "--check"],
        languages: &["rust"],
        script: None,
    },
    Tool {
        id: "clippy",
        title: "Clippy",
        files: &["clippy.toml", ".clippy.toml"],
        marker: Some(("Cargo.toml", &["[lints", "[workspace.lints"])),
        command: "cargo",
        args: &["clippy", "--all-targets"],
        languages: &["rust"],
        script: None,
    },
    Tool {
        id: "eslint",
        title: "ESLint",
        files: &[
            "eslint.config.js",
            "eslint.config.mjs",
            "eslint.config.cjs",
            "eslint.config.ts",
            ".eslintrc",
            ".eslintrc.js",
            ".eslintrc.cjs",
            ".eslintrc.json",
            ".eslintrc.yml",
            ".eslintrc.yaml",
        ],
        marker: None,
        command: "npx",
        args: &["eslint", "."],
        languages: &["javascript", "typescript"],
        script: Some("lint"),
    },
    Tool {
        id: "biome",
        title: "Biome",
        files: &["biome.json", "biome.jsonc"],
        marker: None,
        command: "npx",
        args: &["biome", "check", "."],
        languages: &["javascript", "typescript"],
        script: Some("lint"),
    },
    Tool {
        id: "prettier",
        title: "Prettier",
        files: &[
            ".prettierrc",
            ".prettierrc.json",
            ".prettierrc.yml",
            ".prettierrc.yaml",
            ".prettierrc.toml",
            ".prettierrc.js",
            ".prettierrc.cjs",
            ".prettierrc.mjs",
            "prettier.config.js",
            "prettier.config.cjs",
            "prettier.config.mjs",
        ],
        marker: None,
        command: "npx",
        args: &["prettier", "--check", "."],
        languages: &[],
        script: Some("format"),
    },
    Tool {
        id: "ruff",
        title: "Ruff",
        files: &["ruff.toml", ".ruff.toml"],
        marker: Some(("pyproject.toml", &["[tool.ruff"])),
        command: "ruff",
        args: &["check", "."],
        languages: &["python"],
        script: None,
    },
    Tool {
        id: "black",
        title: "Black",
        files: &[],
        marker: Some(("pyproject.toml", &["[tool.black"])),
        command: "black",
        args: &["--check", "."],
        languages: &["python"],
        script: None,
    },
    Tool {
        id: "flake8",
        title: "Flake8",
        files: &[".flake8"],
        marker: None,
        command: "flake8",
        args: &[],
        languages: &["python"],
        script: None,
    },
    Tool {
        id: "golangci-lint",
        title: "golangci-lint",
        files: &[
            ".golangci.yml",
            ".golangci.yaml",
            ".golangci.toml",
            ".golangci.json",
        ],
        marker: None,
        command: "golangci-lint",
        args: &["run"],
        languages: &["go"],
        script: None,
    },
    Tool {
        id: "rubocop",
        title: "RuboCop",
        files: &[".rubocop.yml"],
        marker: None,
        command: "bundle",
        args: &["exec", "rubocop"],
        languages: &["ruby"],
        script: None,
    },
];

/// Package scripts that state a check by name: the group, the standard's id, what it keeps
/// and the script names that count, first present wins.
const SCRIPTS: [(&str, &str, &str, &[&str]); 3] = [
    ("lint", "lint", "lint", &["lint"]),
    (
        "format",
        "format",
        "formatting",
        &["format:check", "fmt:check", "format-check", "check:format"],
    ),
    (
        "types",
        "typecheck",
        "type",
        &["typecheck", "type-check", "check:types"],
    ),
];

/// How the scan looks at a candidate path.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Look {
    /// A file whose text is read.
    Text,
    /// A file whose presence is enough.
    File,
    /// A folder whose presence is enough.
    Folder,
}

/// One root path as the scan sees it: present, and its text when it was read.
pub struct Source<'a> {
    pub path: &'a str,
    pub text: Option<&'a str>,
}

/// Every project-relative path the scan looks at, and how. The caller reads within
/// [`MAX_BYTES`] and reports what it could not.
pub fn candidates() -> Vec<(&'static str, Look)> {
    let mut list: Vec<(&'static str, Look)> = Vec::new();
    let mut add = |path: &'static str, read: Look| {
        if !list.iter().any(|(known, _)| *known == path) {
            list.push((path, read));
        }
    };
    for path in INSTRUCTIONS.iter().chain(&CONTRIBUTING).chain(&COMMITLINT) {
        add(path, Look::Text);
    }
    for path in [CHANGELOG, EDITORCONFIG, PACKAGE] {
        add(path, Look::Text);
    }
    for path in DECISIONS {
        add(path, Look::Folder);
    }
    for tool in &TOOLS {
        if let Some((file, _)) = tool.marker {
            add(file, Look::Text);
        }
        for file in tool.files {
            add(file, Look::File);
        }
    }
    for (file, _) in MANAGERS {
        add(file, Look::File);
    }
    list
}

fn word_character(character: Option<char>) -> bool {
    character.is_some_and(|c| c.is_alphanumeric() || c == '_')
}

/// The first 1-based line of `text` naming the convention.
fn named_at(named: &Named, text: &str) -> Option<usize> {
    text.lines()
        .position(|line| {
            let line = line.to_lowercase();
            named.phrases.iter().any(|phrase| line.contains(phrase))
                || named.words.iter().any(|word| {
                    line.match_indices(word).any(|(at, found)| {
                        !word_character(line[..at].chars().next_back())
                            && !word_character(line[at + found.len()..].chars().next())
                    })
                })
        })
        .map(|line| line + 1)
}

/// What `[*]` of an EditorConfig file asks for, in one line.
fn editor_summary(text: &str) -> String {
    let mut section = String::new();
    let mut settings: Vec<(String, String)> = Vec::new();
    for line in text.lines().map(str::trim) {
        if line.starts_with('[') {
            section = line.to_string();
        } else if section == "[*]"
            && !line.starts_with(['#', ';'])
            && let Some((key, value)) = line.split_once('=')
        {
            settings.push((key.trim().to_lowercase(), value.trim().to_lowercase()));
        }
    }
    let value = |key: &str| {
        settings
            .iter()
            .find(|(known, _)| known == key)
            .map(|(_, value)| value.as_str())
    };
    let mut parts: Vec<String> = Vec::new();
    match (value("indent_style"), value("indent_size")) {
        (Some("tab"), _) => parts.push("tab indentation".into()),
        (Some("space"), Some(size)) => parts.push(format!("{size}-space indentation")),
        (Some("space"), None) => parts.push("space indentation".into()),
        _ => {}
    }
    if let Some(end) = value("end_of_line") {
        parts.push(format!("{} line endings", end.to_uppercase()));
    }
    if let Some(charset) = value("charset") {
        parts.push(charset.to_uppercase());
    }
    if value("insert_final_newline") == Some("true") {
        parts.push("final newline".into());
    }
    if value("trim_trailing_whitespace") == Some("true") {
        parts.push("no trailing whitespace".into());
    }
    if let Some(length) = value("max_line_length").filter(|length| *length != "off") {
        parts.push(format!("lines up to {length}"));
    }
    if parts.is_empty() {
        "Formatting follows .editorconfig.".into()
    } else {
        format!("Formatting follows .editorconfig: {}.", parts.join(", "))
    }
}

fn evidence(file: &str, line: Option<usize>) -> Value {
    match line {
        Some(line) => json!({"file": file, "line": line}),
        None => json!({"file": file}),
    }
}

/// Whether the project already declares what `standard` would add: the same anchor, the same
/// file or the same command.
fn declared<'a>(settings: &'a Value, standard: &Value) -> Option<&'a str> {
    fn anchor(value: &Value) -> Option<&str> {
        value["anchor"].as_str().map(anchors::canonical)
    }
    settings["standards"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|known| {
            (standard.get("anchor").is_some() && anchor(known) == anchor(standard))
                || (standard.get("file").is_some() && known["file"] == standard["file"])
                || (standard.get("run").is_some()
                    && known["run"]["command"] == standard["run"]["command"]
                    && known["run"].get("args") == standard["run"].get("args"))
        })
        .and_then(|known| known["id"].as_str())
}

/// The standards the scanned files support: `{proposals, declared}`. A proposal carries the
/// declaration to copy, its form and the files that support it; `declared` lists what the
/// project configuration already covers. Every proposal passes the project validator, and
/// ids never collide with declared ones.
pub fn propose(settings: &Value, sources: &[Source]) -> Result<Value, String> {
    let text = |path: &str| {
        sources
            .iter()
            .find(|source| source.path == path)
            .and_then(|source| source.text)
    };
    let present = |path: &str| sources.iter().any(|source| source.path == path);
    let mut found: Vec<(&str, Value, Vec<Value>)> = Vec::new();

    let disabled = |id: &str| {
        settings["anchors"]["disables"]
            .as_array()
            .is_some_and(|names| {
                names
                    .iter()
                    .filter_map(Value::as_str)
                    .any(|name| anchors::canonical(name) == id)
            })
    };
    for named in &NAMED {
        let offered = anchors::resolve(settings, named.anchor)
            .is_some_and(|anchor| anchor.tier != Tier::Refused && anchor.check.is_some());
        if !offered || disabled(named.anchor) {
            continue;
        }
        let mut support: Vec<Value> = INSTRUCTIONS
            .iter()
            .chain(&CONTRIBUTING)
            .filter_map(|path| Some(evidence(path, Some(named_at(named, text(path)?)?))))
            .collect();
        // A changelog states its own format in its preamble; its entries only mention names.
        if CHANGELOG_NAMES.contains(&named.anchor)
            && let Some(line) = text(CHANGELOG)
                .and_then(|text| named_at(named, text))
                .filter(|line| *line <= CHANGELOG_PREAMBLE)
        {
            support.push(evidence(CHANGELOG, Some(line)));
        }
        if named.anchor == "conventional-commits" {
            support.extend(
                COMMITLINT
                    .iter()
                    .chain(&[PACKAGE])
                    .filter(|path| text(path).is_some_and(|t| t.contains("config-conventional")))
                    .map(|path| evidence(path, None)),
            );
        }
        if support.is_empty() {
            continue;
        }
        let mut standard = json!({"id": named.id, "anchor": named.anchor});
        if named.records {
            // Without the folder the check would read nothing: no proposal.
            let Some(folder) = DECISIONS.iter().find(|folder| present(folder)) else {
                continue;
            };
            standard["scope"] = json!({"paths": [folder]});
        }
        found.push(("anchor", standard, support));
    }

    let package: Value = text(PACKAGE)
        .and_then(|text| serde_json::from_str(text).ok())
        .unwrap_or(Value::Null);
    let manager = MANAGERS
        .iter()
        .find(|(file, _)| present(file))
        .map_or("npm", |(_, manager)| manager);
    let mut scripted: Vec<&str> = Vec::new();
    for (group, id, what, names) in SCRIPTS {
        let Some(script) = names
            .iter()
            .find(|name| package["scripts"][**name].is_string())
        else {
            continue;
        };
        scripted.push(group);
        found.push((
            "run",
            json!({
                "id": id,
                "run": {"command": manager, "args": ["run", script]},
                "summary": format!("The project's {what} check passes: `{manager} run {script}`."),
            }),
            vec![evidence(PACKAGE, None)],
        ));
    }
    for tool in &TOOLS {
        if tool.script.is_some_and(|group| scripted.contains(&group)) {
            continue;
        }
        let mut support: Vec<Value> = tool
            .files
            .iter()
            .filter(|file| present(file))
            .map(|file| evidence(file, None))
            .collect();
        if let Some((file, needles)) = tool.marker
            && text(file).is_some_and(|text| needles.iter().any(|needle| text.contains(needle)))
        {
            support.push(evidence(file, None));
        }
        if support.is_empty() {
            continue;
        }
        let command = std::iter::once(tool.command)
            .chain(tool.args.iter().copied())
            .collect::<Vec<_>>()
            .join(" ");
        let mut standard = json!({
            "id": tool.id,
            "run": {"command": tool.command},
            "summary": format!("{} is configured for this project: `{command}` passes.", tool.title),
        });
        if !tool.args.is_empty() {
            standard["run"]["args"] = json!(tool.args);
        }
        if !tool.languages.is_empty() {
            standard["scope"] = json!({"languages": tool.languages});
        }
        found.push(("run", standard, support));
    }

    if let Some(path) = CONTRIBUTING.iter().find(|path| present(path)) {
        found.push((
            "file",
            json!({
                "id": "contributing",
                "file": path,
                "summary": format!("Contribution rules the project states in {path}."),
            }),
            vec![evidence(path, None)],
        ));
    }
    if let Some(text) = text(EDITORCONFIG) {
        found.push((
            "file",
            json!({"id": "editorconfig", "file": EDITORCONFIG, "summary": editor_summary(text)}),
            vec![evidence(EDITORCONFIG, None)],
        ));
    }

    let known = settings["standards"]
        .as_array()
        .map_or(&[][..], Vec::as_slice);
    let mut ids: Vec<String> = known
        .iter()
        .filter_map(|standard| standard["id"].as_str().map(str::to_owned))
        .collect();
    let room = MAX_STANDARDS.saturating_sub(known.len());
    let catalog = crate::routing::catalog::catalog()?;
    let intents: Vec<&str> = catalog["intents"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .collect();
    let mut proposals = Vec::new();
    let mut covered = Vec::new();
    for (form, mut standard, support) in found {
        if let Some(id) = declared(settings, &standard) {
            covered.push(json!({"standard": id, "evidence": support}));
            continue;
        }
        if proposals.len() == room {
            break;
        }
        let base = standard["id"].as_str().unwrap_or_default().to_string();
        let mut id = base.clone();
        let mut next = 2;
        while ids.contains(&id) {
            id = format!("{base}-{next}");
            next += 1;
        }
        standard["id"] = json!(id);
        standards::validate(&json!([standard]), &[], &intents)?;
        let level = standards::level(settings, &standard).to_string();
        ids.push(id);
        proposals.push(json!({
            "standard": standard,
            "form": form,
            "level": level,
            "evidence": support,
        }));
    }
    Ok(json!({"proposals": proposals, "declared": covered}))
}

/// How many proposed standards a session start names; the rest are counted.
const OFFER_NAMES: usize = 6;

/// What a session start says about `proposed` (the result of [`propose`]) in a project
/// that declares no standards: `(offer, record)`, the offer `{count, standards}` and the
/// record to keep so the same set is not named again. `project` is the project's own
/// configuration: standards it inherits from an organization or user layer are not its
/// declarations, and what they cover is already left out of `proposed`. `None` when the
/// project declares a standard, nothing is proposed, or `shown` already records this set.
pub fn offer(project: &Value, proposed: &Value, shown: &Value) -> Option<(Value, Value)> {
    let declares = project["standards"]
        .as_array()
        .is_some_and(|standards| !standards.is_empty());
    let ids: Vec<&str> = proposed["proposals"]
        .as_array()?
        .iter()
        .filter_map(|proposal| proposal["standard"]["id"].as_str())
        .collect();
    if declares || ids.is_empty() {
        return None;
    }
    let key = crate::hash(ids.join("\n").as_bytes());
    if shown["formatVersion"] == 1 && shown["key"] == key.as_str() {
        return None;
    }
    let named = &ids[..ids.len().min(OFFER_NAMES)];
    Some((
        json!({"count": ids.len(), "standards": named}),
        json!({"formatVersion": 1, "key": key}),
    ))
}

/// The line a user reads for an offer.
pub fn offer_line(offer: &Value) -> String {
    let count = offer["count"].as_u64().unwrap_or_default();
    let names: Vec<&str> = offer["standards"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .collect();
    let more = (count as usize).saturating_sub(names.len());
    format!(
        "Craft: this project's own files support {count} standard{} it has not declared ({}{}). `craft standards --propose` lists them with evidence; none is active until you add it.",
        if count == 1 { "" } else { "s" },
        names.join(", "),
        if more > 0 {
            format!(", {more} more")
        } else {
            String::new()
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scan(settings: &Value, files: &[(&str, Option<&str>)]) -> Value {
        let sources: Vec<Source> = files
            .iter()
            .map(|(path, text)| Source { path, text: *text })
            .collect();
        propose(settings, &sources).expect("proposal")
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

    #[test]
    fn an_undeclared_project_is_offered_its_proposals_once_per_set() {
        let proposed = scan(&json!({}), &[("rustfmt.toml", None), (".flake8", None)]);
        let (offer, record) = offer(&json!({}), &proposed, &Value::Null).expect("offer");
        assert_eq!(
            offer,
            json!({"count": 2, "standards": ["rustfmt", "flake8"]})
        );
        assert_eq!(
            offer_line(&offer),
            "Craft: this project's own files support 2 standards it has not declared (rustfmt, flake8). `craft standards --propose` lists them with evidence; none is active until you add it."
        );
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
}
