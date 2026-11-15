//! What a session's hooks delivered to the model: context bytes, Project Intelligence answers,
//! standards findings, empty test runs and resumes. The record bounds what is injected (an
//! answer budget per context window, no identical answer twice) and is what the session
//! summary counts. It holds counts and short hashes, never the delivered text.
//!
//! `{formatVersion: 1, context: {hooks, bytes, dropped}, answers: {delivered, bytes, repeated,
//! overBudget, window: {bytes, keys}}, findings: {raised, fixed, open: {<scope>: [key]}},
//! tests: {empty}, hooks: {failed}, resumes, refused, shown?, next?}`; `Null` is no record.
//! `hooks.failed` counts delivery hooks that failed to decide and so gave no answer. `shown` is the last
//! summary line the user was shown and `next` the next step said last.
use serde_json::{Map, Value, json};

/// Bytes of Project Intelligence answers one context window receives. A compaction starts a
/// new window: what was answered before it is no longer in the model's context.
pub const ANSWER_WINDOW_BYTES: u64 = 16_384;
/// How many delivered answers a window remembers, to keep an identical one from repeating.
const ANSWER_KEYS: usize = 64;
/// How many files with open findings, and open findings per file, the record remembers.
const OPEN_SCOPES: usize = 64;
const OPEN_KEYS: usize = 32;
/// Longest scope name (a project-relative path) kept.
const SCOPE_BYTES: usize = 512;
const SHOWN_BYTES: usize = 512;
const KEY: usize = 16;

fn count(value: &Value) -> u64 {
    value.as_u64().unwrap_or(0)
}

fn keys(value: &Value) -> Vec<String> {
    value
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|key| key.as_str().map(str::to_owned))
        .collect()
}

fn valid_count(value: Option<&Value>) -> bool {
    value.is_none_or(Value::is_u64)
}

fn valid_keys(value: Option<&Value>, most: usize) -> bool {
    value.is_none_or(|keys| {
        keys.as_array().is_some_and(|keys| {
            keys.len() <= most
                && keys
                    .iter()
                    .all(|key| key.as_str().is_some_and(|k| !k.is_empty() && k.len() <= 64))
        })
    })
}

fn section<'a>(record: &'a Value, name: &str, fields: &[&str]) -> Result<&'a Value, String> {
    let section = &record[name];
    let known = match section {
        Value::Null => true,
        Value::Object(map) => map.keys().all(|key| fields.contains(&key.as_str())),
        _ => false,
    };
    if known {
        Ok(section)
    } else {
        Err("invalid session delivery record".into())
    }
}

/// A delivery record as this module writes it; `Null` is no record.
pub fn validate(record: &Value) -> Result<(), String> {
    if record.is_null() {
        return Ok(());
    }
    let invalid = || "invalid session delivery record".to_string();
    let context = section(record, "context", &["hooks", "bytes", "dropped"])?;
    let answers = section(
        record,
        "answers",
        &["delivered", "bytes", "repeated", "overBudget", "window"],
    )?;
    let window = section(answers, "window", &["bytes", "keys"])?;
    let findings = section(record, "findings", &["raised", "fixed", "open"])?;
    let tests = section(record, "tests", &["empty"])?;
    let hooks = section(record, "hooks", &["failed"])?;
    let counts = [
        context.get("hooks"),
        context.get("bytes"),
        context.get("dropped"),
        answers.get("delivered"),
        answers.get("bytes"),
        answers.get("repeated"),
        answers.get("overBudget"),
        window.get("bytes"),
        findings.get("raised"),
        findings.get("fixed"),
        tests.get("empty"),
        hooks.get("failed"),
        record.get("resumes"),
        record.get("refused"),
    ];
    let open = match findings.get("open") {
        None => true,
        Some(Value::Object(open)) => {
            open.len() <= OPEN_SCOPES
                && open.iter().all(|(scope, keys)| {
                    scope.len() <= SCOPE_BYTES && valid_keys(Some(keys), OPEN_KEYS)
                })
        }
        Some(_) => false,
    };
    let line = |name: &str| {
        record
            .get(name)
            .is_none_or(|line| line.as_str().is_some_and(|l| l.len() <= SHOWN_BYTES))
    };
    let shown = line("shown") && line("next");
    if record["formatVersion"] != 1
        || !counts.into_iter().all(valid_count)
        || !valid_keys(window.get("keys"), ANSWER_KEYS)
        || !open
        || !shown
    {
        return Err(invalid());
    }
    Ok(())
}

