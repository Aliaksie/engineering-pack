//! Deterministic checks for structure-tier anchors. The model is shown a one-line
//! summary instead of the standard's text, so each check holds only the elements both models
//! listed as required when asked for the name alone; informational and divergent elements
//! (ADR section order, changelog title, bump level without a declared change kind) stay out.
//! Checks report findings and never decide whether work may proceed.
use serde_json::Value;
use std::cmp::Ordering;

/// The checks an overlay entry may name in `check`.
pub const CHECKS: &[&str] = &[
    "conventional-commits",
    "keep-a-changelog",
    "semver",
    "nygard-adr",
    "madr",
    "gherkin",
    "ears",
    "connextra",
];

#[derive(Debug, PartialEq, Eq)]
pub struct Finding {
    pub rule: &'static str,
    /// 1-based.
    pub line: Option<usize>,
    pub message: String,
}

pub(super) fn finding(
    rule: &'static str,
    line: Option<usize>,
    message: impl Into<String>,
) -> Finding {
    Finding {
        rule,
        line,
        message: message.into(),
    }
}

/// Findings for `text` under the check named `name`; empty means it passes. `path` is the project-relative file (None for a commit message); used by madr (file name) and semver (which manifest).
pub fn check(name: &str, text: &str, path: Option<&str>) -> Vec<Finding> {
    match name {
        "conventional-commits" => conventional_commit(text),
        "keep-a-changelog" => changelog(text),
        "semver" => semver(text, path),
        "nygard-adr" => nygard(text),
        "madr" => madr(text, path),
        "gherkin" => gherkin(text),
        "ears" => ears(text),
        "connextra" => connextra(text),
        _ => vec![finding(
            "unknown-anchor",
            None,
            format!("no anchor check named {name}"),
        )],
    }
}

fn file_name(path: &str) -> &str {
    path.rsplit(['/', '\\']).next().unwrap_or(path)
}

/// A list item, quote or numbered line with its marker removed, and whether one was there.
fn strip_marker(line: &str) -> (&str, bool) {
    let line = line.trim_start();
    for marker in ["- ", "* ", "+ ", "> "] {
        if let Some(rest) = line.strip_prefix(marker) {
            return (rest.trim(), true);
        }
    }
    let digits = line.bytes().take_while(u8::is_ascii_digit).count();
    if digits > 0 {
        let rest = &line[digits..];
        if let Some(rest) = rest.strip_prefix(". ").or_else(|| rest.strip_prefix(") ")) {
            return (rest.trim(), true);
        }
    }
    (line.trim(), false)
}

fn unbold(line: &str) -> String {
    line.replace("**", "").replace("__", "")
}

// Conventional Commits

fn conventional_commit(text: &str) -> Vec<Finding> {
    // A Git commit-msg file carries `#` comments and, under `commit -v`, a diff after the scissors.
    let mut lines: Vec<(usize, &str)> = text
        .lines()
        .enumerate()
        .take_while(|(_, line)| !(line.starts_with('#') && line.contains(">8")))
        .filter(|(_, line)| !line.starts_with('#'))
        .map(|(index, line)| (index + 1, line.trim_end()))
        .collect();
    while lines.first().is_some_and(|(_, line)| line.is_empty()) {
        lines.remove(0);
    }
    while lines.last().is_some_and(|(_, line)| line.is_empty()) {
        lines.pop();
    }
    let Some(&(number, header)) = lines.first() else {
        return vec![finding("header", None, "the commit message is empty")];
    };
    let mut findings = Vec::new();
    if let Err(problem) = commit_header(header) {
        findings.push(finding("header", Some(number), problem));
    }
    if let Some(&(number, line)) = lines.get(1)
        && !line.is_empty()
    {
        findings.push(finding(
            "blank-line",
            Some(number),
            "the header must be followed by a blank line",
        ));
    }
    if let Some(paragraph) = lines[1..]
        .split(|(_, line)| line.is_empty())
        .rfind(|paragraph| !paragraph.is_empty())
    {
        footer_findings(paragraph, &mut findings);
    }
    findings
}

