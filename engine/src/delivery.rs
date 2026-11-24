//! Context delivery at host lifecycle boundaries: the work a lifecycle fact asks for, the
//! context a hook delivers and when an identical routing hint stays quiet. Lifecycle facts
//! come from the application; host event names and wire formats never enter this policy.
use crate::routing::catalog::SOURCE;
use crate::values::lower_hex;
use serde_json::{Value, json};

/// Host-neutral facts about one hook event.
#[derive(Default)]
pub struct Facts<'a> {
    pub session: bool,
    /// The host can show this event's context to the model; routing hints need it.
    pub context_supported: bool,
    /// An edit at the one tool hook that delivers its context: before the tool runs when the
    /// host shows context there, otherwise after it.
    pub edit: bool,
    /// The intent the session's requests established, from its intent record.
    pub established: Option<&'a str>,
    /// The edited file was already routed under the established `implement` intent.
    pub file_routed: bool,
    /// The submitted prompt, for prompt events only.
    pub prompt: Option<&'a str>,
    pub started: bool,
    pub context_changing: bool,
    pub context_restored: bool,
    pub turn_finished: bool,
    /// Context changed and no restored-context event delivered the task since, at a prompt
    /// or tool event: hosts without one get the resume at the next event that can show it.
    pub restore_pending: bool,
}

#[derive(Debug, PartialEq)]
pub struct Plan {
    pub intent: Option<&'static str>,
    /// What a prompt asks to produce when it decides the workflow: `spec-audit`, `adr`.
    pub artifact: Option<&'static str>,
    /// The intent this event establishes for the session: a prompt's request, or the
    /// `implement` an edit routes when no request established one.
    pub establish: Option<&'static str>,
    pub invalidate_hint: bool,
    pub services: bool,
    pub save_checkpoint: bool,
    pub recover_checkpoint: bool,
    pub read_checkpoint: bool,
    /// Offer unfinished tasks of earlier sessions to a session without one.
    pub offer_handoff: bool,
    /// Tell a started or restored session its identity, which task commands take. Part of
    /// the task records feature: off with checkpoints, so a fully manual policy says nothing.
    pub identify_session: bool,
    /// Remember that context is changing, so the task resumes at the next chance.
    pub mark_restore: bool,
}

/// What a lifecycle fact asks for under the effective `settings`. Checkpoints are saved at
/// turn completion and before context changes, never after each tool call.
///
/// Whether hooks record the test commands the host runs: unless `routing.testRuns` is false.
pub fn test_runs(settings: &Value) -> bool {
    settings["routing"]["testRuns"] != false
}

/// Whether the user is shown the session summary line when a turn ends: unless
/// `routing.sessionSummary` is false.
pub fn session_summary(settings: &Value) -> bool {
    settings["routing"]["sessionSummary"] != false
}

/// Whether a session start names the standards an undeclared project's own files support:
/// unless `routing.conventionProposals` is false.
pub fn convention_proposals(settings: &Value) -> bool {
    settings["routing"]["conventionProposals"] != false
}

/// Whether hooks keep the session's intent record and its pending-resume mark: unless
/// `routing.sessionRecords` is false.
pub fn session_records(settings: &Value) -> bool {
    settings["routing"]["sessionRecords"] != false
}

