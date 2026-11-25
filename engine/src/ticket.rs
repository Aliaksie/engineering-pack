//! How work that starts from a ticket is delivered: directly, plan first, or after asking the
//! ticket's open questions. The agent reads the ticket and states its facts; the decision is
//! made here from those facts and the paths the work touches, with no model call.
use serde_json::{Value, json};

pub mod criteria;

/// Whether delivery offers are made: unless `routing.ticketOffer` is false.
pub fn enabled(settings: &Value) -> bool {
    settings["routing"]["ticketOffer"] != false
}

/// Intents whose work writes no code: stories, decisions and plans, testing charters and
/// testing-only work, reviews and lookups.
const NO_CHANGE: [&str; 6] = ["story", "plan", "explore", "verify", "review", "locate"];

/// Whether work under the session's `intent` is a change delivered directly or by a plan:
/// any intent but one that writes no code, and none at all, which may be a change.
pub fn delivers_change(intent: Option<&str>) -> bool {
    intent.is_none_or(|intent| !NO_CHANGE.contains(&intent))
}

/// The ticket a prompt starts work from: an issue or merge request URL of a code host, a
/// tracker URL, a key or number the prompt calls an issue, ticket, story or bug, one a
/// start-work phrase names ("start on 164", "go ahead with #176"), or a `#12` no pull or merge
/// request owns.
pub fn reference(prompt: &str) -> Option<String> {
    let mut words = Vec::new();
    let mut opens = Vec::new();
    for line in prompt.lines() {
        for (at, word) in line.split_whitespace().enumerate() {
            words.push(word);
            opens.push(at == 0);
        }
    }
    for word in &words {
        let word = word.trim_matches(['<', '>', '(', ')', ',', '.', '"', '\'']);
        if url(word) {
            return Some(word.to_owned());
        }
    }
    let id = |word: &str| {
        word.trim_end_matches([',', '.', ':', ';', ')', '!', '?'])
            .to_owned()
    };
    let lower: Vec<String> = words
        .iter()
        .map(|word| {
            word.trim_matches(|c: char| !c.is_ascii_alphanumeric() && c != '#')
                .to_ascii_lowercase()
        })
        .collect();
    let named = ["issue", "ticket", "story", "bug", "task"];
    let called = (1..words.len()).find_map(|at| {
        let id = id(words[at]);
        (named.contains(&lower[at - 1].as_str()) && (number(&id) || key(&id))).then_some(id)
    });
    // After a start-work phrase, a bare number or key must end its clause: "fix 3 tests" counts
    // things and "fix UTF-8 handling" names an encoding.
    let started = (0..words.len()).find_map(|at| {
        let id = id(words[at]);
        let after = START.iter().any(|phrase| {
            at >= phrase.len()
                && lower[at - phrase.len()..at]
                    .iter()
                    .zip(phrase.iter())
                    .all(|(word, start)| word == start)
        });
        let ends = id.len() < words[at].len()
            || lower
                .get(at + 1)
                .is_none_or(|next| CLAUSE.contains(&next.as_str()));
        let item = id.starts_with('#') && number(&id) || (key(&id) || number(&id)) && ends;
        // `1.` or `2)` opening a line numbers a list item.
        let marker = opens[at]
            && words[at]
                .strip_suffix(['.', ')'])
                .is_some_and(|digits| !digits.starts_with('#') && number(digits));
        (after && item && !marker).then_some(id)
    });
    let hashed = || {
        (0..words.len()).find_map(|at| {
            let id = id(words[at]);
            let owned = at > 0 && OWNERS.contains(&lower[at - 1].as_str());
            (id.starts_with('#') && number(&id) && !owned).then_some(id)
        })
    };
    called.or(started).or_else(hashed)
}

/// Phrases that start work on the item named right after them.
const START: &[&[&str]] = &[
    &["start"],
    &["start", "on"],
    &["start", "work", "on"],
    &["started", "on"],
    &["begin"],
    &["work", "on"],
    &["working", "on"],
    &["go", "ahead", "with"],
    &["go", "with"],
    &["proceed", "with"],
    &["continue"],
    &["continue", "with"],
    &["resume"],
    &["pick", "up"],
    &["move", "on", "to"],
    &["implement"],
    &["fix"],
    &["tackle"],
    &["do"],
];

/// Words that may follow a bare number a start-work phrase names: "fix 3 tests" counts things,
/// "start on 133 and close 132" names an item.
const CLAUSE: &[&str] = &[
    "and", "then", "next", "now", "please", "first", "too", "also", "as", "from", "with",
];

/// Words whose `#12` is a pull or merge request, not a ticket.
const OWNERS: &[&str] = &["pr", "mr", "pull", "pulls", "merge", "request"];

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