fn commit_header(header: &str) -> Result<(), &'static str> {
    let kind = header
        .find(|c: char| !c.is_ascii_lowercase())
        .unwrap_or(header.len());
    if kind == 0 {
        return Err("the header must start with a lowercase type");
    }
    let mut rest = &header[kind..];
    if let Some(scoped) = rest.strip_prefix('(') {
        let Some((scope, after)) = scoped.split_once(')') else {
            return Err("the scope is not closed");
        };
        if scope.trim().is_empty() || scope.contains('(') {
            return Err("the scope must be a non-empty noun");
        }
        rest = after;
    }
    rest = rest.strip_prefix('!').unwrap_or(rest);
    let Some(description) = rest.strip_prefix(": ") else {
        return Err("the type must be followed by `: ` and a description");
    };
    if description.trim().is_empty() || description.starts_with(char::is_whitespace) {
        return Err("the description must follow a single space");
    }
    Ok(())
}

fn breaking(token: &str) -> bool {
    token.eq_ignore_ascii_case("breaking change") || token.eq_ignore_ascii_case("breaking-change")
}

fn strict_token(token: &str) -> bool {
    !token.is_empty()
        && token
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-')
}

/// The token of a line shaped like a footer: up to three words, then `: value` or ` #value`.
fn footer_token(line: &str) -> Option<&str> {
    let end = match (line.find(": "), line.find(" #")) {
        (Some(colon), Some(hash)) => colon.min(hash),
        (colon, hash) => colon.or(hash)?,
    };
    let token = &line[..end];
    let words: Vec<&str> = token.split(' ').collect();
    let shaped = token.starts_with(|c: char| c.is_ascii_alphabetic())
        && words.len() <= 3
        && words.iter().all(|word| strict_token(word));
    (shaped && !line[end + 2..].trim().is_empty()).then_some(token)
}

/// Only a trailing paragraph that reads as footers is judged, so body prose with a colon is
/// never mistaken for a malformed token. A footer value may continue over following lines.
fn footer_findings(paragraph: &[(usize, &str)], findings: &mut Vec<Finding>) {
    let tokens: Vec<Option<&str>> = paragraph
        .iter()
        .map(|(_, line)| footer_token(line))
        .collect();
    let Some(first) = tokens[0] else { return };
    let all_footers = tokens.iter().all(Option::is_some);
    let anchored = tokens
        .iter()
        .flatten()
        .any(|token| strict_token(token) || breaking(token));
    if !anchored || !(all_footers || strict_token(first) || breaking(first)) {
        return;
    }
    for (&(number, _), token) in paragraph.iter().zip(&tokens) {
        let Some(token) = *token else { continue };
        let invalid = if breaking(token) {
            token != "BREAKING CHANGE" && token != "BREAKING-CHANGE"
        } else {
            all_footers && !strict_token(token)
        };
        if invalid {
            findings.push(finding(
                "footer-token",
                Some(number),
                format!(
                    "footer token `{token}` must use hyphens for spaces; only `BREAKING CHANGE` (uppercase) may contain one"
                ),
            ));
        }
    }
}

// Keep a Changelog

const CHANGE_GROUPS: &[&str] = &[
    "Added",
    "Changed",
    "Deprecated",
    "Removed",
    "Fixed",
    "Security",
];

fn fence(line: &str) -> bool {
    let line = line.trim_start();
    line.starts_with("```") || line.starts_with("~~~")
}

/// Numbered lines outside fenced code blocks.
fn prose_lines(text: &str) -> Vec<(usize, &str)> {
    let mut fenced = false;
    let mut lines = Vec::new();
    for (index, line) in text.lines().enumerate() {
        if fence(line) {
            fenced = !fenced;
        } else if !fenced {
            lines.push((index + 1, line));
        }
    }
    lines
}

enum Release<'a> {
    Unreleased,
    Version(&'a str),
}

fn release_heading(title: &str) -> Result<Release<'_>, &'static str> {
    if title.eq_ignore_ascii_case("[unreleased]") {
        return Ok(Release::Unreleased);
    }
    let format = "a release heading must be `## [Unreleased]` or `## [X.Y.Z] - YYYY-MM-DD`";
    let Some((version, rest)) = title
        .strip_prefix('[')
        .and_then(|rest| rest.split_once(']'))
    else {
        return Err(format);
    };
    let Some(date) = rest.strip_prefix(" - ") else {
        return Err(format);
    };
    let date = date.strip_suffix(" [YANKED]").unwrap_or(date);
    if loose_core(version).is_none() {
        return Err(format);
    }
    if !iso_date(date) {
        return Err("the release date must be YYYY-MM-DD");
    }
    Ok(Release::Version(version))
}

