//! Design debt a change introduces, and the one stop-time summary that reports it. Markers
//! (DD-0) are read from the lines a change adds or removes against its base revision, never
//! from the whole file, so existing debt stays quiet. The stop summary collects these, module
//! dependency findings and the declared `review` items of the changed scope, and speaks only
//! for what this session has not been told yet.
use serde_json::{Value, json};

/// The lines a unified diff adds, with their 1-based line in the new file, and the lines it
/// removes. Expects `git diff -U0` output for one file; anything else contributes nothing.
pub fn changes(diff: &str) -> (Vec<(usize, String)>, Vec<String>) {
    let mut added = Vec::new();
    let mut removed = Vec::new();
    let mut next = None;
    for line in diff.lines() {
        if let Some(header) = line.strip_prefix("@@ ") {
            // `@@ -a[,b] +c[,d] @@`: added lines start at c.
            next = header
                .split(' ')
                .find_map(|part| part.strip_prefix('+'))
                .and_then(|range| range.split(',').next())
                .and_then(|start| start.parse::<usize>().ok());
            continue;
        }
        let Some(at) = next.as_mut() else {
            continue;
        };
        if let Some(text) = line.strip_prefix('+') {
            added.push((*at, text.to_owned()));
            *at += 1;
        } else if let Some(text) = line.strip_prefix('-') {
            removed.push(text.to_owned());
        } else if !line.starts_with('\\') {
            // A context line (none with -U0) or the next file's header.
            *at += 1;
        }
    }
    (added, removed)
}

const TODO: &[&str] = &["TODO", "FIXME", "XXX", "HACK"];
const COMMENTS: &[&str] = &["//", "#", "/*", "*", "--", "<!--", ";", "%"];
const SUPPRESSIONS: &[&str] = &[
    "eslint-disable",
    "@ts-ignore",
    "@ts-nocheck",
    "@ts-expect-error",
    "# noqa",
    "#noqa",
    "# type: ignore",
    "pylint: disable",
    "#[allow(",
    "#![allow(",
    "@SuppressWarnings",
    "//nolint",
    "// nolint",
    "NOSONAR",
    "rubocop:disable",
    "#pragma warning disable",
    "@phpstan-ignore",
    "istanbul ignore",
    "c8 ignore",
    "pragma: no cover",
];
const SKIPS: &[&str] = &[
    "#[ignore]",
    "it.skip(",
    "describe.skip(",
    "test.skip(",
    "suite.skip(",
    "context.skip(",
    "xit(",
    "xdescribe(",
    "xtest(",
    "@Disabled",
    "@Ignore",
    "@pytest.mark.skip",
    "@unittest.skip",
    "t.Skip(",
    "{ skip: true }",
];

/// Whether `word` occurs in `text` with no identifier character on either side.
fn word_at(text: &str, word: &str) -> Option<usize> {
    let identifier = |c: char| c.is_ascii_alphanumeric() || c == '_';
    text.match_indices(word).map(|(at, _)| at).find(|&at| {
        let before = text[..at].chars().next_back();
        let after = text[at + word.len()..].chars().next();
        !before.is_some_and(identifier) && !after.is_some_and(identifier)
    })
}

/// A line that states an assertion: `assert…`, `expect(`, `.should`, testify `require.`.
fn asserts(line: &str) -> bool {
    let line = line.trim_start();
    if COMMENTS.iter().any(|comment| line.starts_with(comment)) {
        return false;
    }
    ["assert", "Assert", "expect(", ".should", "require."]
        .iter()
        .any(|token| line.contains(token))
}