/// A prompt's request establishes the session's intent even where the host cannot show its
/// hint. An edit is a side effect of that intent, routed once per file: under the story,
/// exploration or planning work that writes it when the project declares standards, so the
/// edit hears the gates its prompt left out, and otherwise as `implement` when no request
/// established another intent.
pub fn plan(facts: &Facts, settings: &Value) -> Plan {
    let request = facts.prompt.and_then(prompt::request);
    // Document work routes an edit only for the standards it could name.
    let standards = settings["standards"]
        .as_array()
        .is_some_and(|declared| !declared.is_empty())
        || settings["anchors"]["defaults"] == true;
    let edited = match facts.established {
        None => Some("implement"),
        Some(_) if facts.file_routed => None,
        Some("implement") => Some("implement"),
        Some(_) if !standards => None,
        Some("story") => Some("story"),
        Some("explore") => Some("explore"),
        Some("plan") => Some("plan"),
        Some(_) => None,
    }
    .filter(|_| facts.edit && settings["routing"]["editHints"] == true);
    let edit = edited.is_some();
    let (intent, artifact) = if !facts.context_supported {
        (None, None)
    } else if let Some(intent) = edited {
        (Some(intent), None)
    } else {
        // Turning prompt hints off keeps the request's intent as session state, unshown.
        request
            .as_ref()
            .filter(|_| settings["routing"]["promptHints"] != false)
            .map_or((None, None), |request| {
                (Some(request.intent), request.artifact)
            })
    };
    let establish = match (&request, intent) {
        _ if !facts.session => None,
        (Some(request), _) => Some(request.intent),
        (None, Some(intent)) if edit => Some(intent),
        _ => None,
    };
    let checkpoint = facts.session && settings["checkpoint"]["enabled"] == true;
    let services = settings["services"]
        .as_object()
        .is_some_and(|s| !s.is_empty());
    Plan {
        intent,
        artifact,
        establish,
        invalidate_hint: facts.session
            && settings["routing"]["dedupeHints"] == true
            && (facts.started || facts.context_changing || facts.context_restored),
        services: facts.started && services,
        save_checkpoint: checkpoint && (facts.turn_finished || facts.context_changing),
        recover_checkpoint: checkpoint
            && (facts.started
                || facts.context_restored
                || (facts.restore_pending && facts.context_supported)),
        read_checkpoint: checkpoint && facts.context_changing,
        offer_handoff: checkpoint && facts.started,
        identify_session: checkpoint && (facts.started || facts.context_restored),
        mark_restore: checkpoint && facts.context_changing,
    }
}

/// The intent a prompt's own words ask for, whether or not its hint is shown.
pub fn requested_intent(prompt: &str) -> Option<&'static str> {
    prompt::request(prompt).map(|request| request.intent)
}

/// The hook decision: a bound task's resume first, then routing, a saved checkpoint,
/// configured services, or the continuation reminder before context changes.
pub fn decision(
    routing: Option<&Value>,
    checkpoint: Option<&Value>,
    services: Option<&Value>,
    preserve_continuation: bool,
) -> Value {
    let saved = checkpoint.filter(|c| !matches!(c["saved"], Value::Null | Value::Bool(false)));
    // A bound task resumes from its records: what it is, where it stands, what was decided.
    let resume = checkpoint
        .and_then(|c| c.get("resume"))
        .filter(|r| r.is_object());
    let context = if let Some(resume) = resume {
        json!({"kind": "resume", "data": resume})
    } else if let Some(routing) =
        routing.filter(|routing| informative(&shown(&json!({"kind": "routing", "data": routing}))))
    {
        json!({"kind": "routing", "data": routing})
    } else if let Some(checkpoint) = saved {
        json!({"kind": "checkpoint", "data": checkpoint})
    } else if let Some(services) = services {
        json!({"kind": "services", "data": services["services"]})
    } else if preserve_continuation {
        json!({"kind": "preserve"})
    } else {
        return json!({"decision": "allow"});
    };
    json!({"decision": "allow", "context": context})
}

/// The lookup a locate route can run now: the runtime that answered the hook and the
/// project root, then one question. Naming only the skill left the command behind a
/// `<package-root>` placeholder, and models searched with shell tools instead.
pub fn lookup(intent: &str, runtime: Option<&str>, root: &str) -> Option<Value> {
    let runtime = runtime.filter(|runtime| intent == "locate" && !runtime.is_empty())?;
    Some(json!({
        "run": [runtime, "locate", "--root", root],
        "ask": "one of --callers <name>, --declaration <name>, --imports <target>, --section <heading>, --outline <document>, --mentions <name-or-path>, --decisions <title-word or *>, --key <dotted.key>, or --term <text> (repeatable); --path <dir> scopes it and is required outside Git",
    }))
}

