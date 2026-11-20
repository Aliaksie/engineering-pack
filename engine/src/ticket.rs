//! How work that starts from a ticket is delivered: directly, plan first, or after asking the
//! ticket's open questions. The agent reads the ticket and states its facts; the decision is
//! made here from those facts and the paths the work touches, with no model call.
use serde_json::{Value, json};

/// Whether delivery offers are made: unless `routing.ticketOffer` is false.
pub fn enabled(settings: &Value) -> bool {
    settings["routing"]["ticketOffer"] != false
}

/// The ticket a prompt starts work from: an issue or merge request URL of a code host, a
/// tracker URL, or a key or number the prompt calls an issue, ticket, story or bug.
pub fn reference(prompt: &str) -> Option<String> {
    let words: Vec<&str> = prompt.split_whitespace().collect();
    for word in &words {
        let word = word.trim_matches(['<', '>', '(', ')', ',', '.', '"', '\'']);
        if url(word) {
            return Some(word.to_owned());
        }
    }
    let named = ["issue", "ticket", "story", "bug", "task"];
    words.windows(2).find_map(|pair| {
        let noun = pair[0].trim_end_matches(':').to_ascii_lowercase();
        let id = pair[1].trim_end_matches([',', '.', ':', ')']);
        (named.contains(&noun.as_str()) && (number(id) || key(id))).then(|| id.to_owned())
    })
}

/// An issue (`/issues/12`, `/-/issues/12`), a work item (`/_workitems/edit/12`) or a tracker
/// key in a URL (`/browse/SHOP-12`, `/issue/SHOP-12`).
fn url(word: &str) -> bool {
    let Some(rest) = word
        .strip_prefix("https://")
        .or_else(|| word.strip_prefix("http://"))
    else {
        return false;
    };
    let parts: Vec<&str> = rest
        .split(['?', '#'])
        .next()
        .unwrap_or_default()
        .split('/')
        .collect();
    parts.windows(2).any(|pair| match pair[0] {
        "issues" | "work_items" | "edit" => {
            pair[1].chars().all(|c| c.is_ascii_digit()) && !pair[1].is_empty()
        }
        "browse" | "issue" => key(pair[1]),
        _ => false,
    })
}

/// `#12` or `12`.
fn number(id: &str) -> bool {
    let digits = id.strip_prefix('#').unwrap_or(id);
    !digits.is_empty() && digits.len() <= 9 && digits.chars().all(|c| c.is_ascii_digit())
}

/// A tracker key: `SHOP-12`.
fn key(id: &str) -> bool {
    let Some((project, digits)) = id.split_once('-') else {
        return false;
    };
    project.len() >= 2
        && project
            .chars()
            .next()
            .is_some_and(|c| c.is_ascii_uppercase())
        && project
            .chars()
            .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit())
        && number(digits)
}

/// The work plan id a ticket's planned delivery uses: `issue-145` for an issue number or a
/// URL ending in one, the lowercased key for a tracker key (`shop-12`).
pub fn plan_id(reference: &str) -> String {
    let path = reference
        .trim()
        .split(['?', '#'])
        .next()
        .unwrap_or_default();
    let last = path
        .trim_end_matches('/')
        .rsplit('/')
        .next()
        .unwrap_or_default();
    let last = match reference.trim().strip_prefix('#') {
        Some(number) => number,
        None => last,
    };
    let base = if !last.is_empty() && last.chars().all(|c| c.is_ascii_digit()) {
        format!("issue-{last}")
    } else {
        last.to_ascii_lowercase()
    };
    let id: String = base
        .chars()
        .map(|c| match c {
            'a'..='z' | '0'..='9' | '-' | '_' | '.' => c,
            _ => '-',
        })
        .take(64)
        .collect();
    let id = id.trim_start_matches('.');
    if id.is_empty() {
        "ticket".to_owned()
    } else {
        id.to_owned()
    }
}

/// The plan reference that links a work plan to its ticket: `link:<url>` for a URL,
/// `tracker:<key>` otherwise.
pub fn plan_reference(reference: &str) -> String {
    let reference = reference.trim();
    if reference.starts_with("https://") || reference.starts_with("http://") {
        format!("link:{reference}")
    } else {
        format!("tracker:{reference}")
    }
}

/// The contract a path holds, which other builds, services or people rely on: a published
/// schema or interface definition, a database migration, host hook wiring or CI.
pub fn contract(path: &str) -> Option<&'static str> {
    let lower = path.to_ascii_lowercase();
    let name = lower.rsplit('/').next().unwrap_or(&lower);
    let folders: Vec<&str> = lower.split('/').collect();
    let folders = &folders[..folders.len().saturating_sub(1)];
    let within = |names: &[&str]| folders.iter().any(|folder| names.contains(folder));
    if name.ends_with(".schema.json")
        || within(&["schemas", "schema"])
        || [".proto", ".graphql", ".gql", ".avsc", ".thrift"]
            .iter()
            .any(|extension| name.ends_with(extension))
        || name.starts_with("openapi.")
        || name.starts_with("swagger.")
    {
        Some("schema")
    } else if within(&["migrations", "migration", "migrate"]) {
        Some("migration")
    } else if name == "hooks.json"
        || lower.starts_with(".claude/settings")
        || lower.starts_with(".codex/")
        || lower.starts_with(".githooks/")
    {
        Some("hooks")
    } else if lower.starts_with(".github/workflows/") || name == ".gitlab-ci.yml" {
        Some("ci")
    } else {
        None
    }
}