fn iso_date(date: &str) -> bool {
    let bytes = date.as_bytes();
    let shaped = bytes.len() == 10
        && bytes.iter().enumerate().all(|(index, byte)| {
            if index == 4 || index == 7 {
                *byte == b'-'
            } else {
                byte.is_ascii_digit()
            }
        });
    shaped
        && date[5..7]
            .parse::<u8>()
            .is_ok_and(|month| (1..=12).contains(&month))
        && date[8..10]
            .parse::<u8>()
            .is_ok_and(|day| (1..=31).contains(&day))
}

fn changelog(text: &str) -> Vec<Finding> {
    let mut findings = Vec::new();
    let mut previous: Option<&str> = None;
    let mut unreleased = false;
    for (number, line) in prose_lines(text) {
        if let Some(title) = line.strip_prefix("## ") {
            match release_heading(title.trim()) {
                Ok(Release::Unreleased) => {
                    if unreleased || previous.is_some() {
                        findings.push(finding(
                            "order",
                            Some(number),
                            "`## [Unreleased]` must come once, before every release",
                        ));
                    }
                    unreleased = true;
                }
                Ok(Release::Version(version)) => {
                    if let Some(newer) = previous
                        && release_order(version, newer) != Ordering::Less
                    {
                        findings.push(finding(
                            "order",
                            Some(number),
                            format!(
                                "release {version} must be older than {newer}, which precedes it"
                            ),
                        ));
                    }
                    previous = Some(version);
                }
                Err(problem) => findings.push(finding("heading", Some(number), problem)),
            }
        } else if let Some(title) = line.strip_prefix("### ") {
            let title = title.trim();
            if !CHANGE_GROUPS
                .iter()
                .any(|group| group.eq_ignore_ascii_case(title))
            {
                findings.push(finding(
                    "heading",
                    Some(number),
                    format!(
                        "`### {title}` is not a change group; use Added, Changed, Deprecated, Removed, Fixed or Security"
                    ),
                ));
            }
        }
    }
    findings
}

/// The three numeric parts of a release, tolerating leading zeros; grammar is semver's job.
fn loose_core(version: &str) -> Option<[&str; 3]> {
    let core = version.split(['-', '+']).next()?;
    let parts: Vec<&str> = core.split('.').collect();
    match parts.as_slice() {
        &[major, minor, patch]
            if [major, minor, patch]
                .iter()
                .all(|part| !part.is_empty() && part.bytes().all(|b| b.is_ascii_digit())) =>
        {
            Some([major, minor, patch])
        }
        _ => None,
    }
}

fn release_order(left: &str, right: &str) -> Ordering {
    let (Some(a), Some(b)) = (loose_core(left), loose_core(right)) else {
        return Ordering::Equal;
    };
    let core = a
        .iter()
        .zip(b)
        .map(|(x, y)| numeric_order(x, y))
        .find(|order| order.is_ne())
        .unwrap_or(Ordering::Equal);
    if core.is_ne() {
        return core;
    }
    match (parse_version(left), parse_version(right)) {
        (Ok(a), Ok(b)) => precedence(&a, &b),
        _ => Ordering::Equal,
    }
}

// Semantic Versioning

struct Version<'a> {
    core: [&'a str; 3],
    pre: Vec<&'a str>,
}

fn identifier(id: &str) -> bool {
    !id.is_empty() && id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
}

fn digits(id: &str) -> bool {
    !id.is_empty() && id.bytes().all(|b| b.is_ascii_digit())
}

fn leading_zero(id: &str) -> bool {
    id.len() > 1 && id.starts_with('0')
}