/// Whether a route carries anything for the model to use. A lookup route without a verified
/// answer only points at a command: models that followed it spent one or two turns more than
/// a plain search, so it stays silent and the model searches as it would without Craft.
///
/// A change request at the `prompt` names only skills to read: models opened every one
/// before a one-line fix (1.76× vanilla tokens). It stays silent unless a task is bound or it
/// reports a missing input; the session's intent still routes the first edit, whose hint
/// carries the scope's guidance where the change happens.
///
/// A route whose every skill belongs to a package the host has not installed names a workflow
/// nobody can load: it stays silent unless it still carries a standard, a task or a missing
/// input.
pub fn speaks(route: &Value, prompt: bool) -> bool {
    if route.get("answer").is_some_and(|answer| !answer.is_null()) {
        return true;
    }
    let carries = |field: &str| route[field].as_array().is_some_and(|list| !list.is_empty());
    let unserved = route["skillSources"]
        .as_array()
        .is_some_and(|sources| sources.iter().any(|s| s["status"] == "not-installed"))
        && !carries("skills");
    if unserved
        && !carries("standards")
        && !carries("missing")
        && !crate::values::truthy(&route["taskIdentity"]["task"])
    {
        return false;
    }
    match route["intent"].as_str() {
        Some("locate") => false,
        Some("implement") if prompt => {
            crate::values::truthy(&route["taskIdentity"]["task"])
                || route["missing"].as_array().is_some_and(|m| !m.is_empty())
        }
        _ => true,
    }
}

/// A hook that could not be evaluated: shell policy fails closed, other hooks continue
/// and say why their context is unavailable.
pub fn failure(enforcement: bool, message: &str) -> Value {
    if enforcement {
        json!({"decision": "deny", "reason": format!("Policy could not be evaluated: {message}")})
    } else {
        json!({"decision": "allow", "context": {"kind": "unavailable", "data": message}})
    }
}

/// How long an emitted routing hint suppresses an identical one.
pub const HINT_TTL_MS: i64 = 15 * 60 * 1000;

/// A routing hint record `{formatVersion: 1, key, emittedAt}`; `Null` is no record.
pub fn validate_hint(state: &Value) -> Result<(), String> {
    if state.is_null() {
        return Ok(());
    }
    let key = match state.get("key") {
        Some(Value::Null) => true,
        Some(Value::String(key)) => lower_hex(key, 64),
        _ => false,
    };
    if state["formatVersion"] != 1 || !key || !state["emittedAt"].is_number() {
        return Err("invalid routing hint state".into());
    }
    Ok(())
}

/// The part of a routing context a host is shown: what the model can act on. That is the
/// project's own skills (its workflow skill as the next step), the standards, references and
/// answers, but not the package's skill names, which models did not open even as the one
/// next step, nor the default `main` execution. The task identity shows only once a task is
/// bound (the session line already names the session). Other contexts are shown whole.
pub fn shown(context: &Value) -> Value {
    if context["kind"] != "routing" {
        return context.clone();
    }
    let data = &context["data"];
    let carries = |value: &Value| match value {
        Value::Array(list) => !list.is_empty(),
        Value::Object(map) => !map.is_empty(),
        other => crate::values::truthy(other),
    };
    let mut hint = serde_json::Map::new();
    for key in [
        "intent",
        "execution",
        "next",
        "skills",
        "answer",
        "lookup",
        "references",
        "modules",
        "standards",
        "missing",
        "taskBoundary",
    ] {
        let value = match key {
            "execution" if data[key]["mode"] == "main" => continue,
            "next" if !project_skill(&data[key]) => continue,
            "skills" => json!(project_skills(data)),
            _ => data[key].clone(),
        };
        if carries(&value) {
            hint.insert(key.into(), value);
        }
    }
    if crate::values::truthy(&data["taskIdentity"]["task"]) {
        hint.insert("taskIdentity".into(), data["taskIdentity"].clone());
    }
    json!({"kind": "routing", "data": hint})
}