/// The id a tracker gives the ticket `reference` names: the number of `#12`, `12` or a URL
/// ending in one, or a key such as `SHOP-12`, alone or ending a URL. `None` for work with no
/// ticket and for anything else.
pub fn issue_id(reference: &str) -> Option<String> {
    let reference = reference.trim();
    if work_name(reference).is_some() {
        return None;
    }
    let last = if url(reference) {
        let path = reference.split(['?', '#']).next().unwrap_or_default();
        path.trim_end_matches('/')
            .rsplit('/')
            .next()
            .unwrap_or_default()
    } else {
        reference
    };
    let id = last.strip_prefix('#').unwrap_or(last);
    (number(id) || key(id)).then(|| id.to_owned())
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
/// `tracker:<key>` otherwise, none for work with no ticket.
pub fn plan_reference(reference: &str) -> Option<String> {
    let reference = reference.trim();
    if work_name(reference).is_some() {
        None
    } else if reference.starts_with("https://") || reference.starts_with("http://") {
        Some(format!("link:{reference}"))
    } else {
        Some(format!("tracker:{reference}"))
    }
}

/// Work with no ticket is decided and recorded under `work:<name>`; its plan is `work-<name>`.
const WORK: &str = "work:";

/// The reference of work with no ticket named `name`: a short name of letters, digits, `-`,
/// `_` or `.`, so the commands that carry it need no quoting.
pub fn work_reference(name: &str) -> Option<String> {
    let valid = !name.is_empty()
        && name.len() <= 64
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'));
    valid.then(|| format!("{WORK}{name}"))
}

/// The name of work with no ticket, `None` for a ticket.
pub fn work_name(reference: &str) -> Option<&str> {
    reference.strip_prefix(WORK)
}

/// The `craft ticket` option that names `reference` again: `--work <name>` or `--ticket <ref>`.
pub fn selector(reference: &str) -> String {
    match work_name(reference) {
        Some(name) => format!("--work {name}"),
        None => format!("--ticket {reference}"),
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
    /// The status of the issue the tracker returned, when the agent handed Craft its read.
    pub status: Option<&'a str>,
}

/// Statuses that say a tracker item is finished: closed, done or decided against.
const FINISHED: &[&str] = &[
    "closed",
    "done",
    "resolved",
    "completed",
    "complete",
    "cancelled",
    "canceled",
    "won't do",
    "won't fix",
    "wontfix",
    "rejected",
    "declined",
    "duplicate",
];

/// Whether an issue's `status` says it is finished, in any case.
pub fn finished(status: &str) -> bool {
    let status = status.trim().to_lowercase().replace('\u{2019}', "'");
    FINISHED.contains(&status.as_str())
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
    // Work on a finished issue is confirmed first: it may be done, or meant for another one.
    if let Some(status) = facts.status.filter(|status| finished(status)) {
        return json!({"mode": "ask", "reasons": [format!("issue {}", status.trim())], "finished": true});
    }
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
        Some("ask") if decision["finished"] == true => {
            format!("Craft suggests: confirm the ticket is still to be done ({reasons})")
        }
        Some("ask") => format!("Craft suggests: answer the ticket's questions first ({reasons})"),
        _ => format!("Craft: direct delivery ({reasons})"),
    }
}

/// Whether an agent's shell command records a plan approval: it names `ticket` and
/// `--approve` as words, however it is wrapped (`craft ticket`, `npm run ticket --`). A
/// false match only labels a user's approval as the agent's report.
pub fn approval_command(command: &str) -> bool {
    ticket_command(command, "--approve")
}

/// Whether an agent's shell command records the user's delivery choice for a ticket
/// (`ticket --accept`), matched as [`approval_command`] matches an approval.
pub fn accept_command(command: &str) -> bool {
    ticket_command(command, "--accept")
}

/// Whether `command` names `ticket` and `flag` as words, however it is wrapped.
fn ticket_command(command: &str, flag: &str) -> bool {
    let words = command.split(|c: char| {
        c.is_whitespace() || matches!(c, ';' | '&' | '|' | '(' | ')' | '\'' | '"' | '`')
    });
    let (mut ticket, mut named) = (false, false);
    for word in words {
        ticket |= word == "ticket" || word.ends_with("/ticket");
        named |= word == flag
            || word
                .strip_prefix(flag)
                .is_some_and(|rest| rest.starts_with('='));
    }
    ticket && named
}

/// The source of an approval Craft read from the user's answer to the host's own question.
pub const HOST_OBSERVED: &str = "host-observed";
/// The option that approves a plan in the host's question.
pub const APPROVE: &str = "Approve";

/// The question that asks the user to approve revision `revision` of plan `plan`, which a
/// hook that sees the answer records as the approval of that revision.
pub fn approval_question(plan: &str, revision: impl std::fmt::Display) -> String {
    format!("Approve plan {plan} revision {revision}?")
}

/// The revision of plan `plan` the user approved in one host question, from the
/// `[{question, answer}]` pairs a hook saw: a question that starts with
/// [`approval_question`]'s words for this plan, answered with exactly [`APPROVE`] (a host may
/// mark it recommended). Another answer, a free-text one, several options or another plan
/// approve nothing.
pub fn observed_approval(answers: &Value, plan: &str) -> Option<u64> {
    let prefix = format!("Approve plan {plan} revision ");
    answers.as_array()?.iter().find_map(|pair| {
        let rest = pair["question"]
            .as_str()?
            .trim_start()
            .strip_prefix(&prefix)?;
        let digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
        if !rest[digits.len()..].starts_with('?') {
            return None;
        }
        let answer = pair["answer"].as_str()?.trim();
        let approves = answer == APPROVE || answer == format!("{APPROVE} (Recommended)");
        approves.then(|| digits.parse().ok()).flatten()
    })
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
        Some(HOST_OBSERVED) => HOST_OBSERVED,
        _ => "unknown",
    }
}

/// An approval as prose names it after "since": never the user's for one the agent reported.
pub fn approved_by(source: &str) -> &'static str {
    match source {
        "user" => "the user approved it",
        "agent" => "the agent reported the user's approval",
        HOST_OBSERVED => "the user approved it in the host's question",
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