/// The SemVer 2.0.0 grammar.
fn parse_version(text: &str) -> Result<Version<'_>, &'static str> {
    let (rest, build) = match text.split_once('+') {
        Some((rest, build)) => (rest, Some(build)),
        None => (text, None),
    };
    if let Some(build) = build
        && !build.split('.').all(identifier)
    {
        return Err("build metadata must be dot-separated non-empty [0-9A-Za-z-] identifiers");
    }
    let (core, pre) = match rest.split_once('-') {
        Some((core, pre)) => (core, Some(pre)),
        None => (rest, None),
    };
    let parts: Vec<&str> = core.split('.').collect();
    let &[major, minor, patch] = parts.as_slice() else {
        return Err("a version must be MAJOR.MINOR.PATCH");
    };
    if [major, minor, patch]
        .iter()
        .any(|part| !digits(part) || leading_zero(part))
    {
        return Err("MAJOR, MINOR and PATCH must be non-negative integers without leading zeros");
    }
    let pre: Vec<&str> = pre.map(|pre| pre.split('.').collect()).unwrap_or_default();
    if pre
        .iter()
        .any(|id| !identifier(id) || (digits(id) && leading_zero(id)))
    {
        return Err(
            "pre-release identifiers must be non-empty [0-9A-Za-z-], numeric ones without leading zeros",
        );
    }
    Ok(Version {
        core: [major, minor, patch],
        pre,
    })
}

/// Compares digit strings of any length without parsing them into a bounded integer.
fn numeric_order(left: &str, right: &str) -> Ordering {
    let left = left.trim_start_matches('0');
    let right = right.trim_start_matches('0');
    left.len().cmp(&right.len()).then_with(|| left.cmp(right))
}

fn precedence(left: &Version, right: &Version) -> Ordering {
    let core = left
        .core
        .iter()
        .zip(right.core)
        .map(|(a, b)| numeric_order(a, b))
        .find(|order| order.is_ne())
        .unwrap_or(Ordering::Equal);
    if core.is_ne() {
        return core;
    }
    match (left.pre.is_empty(), right.pre.is_empty()) {
        (true, true) => Ordering::Equal,
        (true, false) => Ordering::Greater,
        (false, true) => Ordering::Less,
        (false, false) => left
            .pre
            .iter()
            .zip(&right.pre)
            .map(|(a, b)| match (digits(a), digits(b)) {
                (true, true) => numeric_order(a, b),
                (true, false) => Ordering::Less,
                (false, true) => Ordering::Greater,
                (false, false) => a.cmp(b),
            })
            .find(|order| order.is_ne())
            .unwrap_or_else(|| left.pre.len().cmp(&right.pre.len())),
    }
}

fn semver(text: &str, path: Option<&str>) -> Vec<Finding> {
    let versions: Vec<(Option<usize>, &str)> = match path.map(file_name) {
        None => text
            .lines()
            .enumerate()
            .filter(|(_, line)| !line.trim().is_empty())
            .map(|(index, line)| (Some(index + 1), line.trim()))
            .collect(),
        Some("package.json") => return package_version(text),
        Some("Cargo.toml") => toml_versions(text, &["package", "workspace.package"]),
        Some("pyproject.toml") => toml_versions(text, &["project", "tool.poetry"]),
        Some(name) if name.to_ascii_lowercase().ends_with(".md") => changelog_versions(text),
        Some(_) => Vec::new(),
    };
    versions
        .into_iter()
        .filter_map(|(line, version)| {
            parse_version(version)
                .err()
                .map(|problem| finding("version", line, format!("`{version}`: {problem}")))
        })
        .collect()
}

/// A package without a version is valid (private packages), so only a present one is judged.
fn package_version(text: &str) -> Vec<Finding> {
    let manifest: Value = match serde_json::from_str(text) {
        Ok(manifest) => manifest,
        Err(error) => {
            return vec![finding(
                "parse",
                None,
                format!("package.json is not valid JSON: {error}"),
            )];
        }
    };
    let line = text
        .lines()
        .position(|line| line.trim_start().starts_with("\"version\""))
        .map(|index| index + 1);
    match manifest.get("version") {
        None => Vec::new(),
        Some(Value::String(version)) => parse_version(version)
            .err()
            .map(|problem| finding("version", line, format!("`{version}`: {problem}")))
            .into_iter()
            .collect(),
        Some(_) => vec![finding(
            "version",
            line,
            "package.json `version` must be a string",
        )],
    }
}