/// The identity of an answer's text within a context window.
pub fn answer_key(answer: &Value) -> String {
    crate::digest(answer)[..KEY].to_string()
}

/// The identity of one finding of `kind` (one letter per kind of check) across checks: its
/// standard, rule and message, not its line, so an edit above it does not make it new.
pub fn finding_key(kind: char, standard: &Value, rule: &Value, message: &Value) -> String {
    let text = |value: &Value| value.as_str().unwrap_or_default().to_string();
    let identity = crate::identity(&[&text(standard), &text(rule), &text(message)]);
    format!("{kind}:{}", &identity[..KEY])
}

/// The failing `verdicts` (`{standard, findings: [{rule, message, …}], …}`) of a check of
/// `kind` on file `scope`, without the findings the session was told there and has not fixed
/// since: an edit that leaves them in place hears only what is new. A verdict left with no
/// finding is dropped.
pub fn unheard(record: &Value, scope: &str, kind: char, verdicts: Vec<Value>) -> Vec<Value> {
    let heard = keys(&record["findings"]["open"][scope]);
    if heard.is_empty() {
        return verdicts;
    }
    verdicts
        .into_iter()
        .filter_map(|mut verdict| {
            let standard = verdict["standard"].clone();
            let findings = verdict.get_mut("findings")?.as_array_mut()?;
            findings.retain(|finding| {
                let key = finding_key(kind, &standard, &finding["rule"], &finding["message"]);
                !heard.contains(&key)
            });
            (!findings.is_empty()).then_some(verdict)
        })
        .collect()
}

/// Whether an answer may be delivered now.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Answer {
    Deliver,
    /// The same text was already delivered in this context window.
    Repeated,
    /// The window's answer budget is spent.
    OverBudget,
}

/// Whether the window has room for no further answer, so its lookup need not run.
pub fn window_spent(record: &Value) -> bool {
    count(&record["answers"]["window"]["bytes"]) >= ANSWER_WINDOW_BYTES
}

/// Whether an answer with identity `key` and `bytes` of text may be delivered.
pub fn answer(record: &Value, key: &str, bytes: usize) -> Answer {
    let window = &record["answers"]["window"];
    if keys(&window["keys"]).iter().any(|k| k == key) {
        Answer::Repeated
    } else if count(&window["bytes"]).saturating_add(bytes as u64) > ANSWER_WINDOW_BYTES {
        Answer::OverBudget
    } else {
        Answer::Deliver
    }
}

/// One scope a check just covered: the file (or `commit`) it read, the kinds of finding it
/// looks for and the keys of those it found. A kind checked and not found is fixed.
#[derive(Debug, Clone, PartialEq)]
pub struct Checked {
    pub scope: String,
    pub kinds: String,
    pub keys: Vec<String>,
}

/// What one hook delivered.
#[derive(Debug, Default)]
pub struct Delivery<'a> {
    /// Bytes of context the host was given; zero when the hook said nothing.
    pub context_bytes: usize,
    /// Pieces left out to stay under the host's ceiling.
    pub dropped: usize,
    /// A delivered Project Intelligence answer: its key and bytes.
    pub answer: Option<(&'a str, usize)>,
    /// An answer held back, and why.
    pub held: Option<Answer>,
    /// Scopes whose checks ran and whose findings, if any, the model was shown.
    pub checked: &'a [Checked],
    /// A test run that executed no test was reported.
    pub empty_test: bool,
    /// The task or checkpoint was restated after the host compacted the context.
    pub resume: bool,
}

fn add(target: &mut Value, by: u64) {
    *target = json!(count(target).saturating_add(by));
}