/// The project's own skills a route selected, other than its next step.
fn project_skills(route: &Value) -> Vec<&Value> {
    route["skills"]
        .as_array()
        .map_or(&[][..], Vec::as_slice)
        .iter()
        .filter(|skill| project_skill(skill) && **skill != route["next"])
        .collect()
}

/// Whether a skill name is one of the project's own skills rather than a package's.
fn project_skill(skill: &Value) -> bool {
    skill
        .as_str()
        .and_then(|s| s.strip_prefix(crate::skills::SOURCE))
        .is_some_and(|rest| rest.starts_with(':'))
}

/// Whether a shown hint says more than its intent; one that does not stays unsent.
pub fn informative(hint: &Value) -> bool {
    hint["data"]
        .as_object()
        .is_some_and(|data| data.keys().any(|key| key != "intent"))
}

/// A shown hint as lines to act on: the next step and any answer, the project's skills, each
/// standard with what keeps it, then references and the structured rest. `skill` names a skill the way
/// the host loads it.
pub fn hint_lines(hint: &Value, skill: &dyn Fn(&str) -> String) -> String {
    let data = &hint["data"];
    let mut lines = Vec::new();
    if let Some(intent) = data["intent"].as_str() {
        lines.push(format!("intent: {}", one_line(intent)));
    }
    if let Some(next) = data["next"].as_str() {
        lines.push(format!("next: open {}", one_line(&skill(next))));
    }
    // An answer comes before anything a project's configuration can lengthen: a hint over
    // the host's ceiling is cut from its end, and the answer is what the request asked for.
    for key in ["answer", "lookup"] {
        if let Some(value) = data.get(key) {
            lines.push(format!("{key}: {value}"));
        }
    }
    let names = |field: &str| -> Vec<String> {
        data[field]
            .as_array()
            .map_or(&[][..], Vec::as_slice)
            .iter()
            .filter_map(Value::as_str)
            .map(|name| {
                one_line(&if field == "skills" {
                    skill(name)
                } else {
                    name.to_string()
                })
            })
            .collect()
    };
    let project = names("skills");
    if !project.is_empty() {
        lines.push(format!("project skills: {}", project.join(", ")));
    }
    if let Some(standards) = data["standards"].as_array() {
        lines.push("standards:".into());
        for standard in standards {
            lines.extend(standard_lines(standard));
        }
    }
    let references = names("references");
    if !references.is_empty() {
        lines.push(format!("read: {}", references.join(", ")));
    }
    let mut rest = serde_json::Map::new();
    for key in [
        "execution",
        "modules",
        "missing",
        "taskBoundary",
        "taskIdentity",
    ] {
        if let Some(value) = data.get(key) {
            rest.insert(key.into(), value.clone());
        }
    }
    if !rest.is_empty() {
        lines.push(format!("data: {}", Value::Object(rest)));
    }
    lines.join("\n")
}

/// A value as part of one hint line: a line break or control character in configured text
/// cannot start a line of its own.
fn one_line(text: &str) -> String {
    text.chars()
        .map(|c| {
            if c.is_control() || c == '\u{2028}' || c == '\u{2029}' {
                ' '
            } else {
                c
            }
        })
        .collect()
}

/// When `craft check` runs a declared command, in words.
const STAGES: [(&str, &str); 4] = [
    ("step", "at each step"),
    ("commit", "before a commit"),
    ("pr", "before a pull request"),
    ("ci", "in CI"),
];