/// `version = "…"` keys directly under the named TOML tables; inherited and dotted keys are skipped.
fn toml_versions<'a>(text: &'a str, tables: &[&str]) -> Vec<(Option<usize>, &'a str)> {
    let mut current = "";
    let mut versions = Vec::new();
    for (index, line) in text.lines().enumerate() {
        let line = line.trim();
        if let Some(header) = line.strip_prefix('[') {
            current = if header.starts_with('[') {
                ""
            } else {
                header.split(']').next().unwrap_or("").trim()
            };
            continue;
        }
        if !tables.contains(&current) {
            continue;
        }
        let Some(value) = line
            .strip_prefix("version")
            .map(str::trim_start)
            .and_then(|rest| rest.strip_prefix('='))
            .map(str::trim)
        else {
            continue;
        };
        let Some(quote) = value.chars().next().filter(|c| *c == '"' || *c == '\'') else {
            continue;
        };
        if let Some((version, _)) = value[1..].split_once(quote) {
            versions.push((Some(index + 1), version));
        }
    }
    versions
}

fn changelog_versions(text: &str) -> Vec<(Option<usize>, &str)> {
    prose_lines(text)
        .into_iter()
        .filter_map(|(number, line)| {
            let version = line.strip_prefix("## [")?.split_once(']')?.0;
            (!version.eq_ignore_ascii_case("unreleased")).then_some((Some(number), version))
        })
        .collect()
}

// Markdown decision records

struct Heading<'a> {
    level: usize,
    text: &'a str,
    index: usize,
}

struct Markdown<'a> {
    lines: Vec<&'a str>,
    headings: Vec<Heading<'a>>,
}

fn heading(line: &str) -> Option<(usize, &str)> {
    let line = line.trim_start();
    let level = line.bytes().take_while(|b| *b == b'#').count();
    let rest = &line[level..];
    let shaped = (1..=6).contains(&level) && (rest.is_empty() || rest.starts_with([' ', '\t']));
    shaped.then(|| (level, rest.trim()))
}

/// Blank, or a lone HTML comment such as a template's unfilled guidance.
fn blank(line: &str) -> bool {
    let line = line.trim();
    line.is_empty() || (line.starts_with("<!--") && line.ends_with("-->"))
}

impl<'a> Markdown<'a> {
    fn parse(text: &'a str) -> Self {
        let lines: Vec<&str> = text.lines().collect();
        let mut headings = Vec::new();
        let mut fenced = false;
        for (index, &line) in lines.iter().enumerate() {
            if fence(line) {
                fenced = !fenced;
            } else if !fenced && let Some((level, text)) = heading(line) {
                headings.push(Heading { level, text, index });
            }
        }
        Self { lines, headings }
    }

    fn title(&self) -> Option<&Heading<'a>> {
        self.headings.iter().find(|heading| heading.level == 1)
    }

    fn section(&self, name: &str) -> Option<&Heading<'a>> {
        self.headings.iter().find(|heading| {
            heading.level >= 2
                && heading
                    .text
                    .trim_end_matches(':')
                    .trim()
                    .eq_ignore_ascii_case(name)
        })
    }

    /// Non-blank numbered lines up to the next heading at the same or a higher level.
    fn body(&self, heading: &Heading<'a>) -> Vec<(usize, &'a str)> {
        let end = self
            .headings
            .iter()
            .find(|next| next.index > heading.index && next.level <= heading.level)
            .map_or(self.lines.len(), |next| next.index);
        self.lines[heading.index + 1..end]
            .iter()
            .enumerate()
            .map(|(offset, line)| (heading.index + 2 + offset, *line))
            .filter(|(_, line)| !blank(line))
            .collect()
    }

    /// The body of each required section, recording missing and empty ones.
    fn required(
        &self,
        names: &[&str],
        findings: &mut Vec<Finding>,
    ) -> Vec<Option<Vec<(usize, &'a str)>>> {
        names
            .iter()
            .map(|name| {
                let Some(heading) = self.section(name) else {
                    findings.push(finding(
                        "missing-section",
                        None,
                        format!("no `## {name}` section"),
                    ));
                    return None;
                };
                let body = self.body(heading);
                if body.is_empty() {
                    findings.push(finding(
                        "empty-section",
                        Some(heading.index + 1),
                        format!("the {name} section is empty"),
                    ));
                    return None;
                }
                Some(body)
            })
            .collect()
    }
}

const STATUSES: &[&str] = &["proposed", "accepted", "deprecated", "superseded"];

fn numbered(title: &str) -> bool {
    let lower = title.to_ascii_lowercase();
    let rest = lower.strip_prefix("adr").map_or(lower.as_str(), |rest| {
        rest.trim_start_matches(['-', ' ', ':', '_'])
    });
    rest.starts_with(|c: char| c.is_ascii_digit())
}

