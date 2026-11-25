//! The acceptance criteria an issue states, and the engineering contract a task that
//! implements the issue carries for them: every role reads the same criterion IDs.
use crate::values::{js_length, line_text};
use serde_json::{Value, json};

/// The most criteria one contract holds; later ones are counted, not kept.
pub const MAX: usize = 32;
/// The longest criterion text a contract holds, in UTF-16 units.
pub const LENGTH: usize = 512;

/// The criteria of an issue body and how many it states that a contract cannot hold.
#[derive(Debug, Default, PartialEq)]
pub struct Stated {
    pub criteria: Vec<String>,
    /// Criteria past [`MAX`] or longer than [`LENGTH`]: reported, never cut short.
    pub omitted: usize,
}

/// The list under the body's first acceptance heading (`Acceptance:`, `## Acceptance
/// criteria`, `**Acceptance criteria**`): one criterion per item, checkbox markers removed,
/// indented continuation lines and sub-items joined to their item. The list ends at the
/// first line that is neither an item nor indented under one; a body with no such heading
/// states none.
pub fn stated(body: &str) -> Stated {
    let mut lines = body.lines().skip_while(|line| !heading(line)).skip(1);
    let mut items: Vec<String> = Vec::new();
    let mut blank = false;
    for line in lines.by_ref() {
        if line.trim().is_empty() {
            blank = true;
            continue;
        }
        let indented = line.starts_with([' ', '\t']);
        let deep = line.starts_with('\t') || line.starts_with("  ");
        match item(line) {
            Some(text) if !deep || items.is_empty() => items.push(text.to_owned()),
            _ if indented && !items.is_empty() => {
                let text = item(line).unwrap_or(line.trim());
                let last = items.last_mut().expect("an item");
                last.push(' ');
                last.push_str(text);
            }
            // Text between the heading and its list is not a criterion.
            _ if items.is_empty() && !blank && !heading_like(line) => {}
            _ => break,
        }
        blank = false;
    }
    let mut stated = Stated::default();
    for text in items {
        let text = text.split_whitespace().collect::<Vec<_>>().join(" ");
        if text.is_empty() {
            continue;
        }
        if stated.criteria.len() < MAX && line_text(&text, LENGTH) {
            stated.criteria.push(text);
        } else {
            stated.omitted += 1;
        }
    }
    stated
}

/// A line naming the acceptance section: optional `#`s, emphasis and a trailing colon.
fn heading(line: &str) -> bool {
    let name = line
        .trim()
        .trim_start_matches('#')
        .trim()
        .trim_matches(['*', '_'])
        .trim()
        .trim_end_matches(':')
        .trim_matches(['*', '_'])
        .trim()
        .to_ascii_lowercase();
    matches!(
        name.as_str(),
        "acceptance" | "acceptance criteria" | "acceptance criterion"
    )
}

fn heading_like(line: &str) -> bool {
    line.trim_start().starts_with('#')
}

/// The text of a list item line, with its bullet or number and any checkbox removed.
fn item(line: &str) -> Option<&str> {
    let trimmed = line.trim_start();
    let rest = if let Some(rest) = trimmed
        .strip_prefix("- ")
        .or_else(|| trimmed.strip_prefix("* "))
        .or_else(|| trimmed.strip_prefix("+ "))
    {
        rest
    } else {
        let digits = trimmed.bytes().take_while(u8::is_ascii_digit).count();
        let after = trimmed.get(digits..)?;
        if digits == 0 || digits > 3 {
            return None;
        }
        after
            .strip_prefix(". ")
            .or_else(|| after.strip_prefix(") "))?
    };
    let rest = rest.trim_start();
    Some(
        ["[ ] ", "[x] ", "[X] "]
            .iter()
            .find_map(|mark| rest.strip_prefix(mark))
            .unwrap_or(rest)
            .trim(),
    )
}

/// The ID a criterion carries in every view: its position in the issue's list.
pub fn criterion_id(index: usize) -> String {
    format!("ac-{}", index + 1)
}

/// The source ID and check ID the derived contract names.
pub const SOURCE: &str = "issue";
pub const CHECK: &str = "issue-check";

/// The serialized size a contract may reach, as its validation counts it.
const CONTRACT_BYTES: usize = 32 * 1024;

/// The engineering contract for the criteria a linked issue states, `None` when it states
/// none. `issue` is the linked issue: `reference`, optional `title` and `url`, `criteria`,
/// `criteriaOmitted` and `linkedAt`, the time its tracker response was read. Criteria past
/// the contract's size are left out from the end, and the contract's questions say how many
/// the issue lists that it does not hold.
pub fn contract(issue: &Value) -> Option<Value> {
    let mut criteria: Vec<&str> = issue["criteria"]
        .as_array()?
        .iter()
        .filter_map(Value::as_str)
        .take(MAX)
        .collect();
    let stated = issue["criteria"].as_array().map_or(0, Vec::len);
    let reported = issue["criteriaOmitted"].as_u64().unwrap_or(0) as usize;
    let reference = issue["url"]
        .as_str()
        .or_else(|| issue["reference"].as_str())
        .filter(|reference| line_text(reference, 1024) && !reference.is_empty())?;
    let observed = crate::timestamp::format(issue["linkedAt"].as_i64()?);
    let intent = issue["title"]
        .as_str()
        .filter(|title| !title.trim().is_empty() && js_length(title) <= 512)
        .map_or_else(
            || format!("The acceptance criteria of {reference}"),
            str::to_owned,
        );
    while !criteria.is_empty() {
        let omitted = reported + stated - criteria.len();
        let contract = built(&criteria, omitted, reference, &intent, &observed);
        let size = serde_json::to_vec(&contract).map_or(usize::MAX, |bytes| bytes.len());
        if size <= CONTRACT_BYTES {
            return Some(contract);
        }
        criteria.pop();
    }
    None
}

fn built(
    criteria: &[&str],
    omitted: usize,
    reference: &str,
    intent: &str,
    observed: &str,
) -> Value {
    let scope = json!(["."]);
    let questions: Vec<String> = (omitted > 0)
        .then(|| {
            format!(
                "{omitted} acceptance criteria the issue lists are not in this contract; read them in the issue"
            )
        })
        .into_iter()
        .collect();
    json!({
        "formatVersion": 1,
        "intent": intent,
        "exclusions": [],
        "context": {
            "formatVersion": 1,
            "scope": scope,
            "sources": [{
                "id": SOURCE, "kind": "external", "reference": reference, "identity": null,
                "observedAt": observed, "scope": scope, "provenance": "observed",
                "status": "accepted",
            }],
            "constraints": [],
            "checks": [{
                "id": CHECK,
                "description": "Check the behavior the criterion states against the issue",
                "sourceIds": [SOURCE], "scope": scope,
            }],
            "questions": questions,
            "coverage": if omitted > 0 { "incomplete" } else { "unknown" },
        },
        "criteria": criteria.iter().enumerate().map(|(index, text)| json!({
            "id": criterion_id(index), "description": text, "required": true, "scope": scope,
            "sourceIds": [SOURCE], "checkIds": [CHECK], "evidenceKind": "check",
        })).collect::<Vec<_>>(),
        "decisions": [],
        "derivation": {
            "reporter": "craft ticket --issue-data", "method": "observed", "observedAt": observed,
        },
    })
}

#[cfg(test)]
mod tests;