/// The record after `delivery`; `None` when it does not change.
pub fn recorded(record: &Value, delivery: &Delivery) -> Option<Value> {
    let mut next = if record.is_object() {
        record.clone()
    } else {
        json!({"formatVersion": 1})
    };
    if delivery.context_bytes > 0 {
        add(&mut next["context"]["hooks"], 1);
        add(&mut next["context"]["bytes"], delivery.context_bytes as u64);
    }
    add(&mut next["context"]["dropped"], delivery.dropped as u64);
    if let Some((key, bytes)) = delivery.answer {
        add(&mut next["answers"]["delivered"], 1);
        add(&mut next["answers"]["bytes"], bytes as u64);
        add(&mut next["answers"]["window"]["bytes"], bytes as u64);
        let mut seen = keys(&next["answers"]["window"]["keys"]);
        seen.retain(|k| k != key);
        seen.push(key.to_string());
        let excess = seen.len().saturating_sub(ANSWER_KEYS);
        seen.drain(..excess);
        next["answers"]["window"]["keys"] = json!(seen);
    }
    match delivery.held {
        Some(Answer::Repeated) => add(&mut next["answers"]["repeated"], 1),
        Some(Answer::OverBudget) => add(&mut next["answers"]["overBudget"], 1),
        _ => {}
    }
    for checked in delivery.checked {
        if checked.scope.len() > SCOPE_BYTES {
            continue;
        }
        let covers = |key: &String| {
            key.chars()
                .next()
                .is_some_and(|k| checked.kinds.contains(k))
        };
        let before = keys(&next["findings"]["open"][checked.scope.as_str()]);
        let (covered, mut open): (Vec<String>, Vec<String>) = before.into_iter().partition(covers);
        let raised = checked.keys.iter().filter(|k| !covered.contains(k)).count();
        let fixed = covered.iter().filter(|k| !checked.keys.contains(k)).count();
        add(&mut next["findings"]["raised"], raised as u64);
        add(&mut next["findings"]["fixed"], fixed as u64);
        for key in &checked.keys {
            if !open.contains(key) && open.len() < OPEN_KEYS {
                open.push(key.clone());
            }
        }
        if !next["findings"]["open"].is_object() {
            next["findings"]["open"] = json!({});
        }
        let scopes = next["findings"]["open"]
            .as_object_mut()
            .expect("open findings object");
        // A scope checked again moves to the end, so the oldest is the one forgotten.
        scopes.shift_remove(&checked.scope);
        if !open.is_empty() {
            while scopes.len() >= OPEN_SCOPES {
                let Some(oldest) = scopes.keys().next().cloned() else {
                    break;
                };
                scopes.shift_remove(&oldest);
            }
            scopes.insert(checked.scope.clone(), json!(open));
        }
    }
    if delivery.empty_test {
        add(&mut next["tests"]["empty"], 1);
    }
    if delivery.resume {
        add(&mut next["resumes"], 1);
    }
    pruned(&mut next);
    (next != *record && next != json!({"formatVersion": 1})).then_some(next)
}

/// Drop zero counts and empty lists `add` and indexing created, so an unchanged record
/// compares equal and a record holds only what happened.
fn pruned(value: &mut Value) {
    if let Value::Object(map) = value {
        let empty = |value: &Value| match value {
            Value::Null => true,
            Value::Number(number) => number.as_u64() == Some(0),
            Value::Array(list) => list.is_empty(),
            Value::Object(map) => map.is_empty(),
            _ => false,
        };
        for child in map.values_mut() {
            pruned(child);
        }
        let kept: Map<String, Value> = std::mem::take(map)
            .into_iter()
            .filter(|(_, child)| !empty(child))
            .collect();
        *map = kept;
    }
}

/// The record when the context is compacted: the answer window starts afresh. `None` when
/// there is no window to reset.
pub fn new_window(record: &Value) -> Option<Value> {
    let window = record["answers"].get("window")?;
    if !window.is_object() {
        return None;
    }
    let mut next = record.clone();
    next["answers"]
        .as_object_mut()
        .expect("answers object")
        .shift_remove("window");
    pruned(&mut next);
    Some(next)
}

/// The record after the project's safety policy refused one command.
pub fn refusal(record: &Value) -> Value {
    let mut next = if record.is_object() {
        record.clone()
    } else {
        json!({"formatVersion": 1})
    };
    add(&mut next["refused"], 1);
    next
}

/// The record after a delivery hook failed to decide: counted, so a hook that gave no answer
/// is not mistaken for one with nothing to say. (A hook whose input it could not read does not
/// know its session; `craft doctor` counts those.)
pub fn hook_failure(record: &Value) -> Value {
    let mut next = if record.is_object() {
        record.clone()
    } else {
        json!({"formatVersion": 1})
    };
    add(&mut next["hooks"]["failed"], 1);
    next
}