fn nygard(text: &str) -> Vec<Finding> {
    let doc = Markdown::parse(text);
    let mut findings = Vec::new();
    match doc.title() {
        None => findings.push(finding("title", None, "no numbered `# ` title")),
        Some(title) if !numbered(title.text) => findings.push(finding(
            "title",
            Some(title.index + 1),
            "the title must start with the ADR number",
        )),
        Some(_) => {}
    }
    let bodies = doc.required(
        &["Context", "Decision", "Status", "Consequences"],
        &mut findings,
    );
    if let Some(Some(status)) = bodies.get(2)
        && let Some(&(number, line)) = status.first()
    {
        let word: String = line
            .trim_start_matches(|c: char| !c.is_alphabetic())
            .chars()
            .take_while(|c| c.is_alphabetic())
            .collect();
        if !STATUSES.contains(&word.to_lowercase().as_str()) {
            findings.push(finding(
                "status",
                Some(number),
                "the status must be proposed, accepted, deprecated or superseded",
            ));
        }
    }
    findings
}

fn madr_file_name(name: &str) -> bool {
    let Some(stem) = name.strip_suffix(".md") else {
        return false;
    };
    let bytes = stem.as_bytes();
    bytes.len() > 5
        && bytes[..4].iter().all(u8::is_ascii_digit)
        && bytes[4] == b'-'
        && bytes[5..]
            .iter()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || *b == b'-')
}

fn madr(text: &str, path: Option<&str>) -> Vec<Finding> {
    let doc = Markdown::parse(text);
    let mut findings = Vec::new();
    if doc.title().is_none() {
        findings.push(finding("title", None, "no `# ` title"));
    }
    let bodies = doc.required(
        &[
            "Context and Problem Statement",
            "Considered Options",
            "Decision Outcome",
        ],
        &mut findings,
    );
    if let Some(Some(options)) = bodies.get(1)
        && !options
            .iter()
            .any(|(_, line)| !line.trim_start().starts_with('>') && strip_marker(line).1)
    {
        findings.push(finding(
            "options",
            Some(options[0].0),
            "Considered Options lists no option as a list item",
        ));
    }
    if let Some(path) = path
        && !madr_file_name(file_name(path))
    {
        findings.push(finding(
            "file-name",
            None,
            format!(
                "`{}` must be named NNNN-title-with-dashes.md",
                file_name(path)
            ),
        ));
    }
    findings
}

// Gherkin

struct Scenario<'a> {
    line: usize,
    outline: bool,
    steps: usize,
    placeholders: Vec<&'a str>,
    examples: Vec<Examples<'a>>,
}

struct Examples<'a> {
    line: usize,
    header: Option<Vec<&'a str>>,
}

#[derive(Clone, Copy)]
enum Block {
    Other,
    Scenario,
    Examples,
}

const STEPS: &[&str] = &["Given ", "When ", "Then ", "And ", "But ", "* "];

fn keyword(line: &str, word: &str) -> bool {
    line.strip_prefix(word)
        .is_some_and(|rest| rest.starts_with(':'))
}

/// Cells of a table row; `\|` is an escaped pipe inside a cell.
fn cells(row: &str) -> Vec<&str> {
    let mut cells = Vec::new();
    let mut start = None;
    let mut escaped = false;
    for (index, c) in row.char_indices() {
        if escaped {
            escaped = false;
        } else if c == '\\' {
            escaped = true;
        } else if c == '|' {
            if let Some(start) = start {
                cells.push(row[start..index].trim());
            }
            start = Some(index + 1);
        }
    }
    cells
}

fn placeholders<'a>(line: &'a str, names: &mut Vec<&'a str>) {
    let mut rest = line;
    while let Some(open) = rest.find('<') {
        let after = &rest[open + 1..];
        let Some(close) = after.find(['<', '>']) else {
            break;
        };
        let name = &after[..close];
        if after[close..].starts_with('>')
            && !name.is_empty()
            && name.trim() == name
            && !names.contains(&name)
        {
            names.push(name);
        }
        rest = &after[close..];
    }
}

