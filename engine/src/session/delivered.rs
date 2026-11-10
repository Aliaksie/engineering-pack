//! What a session's hooks delivered to the model: context bytes, Project Intelligence answers,
//! standards findings, empty test runs and resumes. The record bounds what is injected (an
//! answer budget per context window, no identical answer twice) and is what the session
//! summary counts. It holds counts and short hashes, never the delivered text.
//!
//! `{formatVersion: 1, context: {hooks, bytes, dropped}, answers: {delivered, bytes, repeated,
//! overBudget, window: {bytes, keys}}, findings: {raised, fixed, open: {<scope>: [key]}},
//! tests: {empty}, resumes, shown?}`; `Null` is no record.
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
        record.get("resumes"),
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
    let shown = record
        .get("shown")
        .is_none_or(|line| line.as_str().is_some_and(|l| l.len() <= SHOWN_BYTES));
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
    /// The task or checkpoint was restated after a start or a compaction.
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

fn counted(count: u64, one: &str, many: &str) -> String {
    format!("{count} {}", if count == 1 { one } else { many })
}

/// The counts the session summary shows: `{findingsRaised, findingsFixed, emptyTestRuns,
/// lookupsAnswered, resumes}`.
pub fn counts(record: &Value) -> Value {
    json!({
        "findingsRaised": count(&record["findings"]["raised"]),
        "findingsFixed": count(&record["findings"]["fixed"]),
        "emptyTestRuns": count(&record["tests"]["empty"]),
        "lookupsAnswered": count(&record["answers"]["delivered"]),
        "resumes": count(&record["resumes"]),
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
            "{} restored",
            counted(of("resumes"), "task context", "task contexts")
        ));
    }
    (!parts.is_empty()).then(|| format!("Craft this session: {}", parts.join(" · ")))
}