fn counted(count: u64, one: &str, many: &str) -> String {
    format!("{count} {}", if count == 1 { one } else { many })
}

/// The counts the session summary shows: `{findingsRaised, findingsFixed, emptyTestRuns,
/// lookupsAnswered, resumes, commandsRefused, hookFailures}`.
pub fn counts(record: &Value) -> Value {
    json!({
        "findingsRaised": count(&record["findings"]["raised"]),
        "findingsFixed": count(&record["findings"]["fixed"]),
        "emptyTestRuns": count(&record["tests"]["empty"]),
        "lookupsAnswered": count(&record["answers"]["delivered"]),
        "resumes": count(&record["resumes"]),
        "commandsRefused": count(&record["refused"]),
        "hookFailures": count(&record["hooks"]["failed"]),
    })
}

/// One line on what Craft did in the session, or `None` when it did none of it.
pub fn summary(record: &Value) -> Option<String> {
    let counts = counts(record);
    let of = |name: &str| count(&counts[name]);
    let mut parts = Vec::new();
    if of("findingsRaised") > 0 {
        parts.push(format!(
            "{} raised, {} fixed",
            counted(
                of("findingsRaised"),
                "standards finding",
                "standards findings"
            ),
            of("findingsFixed")
        ));
    }
    if of("emptyTestRuns") > 0 {
        parts.push(format!(
            "{} caught",
            counted(
                of("emptyTestRuns"),
                "test run that ran no test",
                "test runs that ran no test"
            )
        ));
    }
    if of("lookupsAnswered") > 0 {
        parts.push(format!(
            "{} answered from the project index",
            counted(of("lookupsAnswered"), "lookup", "lookups")
        ));
    }
    if of("resumes") > 0 {
        parts.push(format!(
            "{} restored after compaction",
            counted(of("resumes"), "context", "contexts")
        ));
    }
    if of("commandsRefused") > 0 {
        parts.push(format!(
            "{} refused by the safety policy",
            counted(of("commandsRefused"), "command", "commands")
        ));
    }
    if of("hookFailures") > 0 {
        parts.push(counted(of("hookFailures"), "hook failure", "hook failures"));
    }
    (!parts.is_empty()).then(|| format!("Craft this session: {}", parts.join(" · ")))
}

/// The summary line to show the user now and the record remembering it, or `None` when
/// there is nothing to say or the same line was the last one shown.
pub fn summary_due(record: &Value) -> Option<(String, Value)> {
    match notice_due(record, true, None)? {
        (Some(line), next) => Some((line, next)),
        (None, _) => None,
    }
}

/// The turn-end notice: the session summary (with `summarize`) when it changed since it was
/// last shown, and `next` when it is not the next step said last, in one line; and the record
/// remembering both. A next step that no longer applies is forgotten without a word, so it
/// is said again when it returns. `None` when the record stays as it is.
pub fn notice_due(
    record: &Value,
    summarize: bool,
    next: Option<&str>,
) -> Option<(Option<String>, Value)> {
    let fits = |line: &str| line.len() <= SHOWN_BYTES;
    let summary = summarize
        .then(|| summary(record))
        .flatten()
        .filter(|line| fits(line) && record["shown"] != line.as_str());
    let next = next.filter(|next| fits(next));
    let said = next.filter(|next| record["next"] != *next);
    let forget = next.is_none() && record.get("next").is_some();
    if summary.is_none() && said.is_none() && !forget {
        return None;
    }
    let mut updated = if record.is_null() {
        json!({"formatVersion": 1})
    } else {
        record.clone()
    };
    if let Some(line) = &summary {
        updated["shown"] = json!(line);
    }
    match next {
        Some(next) => updated["next"] = json!(next),
        None => {
            updated
                .as_object_mut()
                .map(|fields| fields.shift_remove("next"));
        }
    }
    let line = match (summary, said) {
        (Some(summary), Some(next)) => Some(format!("{summary} · next: {next}")),
        (Some(summary), None) => Some(summary),
        (None, Some(next)) => Some(format!("Craft next: {next}")),
        (None, None) => None,
    };
    Some((line, updated))
}

#[cfg(test)]
mod tests;