/// Debt markers in what a change adds: a new TODO-style comment, a lint or type-check
/// suppression, a skipped or disabled test, and fewer assertions than the change removed.
/// Each finding is `{rule, line?, message}`; commented-out code is not detected.
pub fn markers(added: &[(usize, String)], removed: &[String]) -> Vec<Value> {
    let mut found = Vec::new();
    for (line, text) in added {
        let trimmed = text.trim();
        let todo = TODO.iter().find_map(|marker| {
            let at = word_at(trimmed, marker)?;
            COMMENTS
                .iter()
                .any(|comment| trimmed[..at].contains(comment))
                .then_some(*marker)
        });
        if let Some(marker) = todo {
            found.push(json!({"rule": "todo-marker", "line": line, "message": format!("adds a {marker} comment: {}", excerpt(trimmed))}));
        }
        if let Some(token) = SUPPRESSIONS.iter().find(|token| trimmed.contains(*token)) {
            found.push(json!({"rule": "suppression", "line": line, "message": format!("adds a {token} suppression: {}", excerpt(trimmed))}));
        }
        if let Some(token) = SKIPS.iter().find(|token| trimmed.contains(*token)) {
            found.push(json!({"rule": "skipped-test", "line": line, "message": format!("skips a test with {token}: {}", excerpt(trimmed))}));
        }
    }
    let lost = removed.iter().filter(|line| asserts(line)).count();
    let kept = added.iter().filter(|(_, line)| asserts(line)).count();
    if lost > kept {
        found.push(json!({
            "rule": "removed-assertion",
            "message": format!("removes {} assertion line(s) and adds {kept}", lost),
        }));
    }
    found
}

/// At most 80 characters of a line, cut at a character boundary.
fn excerpt(line: &str) -> String {
    match line.char_indices().nth(80) {
        Some((at, _)) => format!("{}…", &line[..at]),
        None => line.to_owned(),
    }
}

/// How many findings and review items one stop summary shows, and how many delivered keys a
/// session remembers.
const MAX_SHOWN: usize = 32;
pub const MAX_DELIVERED: usize = 256;

/// What a stop says, and the keys to remember as delivered once it is shown.
#[derive(Debug, PartialEq)]
pub struct Stop {
    pub summary: Value,
    pub keys: Vec<String>,
}

/// A finding's identity across stops: its file, rule and message, not its line, so an edit
/// above it does not make it new.
fn key(finding: &Value) -> String {
    format!(
        "{}\u{1f}{}\u{1f}{}",
        finding["file"].as_str().unwrap_or_default(),
        finding["rule"].as_str().unwrap_or_default(),
        finding["message"].as_str().unwrap_or_default()
    )
}

/// The stop summary over `findings` (each `{file, rule, line?, message, standard?}`) and the
/// shown `review` standards of the changed scope, or `None` when this session was already told
/// all of it. A summary lists every current finding, so a fix shows what remains, but each
/// review item only once per session: its checklist does not change between stops.
pub fn stop(findings: &[Value], review: &[Value], delivered: &[String]) -> Option<Stop> {
    let told = |key: &String| delivered.iter().any(|d| d == key);
    let finding_keys: Vec<String> = findings.iter().map(key).collect();
    let review: Vec<(&Value, String)> = review
        .iter()
        .map(|item| {
            let id = item["id"].as_str().unwrap_or_default();
            (item, format!("review\u{1f}{id}"))
        })
        .filter(|(_, key)| !told(key))
        .collect();
    if review.is_empty() && finding_keys.iter().all(told) {
        return None;
    }
    let mut summary = json!({});
    if !findings.is_empty() {
        summary["findings"] = json!(findings.iter().take(MAX_SHOWN).collect::<Vec<_>>());
        if findings.len() > MAX_SHOWN {
            summary["more"] = json!(findings.len() - MAX_SHOWN);
        }
    }
    if !review.is_empty() {
        summary["review"] = json!(
            review
                .iter()
                .take(MAX_SHOWN)
                .map(|(item, _)| *item)
                .collect::<Vec<_>>()
        );
    }
    let keys = finding_keys
        .into_iter()
        .chain(review.into_iter().map(|(_, key)| key))
        .collect();
    Some(Stop { summary, keys })
}

/// The delivered keys after a stop showed `shown`: earlier ones kept, newest last, bounded.
pub fn delivered(earlier: &[String], shown: &[String]) -> Vec<String> {
    let mut keys: Vec<String> = earlier
        .iter()
        .filter(|key| !shown.contains(key))
        .cloned()
        .collect();
    keys.extend(shown.iter().cloned());
    let excess = keys.len().saturating_sub(MAX_DELIVERED);
    keys.drain(..excess);
    keys
}

#[cfg(test)]
mod tests;