/// One standard as the item to act on: the command and the `craft check` stage that runs
/// it, the anchor its check holds the work to, or the checklist a review keeps.
fn standard_lines(standard: &Value) -> Vec<String> {
    let id = one_line(standard["id"].as_str().unwrap_or_default());
    let summary = match (standard["summary"].as_str(), standard["file"].as_str()) {
        (Some(summary), Some(file)) => format!("{summary} (see {file})"),
        (Some(summary), None) => summary.to_string(),
        (None, Some(file)) => format!("see {file}"),
        (None, None) => String::new(),
    };
    let summary = one_line(&summary);
    let level = standard["level"].as_str().unwrap_or("advise");
    let mut head = format!("- {id}");
    if let Some(run) = standard.get("run") {
        let command = std::iter::once(&run["command"])
            .chain(run["args"].as_array().map_or(&[][..], Vec::as_slice))
            .filter_map(Value::as_str)
            .collect::<Vec<_>>()
            .join(" ");
        let command = one_line(&command);
        let stage = one_line(
            run["stage"]
                .as_str()
                .unwrap_or(crate::checks::DEFAULT_STAGE),
        );
        let when = STAGES
            .iter()
            .find(|(name, _)| *name == stage)
            .map_or(stage.as_str(), |(_, words)| words);
        head.push_str(&format!(
            ": run `{command}` {when} (`craft check --stage {stage}`)"
        ));
    } else if let Some(anchor) = standard["anchor"].as_str().filter(|_| level == "check") {
        head.push_str(&format!(": {summary} (checked: {})", one_line(anchor)));
    } else if level == "review" {
        head.push_str(&format!(" (review): {summary}"));
    } else {
        head.push_str(&format!(": {summary}"));
    }
    let mut lines = vec![head];
    for item in standard["checklist"]
        .as_array()
        .map_or(&[][..], Vec::as_slice)
    {
        if let Some(item) = item.as_str() {
            lines.push(format!("  - {}", one_line(item)));
        }
    }
    lines
}

/// The identity of a routing hint: the routing rules, the invalidation `generation`, the
/// effective settings and what the host would be shown, so a hint that reads the same is
/// not sent twice even when the route behind it (files, reasons) differs.
pub fn hint_key(generation: Option<&str>, settings: &Value, context: &Value) -> String {
    let rules = [env!("CARGO_PKG_VERSION"), &crate::hash(SOURCE.as_bytes())];
    crate::digest(
        &json!({"rules": rules, "generation": generation, "settings": settings, "context": shown(context)}),
    )
}

/// Whether `previous` was emitted for the same `key` within the reuse window.
pub fn hint_valid(previous: &Value, key: &str, now: f64) -> bool {
    let Some(emitted) = previous["emittedAt"].as_f64() else {
        return false;
    };
    previous["key"] == key && now >= emitted && now - emitted < HINT_TTL_MS as f64
}

/// How many routed files an intent record keeps; an older file routes again.
pub const INTENT_FILES: usize = 32;

/// How many planning artifacts an intent record keeps; the oldest is dropped first.
pub const PLANNING_ARTIFACTS: usize = 8;

fn planning_entry(entry: &Value) -> bool {
    entry["path"].as_str().is_some_and(|p| !p.is_empty())
        && entry["contentHash"]
            .as_str()
            .is_some_and(|h| lower_hex(h, 64))
        && entry["revision"].as_u64().is_some_and(|r| r >= 1)
        && entry["intent"] == "plan"
}

/// A session intent record `{formatVersion: 1, intent, files, since?, planning?}`; `Null` is
/// no record. `since` is when the intent was established, in Unix milliseconds. `planning`
/// lists the artifacts written while the session planned: `{path, contentHash, revision,
/// intent: "plan"}`.
pub fn validate_intent(state: &Value) -> Result<(), String> {
    if state.is_null() {
        return Ok(());
    }
    let files = state["files"]
        .as_array()
        .is_some_and(|files| files.len() <= INTENT_FILES && files.iter().all(Value::is_string));
    let planning = match state.get("planning") {
        None => true,
        Some(Value::Array(entries)) => {
            entries.len() <= PLANNING_ARTIFACTS && entries.iter().all(planning_entry)
        }
        Some(_) => false,
    };
    let since = state.get("since").is_none_or(Value::is_u64);
    if state["formatVersion"] != 1 || !state["intent"].is_string() || !files || !planning || !since
    {
        return Err("invalid session intent state".into());
    }
    Ok(())
}