/// What the agent states about a ticket, with what the paths it expects to touch show.
pub struct Facts<'a> {
    /// `clear`, `unclear` or `missing`.
    pub criteria: &'a str,
    pub questions: usize,
    pub labels: &'a [String],
    /// The modules the paths fall under; top-level folders where the project declares none.
    pub modules: &'a [String],
    /// Each contract the work changes: one a path holds, or one the agent names.
    pub contracts: &'a [String],
}

/// Labels that say the ticket is still being discussed, or that it needs a design first.
const ASK_LABELS: &[&str] = &[
    "question",
    "needs-info",
    "needs info",
    "needs-triage",
    "discussion",
];
const PLAN_LABELS: &[&str] = &["epic", "breaking", "breaking-change", "needs-design", "rfc"];

/// `direct`, `plan` or `ask`, with the reasons that decided it, in the user's words.
pub fn decide(facts: &Facts) -> Value {
    let label = |names: &[&str]| {
        facts
            .labels
            .iter()
            .find(|label| names.contains(&label.to_ascii_lowercase().as_str()))
            .cloned()
    };
    let plural = |count: usize, one: &str| match count {
        1 => format!("1 {one}"),
        _ => format!("{count} {one}s"),
    };
    let mut ask = Vec::new();
    if facts.questions > 0 {
        ask.push(format!(
            "{} in the ticket",
            plural(facts.questions, "open question")
        ));
    }
    if let Some(label) = label(ASK_LABELS) {
        ask.push(format!("labelled {label}"));
    }
    if !ask.is_empty() {
        return json!({"mode": "ask", "reasons": ask});
    }
    let mut plan = Vec::new();
    if facts.modules.len() > 1 {
        plan.push(plural(facts.modules.len(), "module"));
    }
    let mut kinds: Vec<&str> = facts.contracts.iter().map(String::as_str).collect();
    kinds.sort_unstable();
    kinds.dedup();
    for kind in kinds {
        plan.push(format!("{kind} change"));
    }
    match facts.criteria {
        "clear" => {}
        "missing" => plan.push("no acceptance criteria".into()),
        _ => plan.push("unclear acceptance criteria".into()),
    }
    if let Some(label) = label(PLAN_LABELS) {
        plan.push(format!("labelled {label}"));
    }
    if !plan.is_empty() {
        return json!({"mode": "plan", "reasons": plan});
    }
    let mut direct = vec![plural(facts.modules.len().max(1), "module")];
    direct.push("no contract change".into());
    direct.push("clear acceptance criteria".into());
    json!({"mode": "direct", "reasons": direct})
}

/// The line the user is shown for a decision.
pub fn line(decision: &Value) -> String {
    let reasons: Vec<&str> = decision["reasons"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .collect();
    let reasons = reasons.join(", ");
    match decision["mode"].as_str() {
        Some("plan") => format!("Craft suggests: plan → implement → review ({reasons})"),
        Some("ask") => format!("Craft suggests: answer the ticket's questions first ({reasons})"),
        _ => format!("Craft: direct delivery ({reasons})"),
    }
}

/// Whether an agent's shell command records a plan approval: it names `ticket` and
/// `--approve` as words, however it is wrapped (`craft ticket`, `npm run ticket --`). A
/// false match only labels a user's approval as the agent's report.
pub fn approval_command(command: &str) -> bool {
    let words = command.split(|c: char| {
        c.is_whitespace() || matches!(c, ';' | '&' | '|' | '(' | ')' | '\'' | '"' | '`')
    });
    let (mut ticket, mut approve) = (false, false);
    for word in words {
        ticket |= word == "ticket" || word.ends_with("/ticket");
        approve |= word == "--approve" || word.starts_with("--approve=");
    }
    ticket && approve
}

/// Who `craft ticket --approve` records as giving the approval: the user only when the
/// session's hooks are seen to run and saw no agent shell command record it; otherwise the
/// agent, whose report is its statement, not evidence that the user answered.
pub fn command_source(agent_ran: bool, hooks_seen: bool) -> &'static str {
    if hooks_seen && !agent_ran {
        "user"
    } else {
        "agent"
    }
}

/// The source an approval record names: `user`, `agent`, or `unknown` for an approval an
/// earlier build saved without one.
pub fn approval_source(approved: &Value) -> &'static str {
    match approved["source"].as_str() {
        Some("user") => "user",
        Some("agent") => "agent",
        _ => "unknown",
    }
}

/// An approval as prose names it after "since": never the user's for one the agent reported.
pub fn approved_by(source: &str) -> &'static str {
    match source {
        "user" => "the user approved it",
        "agent" => "the agent reported the user's approval",
        _ => "its approval, whose source is unknown",
    }
}

/// An approval as views show it: the revision approved, who gave it, and whether the plan
/// is still as approved (`current`). `None` without an approval.
pub fn approval_view(approved: &Value, current: Option<&str>) -> Option<Value> {
    let digest = approved["digest"].as_str()?;
    Some(json!({
        "revision": approved["revision"],
        "source": approval_source(approved),
        "current": current == Some(digest),
    }))
}

#[cfg(test)]
mod tests;