fn gherkin(text: &str) -> Vec<Finding> {
    let mut findings = Vec::new();
    let mut features = 0;
    let mut scenarios: Vec<Scenario> = Vec::new();
    let mut block = Block::Other;
    let mut table: Option<usize> = None;
    let mut docstring: Option<&str> = None;
    for (index, raw) in text.lines().enumerate() {
        let number = index + 1;
        let line = raw.trim();
        if let Some(delimiter) = docstring {
            if line.starts_with(delimiter) {
                docstring = None;
            } else if matches!(block, Block::Scenario)
                && let Some(scenario) = scenarios.last_mut()
            {
                placeholders(line, &mut scenario.placeholders);
            }
            continue;
        }
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if line.starts_with('|') {
            let row = cells(line);
            let first = table.is_none();
            match table {
                Some(count) if count != row.len() => findings.push(finding(
                    "table",
                    Some(number),
                    format!(
                        "this row has {} cells; the table's first row has {count}",
                        row.len()
                    ),
                )),
                Some(_) => {}
                None => table = Some(row.len()),
            }
            match (block, scenarios.last_mut()) {
                (Block::Scenario, Some(scenario)) => {
                    placeholders(line, &mut scenario.placeholders);
                }
                (Block::Examples, Some(scenario)) => {
                    if let Some(examples) = scenario.examples.last_mut()
                        && examples.header.is_none()
                        && first
                    {
                        examples.header = Some(row);
                    }
                }
                _ => {}
            }
            continue;
        }
        table = None;
        if let Some(delimiter) = ["\"\"\"", "```"]
            .into_iter()
            .find(|delimiter| line.starts_with(*delimiter))
        {
            docstring = Some(delimiter);
            continue;
        }
        if keyword(line, "Feature") {
            features += 1;
            if features > 1 {
                findings.push(finding(
                    "feature",
                    Some(number),
                    "a file holds exactly one Feature",
                ));
            }
            block = Block::Other;
        } else if keyword(line, "Rule") || keyword(line, "Background") {
            block = Block::Other;
        } else if keyword(line, "Scenario Outline") || keyword(line, "Scenario Template") {
            scenarios.push(Scenario {
                line: number,
                outline: true,
                steps: 0,
                placeholders: Vec::new(),
                examples: Vec::new(),
            });
            block = Block::Scenario;
        } else if keyword(line, "Scenario") || keyword(line, "Example") {
            scenarios.push(Scenario {
                line: number,
                outline: false,
                steps: 0,
                placeholders: Vec::new(),
                examples: Vec::new(),
            });
            block = Block::Scenario;
        } else if keyword(line, "Examples") || keyword(line, "Scenarios") {
            if matches!(block, Block::Scenario | Block::Examples)
                && let Some(scenario) = scenarios.last_mut()
            {
                scenario.examples.push(Examples {
                    line: number,
                    header: None,
                });
                block = Block::Examples;
            }
        } else if STEPS.iter().any(|step| line.starts_with(*step))
            && matches!(block, Block::Scenario)
            && let Some(scenario) = scenarios.last_mut()
        {
            scenario.steps += 1;
            placeholders(line, &mut scenario.placeholders);
        }
    }
    if features == 0 {
        findings.push(finding("feature", None, "no `Feature:`"));
    }
    if scenarios.is_empty() {
        findings.push(finding("scenario", None, "no `Scenario:` or `Example:`"));
    }
    for scenario in &scenarios {
        if scenario.steps == 0 {
            findings.push(finding(
                "steps",
                Some(scenario.line),
                "the scenario has no Given/When/Then/And/But/* step",
            ));
        }
        if scenario.outline && scenario.examples.is_empty() {
            findings.push(finding(
                "examples",
                Some(scenario.line),
                "the Scenario Outline has no `Examples:`",
            ));
        }
        for examples in &scenario.examples {
            let Some(header) = &examples.header else {
                findings.push(finding(
                    "examples",
                    Some(examples.line),
                    "`Examples:` has no table header row",
                ));
                continue;
            };
            let missing: Vec<&str> = scenario
                .placeholders
                .iter()
                .filter(|name| !header.contains(*name))
                .copied()
                .collect();
            if !missing.is_empty() {
                findings.push(finding(
                    "examples",
                    Some(examples.line),
                    format!("the header row does not name <{}>", missing.join(">, <")),
                ));
            }
        }
    }
    findings
}

// EARS