/// The summary line to show the user now and the record remembering it, or `None` when
/// there is nothing to say or the same line was the last one shown.
pub fn summary_due(record: &Value) -> Option<(String, Value)> {
    let line = summary(record).filter(|line| line.len() <= SHOWN_BYTES)?;
    if record["shown"] == line.as_str() {
        return None;
    }
    let mut next = record.clone();
    next["shown"] = json!(line);
    Some((line, next))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn after(record: &Value, delivery: Delivery) -> Value {
        let next = recorded(record, &delivery).expect("record changes");
        validate(&next).expect("valid record");
        next
    }

    #[test]
    fn an_answer_is_delivered_once_per_window_and_within_its_budget() {
        let answered = |record: &Value, key: &str, bytes| {
            after(
                record,
                Delivery {
                    context_bytes: bytes + 80,
                    answer: Some((key, bytes)),
                    ..Delivery::default()
                },
            )
        };
        let mut record = Value::Null;
        assert_eq!(answer(&record, "a", 1600), Answer::Deliver);
        record = answered(&record, "a", 1600);
        assert_eq!(answer(&record, "a", 1600), Answer::Repeated);
        // Ten answers of 1600 bytes fit 16 KiB; the eleventh does not.
        for index in 1..10 {
            let key = format!("k{index}");
            assert_eq!(answer(&record, &key, 1600), Answer::Deliver, "{index}");
            record = answered(&record, &key, 1600);
        }
        assert_eq!(answer(&record, "new", 1600), Answer::OverBudget);
        assert_eq!(answer(&record, "small", 300), Answer::Deliver);
        assert!(!window_spent(&record));
        record = answered(&record, "small", 384);
        assert!(window_spent(&record));
        // Held answers are counted, not delivered.
        let held = after(
            &record,
            Delivery {
                held: Some(Answer::OverBudget),
                ..Delivery::default()
            },
        );
        assert_eq!(held["answers"]["overBudget"], 1);
        assert_eq!(held["answers"]["delivered"], 11);
        // A compaction starts a new window; session totals stay.
        let compacted = new_window(&held).expect("window reset");
        validate(&compacted).unwrap();
        assert_eq!(answer(&compacted, "a", 1600), Answer::Deliver);
        assert!(!window_spent(&compacted));
        assert_eq!(compacted["answers"]["delivered"], 11);
        assert_eq!(compacted["answers"]["bytes"], 16_384);
        assert_eq!(new_window(&compacted), None);
    }

    #[test]
    fn a_window_remembers_a_bounded_number_of_answers() {
        let mut record = Value::Null;
        for index in 0..ANSWER_KEYS + 3 {
            let key = format!("k{index}");
            let delivery = Delivery {
                answer: Some((&key, 1)),
                ..Delivery::default()
            };
            record = after(&record, delivery);
        }
        let kept = keys(&record["answers"]["window"]["keys"]);
        assert_eq!(kept.len(), ANSWER_KEYS);
        assert_eq!(kept[0], "k3");
    }

    fn checked(scope: &str, kinds: &str, keys: &[&str]) -> Checked {
        Checked {
            scope: scope.into(),
            kinds: kinds.into(),
            keys: keys.iter().map(|k| k.to_string()).collect(),
        }
    }

    #[test]
    fn a_finding_is_raised_once_and_fixed_when_its_check_no_longer_finds_it() {
        let check = |record: &Value, scopes: &[Checked]| {
            recorded(
                record,
                &Delivery {
                    checked: scopes,
                    ..Delivery::default()
                },
            )
        };
        let first = [checked("src/a.rs", "ad", &["a:1", "a:2", "d:1"])];
        let record = check(&Value::Null, &first).unwrap();
        assert_eq!(record["findings"]["raised"], 3);
        // The same findings on the next edit are not raised again.
        assert_eq!(check(&record, &first), None);
        // The stop checks dependencies and markers only: anchor findings stay open.
        let stop = [checked("src/a.rs", "dm", &["m:1"])];
        let record = check(&record, &stop).unwrap();
        assert_eq!(
            (&record["findings"]["raised"], &record["findings"]["fixed"]),
            (&json!(4), &json!(1))
        );
        assert_eq!(
            record["findings"]["open"]["src/a.rs"],
            json!(["a:1", "a:2", "m:1"])
        );
        // A clean check of every kind closes the file.
        let clean = [checked("src/a.rs", "adm", &[])];
        let record = check(&record, &clean).unwrap();
        assert_eq!(record["findings"]["fixed"], 4);
        assert!(record["findings"].get("open").is_none());
        validate(&record).unwrap();
        // A clean check of a file with nothing open records nothing.
        assert_eq!(check(&Value::Null, &clean), None);
    }

    #[test]
    fn the_summary_names_only_what_happened_and_is_due_once() {
        assert_eq!(summary(&Value::Null), None);
        assert_eq!(summary_due(&json!({"formatVersion": 1})), None);
        // Context bytes alone are nothing a user asked about.
        let quiet = after(
            &Value::Null,
            Delivery {
                context_bytes: 200,
                ..Delivery::default()
            },
        );
        assert_eq!(summary(&quiet), None);
        let scopes = [checked("a.rs", "a", &["a:1", "a:2"])];
        let record = after(
            &quiet,
            Delivery {
                checked: &scopes,
                empty_test: true,
                answer: Some(("k", 100)),
                resume: true,
                ..Delivery::default()
            },
        );
        let (line, shown) = summary_due(&record).unwrap();
        assert_eq!(
            line,
            "Craft this session: 2 standards findings raised, 0 fixed · 1 test run that ran no test caught · 1 lookup answered from the project index · 1 task context restored"
        );
        validate(&shown).unwrap();
        assert_eq!(summary_due(&shown), None);
        // A change makes the line due again.
        let fixed = [checked("a.rs", "a", &["a:2"])];
        let record = after(
            &shown,
            Delivery {
                checked: &fixed,
                ..Delivery::default()
            },
        );
        assert!(summary_due(&record).unwrap().0.contains("raised, 1 fixed"));
    }

    #[test]
    fn records_of_another_shape_are_refused() {
        validate(&Value::Null).unwrap();
        for record in [
            json!({}),
            json!({"formatVersion": 2}),
            json!({"formatVersion": 1, "resumes": -1}),
            json!({"formatVersion": 1, "answers": {"window": {"keys": [1]}}}),
            json!({"formatVersion": 1, "findings": {"open": []}}),
            json!({"formatVersion": 1, "context": {"text": "x"}}),
            json!({"formatVersion": 1, "shown": 1}),
        ] {
            assert!(validate(&record).is_err(), "{record}");
        }
    }
}