/// Whether `file` was routed under the intent `record` holds.
pub fn file_routed(record: &Value, file: &str) -> bool {
    record["files"]
        .as_array()
        .is_some_and(|files| files.iter().any(|f| f == file))
}

/// The intent record after an event establishes `intent` at `now` (Unix milliseconds): a
/// request starts it afresh, an edit adds its routed `file` and keeps when it started. `None`
/// when the record does not change.
pub fn established(record: &Value, intent: &str, file: Option<&str>, now: u64) -> Option<Value> {
    let continued = file.is_some() && record["intent"] == intent;
    let mut files: Vec<Value> = match file {
        Some(_) if record["intent"] == intent => {
            record["files"].as_array().cloned().unwrap_or_default()
        }
        Some(_) => vec![],
        None if record["intent"] == intent
            && record["files"].as_array().is_some_and(Vec::is_empty) =>
        {
            return None;
        }
        None => vec![],
    };
    if let Some(file) = file {
        if files.iter().any(|f| f == file) {
            return None;
        }
        files.push(json!(file));
        let excess = files.len().saturating_sub(INTENT_FILES);
        files.drain(..excess);
    }
    let mut next = json!({"formatVersion": 1, "intent": intent, "files": files});
    match record.get("since") {
        Some(since) if continued => next["since"] = since.clone(),
        _ if continued => {}
        _ => next["since"] = json!(now),
    }
    if let Some(planning) = record.get("planning") {
        next["planning"] = planning.clone();
    }
    Some(next)
}

/// Files a turn may have written, from a snapshot's `-z` porcelain status entries: added,
/// modified and untracked paths, never a deletion or the origin of a rename or copy. Paths are
/// relative to the repository root. At most [`INTENT_FILES`].
pub fn written_paths(status: &[Value]) -> Vec<String> {
    let mut paths = Vec::new();
    let mut origin = false;
    for entry in status.iter().filter_map(Value::as_str) {
        if std::mem::take(&mut origin) {
            continue;
        }
        let code = entry.get(..2).unwrap_or_default();
        origin = code.contains(['R', 'C']);
        match entry.get(3..) {
            Some(path) if !path.is_empty() && !code.contains('D') => paths.push(path.to_string()),
            _ => {}
        }
        if paths.len() == INTENT_FILES {
            break;
        }
    }
    paths
}

/// The intent record after `path` was written with content `hash` while the session plans:
/// the artifact becomes planning evidence, its revision rising with each new content. `None`
/// outside a planning session or when the content is unchanged. The content is never read.
pub fn planning_recorded(record: &Value, path: &str, hash: &str) -> Option<Value> {
    if record["intent"] != "plan" {
        return None;
    }
    let mut entries = record["planning"].as_array().cloned().unwrap_or_default();
    match entries.iter_mut().find(|e| e["path"] == path) {
        Some(entry) if entry["contentHash"] == hash => return None,
        Some(entry) => {
            entry["contentHash"] = json!(hash);
            entry["revision"] = json!(entry["revision"].as_u64().unwrap_or(0) + 1);
        }
        None => {
            entries
                .push(json!({"path": path, "contentHash": hash, "revision": 1, "intent": "plan"}));
            let excess = entries.len().saturating_sub(PLANNING_ARTIFACTS);
            entries.drain(..excess);
        }
    }
    let mut next = record.clone();
    next["planning"] = json!(entries);
    Some(next)
}

pub mod answer;
pub mod budget;
mod prompt;
#[cfg(test)]
mod tests;
pub mod writes;