/// Requirement text without list markers, bold or a leading identifier such as `REQ-12:`.
fn requirement(text: &str) -> String {
    let text = unbold(text);
    let (rest, _) = strip_marker(&text);
    let rest = match rest.split_once(' ') {
        Some((id, after)) if id.ends_with(':') && id.bytes().any(|b| b.is_ascii_digit()) => after,
        _ => rest,
    };
    rest.trim().to_string()
}

fn has_word(text: &str, word: &str) -> bool {
    text.split(|c: char| !c.is_alphanumeric())
        .any(|part| part.eq_ignore_ascii_case(word))
}

/// The text after a non-empty, comma-free clause and its separator.
fn after_clause<'a>(text: &'a str, separator: &str) -> Option<&'a str> {
    let comma = text.find(',')?;
    if text[..comma].trim().is_empty() {
        return None;
    }
    text[comma..].strip_prefix(separator)
}

fn ears_pattern(requirement: &str) -> bool {
    let lower = requirement.to_ascii_lowercase();
    let mut rest = lower.as_str();
    for opener in ["where ", "while "] {
        if let Some(clause) = rest.strip_prefix(opener) {
            let Some(after) = after_clause(clause, ", ") else {
                return false;
            };
            rest = after;
        }
    }
    if let Some(clause) = rest.strip_prefix("when ") {
        let Some(after) = after_clause(clause, ", ") else {
            return false;
        };
        rest = after;
    } else if let Some(clause) = rest.strip_prefix("if ") {
        let Some(after) = after_clause(clause, ", then ") else {
            return false;
        };
        rest = after;
    }
    let Some((system, response)) = rest
        .strip_prefix("the ")
        .and_then(|rest| rest.split_once(" shall "))
    else {
        return false;
    };
    !system.trim().is_empty() && !response.trim().trim_end_matches('.').trim().is_empty()
}

fn ears(text: &str) -> Vec<Finding> {
    let mut findings = Vec::new();
    for (number, line) in prose_lines(text) {
        let line = line.trim();
        if line.starts_with('#') {
            continue;
        }
        let candidates: Vec<String> = if line.starts_with('|') {
            cells(line).into_iter().map(requirement).collect()
        } else {
            vec![requirement(line)]
        };
        for candidate in candidates {
            if has_word(&candidate, "shall") && !ears_pattern(&candidate) {
                findings.push(finding(
                    "pattern",
                    Some(number),
                    "a `shall` requirement must read [Where …,] [While …,] [When …, | If …, then] the <system> shall <response>",
                ));
            }
        }
    }
    findings
}

// Connextra

fn connextra_pattern(story: &str) -> bool {
    let Some(rest) = story
        .strip_prefix("as an ")
        .or_else(|| story.strip_prefix("as a "))
    else {
        return false;
    };
    let Some((role, rest)) = rest.split_once(", i want ") else {
        return false;
    };
    let Some((goal, benefit)) = rest.split_once(" so that ") else {
        return false;
    };
    !role.trim().is_empty()
        && !goal.trim_end_matches(',').trim().is_empty()
        && !benefit.trim().is_empty()
}

/// Stories are list items or paragraphs; a story's clauses may wrap over its following lines.
/// "As a result, …" prose is left alone: only text that reaches for `I want` or `so that` is a story.
fn connextra(text: &str) -> Vec<Finding> {
    let mut items: Vec<(usize, String)> = Vec::new();
    let mut open = false;
    for (number, raw) in prose_lines(text) {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            open = false;
            continue;
        }
        let cleaned = unbold(line);
        let (content, marker) = strip_marker(&cleaned);
        if open
            && !marker
            && let Some((_, item)) = items.last_mut()
        {
            item.push(' ');
            item.push_str(content);
        } else {
            items.push((number, content.to_string()));
            open = true;
        }
    }
    items
        .into_iter()
        .filter_map(|(number, item)| {
            let story = item
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ")
                .to_ascii_lowercase();
            let mut words = story
                .split(|c: char| !c.is_alphanumeric())
                .filter(|word| !word.is_empty());
            let candidate = words.next() == Some("as")
                && matches!(words.next(), Some("a" | "an"))
                && (story.contains("i want") || story.contains("so that"));
            (candidate && !connextra_pattern(&story)).then(|| {
                finding(
                    "pattern",
                    Some(number),
                    "a story must read As a <role>, I want <goal>, so that <benefit>",
                )
            })
        })
        .collect()
}

#[cfg(test)]
mod tests;
