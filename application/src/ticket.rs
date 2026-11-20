//! Work that starts from a ticket: the delivery Craft suggests for the facts the agent states
//! about it, the step the agent takes next on this host, and the delivery the user chose,
//! which the session keeps so the turn that finishes a planned change ends with a review offer.
use crate::failure::Failure;
use crate::session::read_record;
use craft_engine::{routing, ticket};
use craft_platform::state;
use serde_json::{Value, json};
use std::path::Path;

const RECORD_BYTES: usize = 4096;
/// How many paths and labels one decision reads.
const PATHS: usize = 256;
const LABELS: usize = 32;

/// What the agent states about the ticket, from the command line.
pub struct Request<'a> {
    pub ticket: &'a str,
    pub criteria: Option<&'a str>,
    pub questions: Option<&'a str>,
    pub labels: &'a [String],
    pub paths: &'a [String],
    pub affects: &'a [String],
    pub host: Option<&'a str>,
    pub session: Option<&'a str>,
    /// The delivery the user chose: `plan` or `direct`.
    pub accept: Option<&'a str>,
    /// The user approved the ticket's work plan as it stands.
    pub approve: bool,
}

fn record_file(root: &Path, session: &str, create: bool) -> Result<std::path::PathBuf, Failure> {
    Ok(state::file(root, "ticket.json", create, Some(session))?)
}

/// The decision for a ticket, or with `accept`, the user's choice recorded for the session.
pub fn run(root: &Path, settings: &Value, request: &Request) -> Result<Value, Failure> {
    let reference = request.ticket.trim();
    if reference.is_empty() || !craft_engine::values::line_text(reference, 512) {
        return Err("ticket requires --ticket with the issue URL or key".into());
    }
    if request.approve {
        return approve(root, reference, request.session);
    }
    if let Some(choice) = request.accept {
        return accept(
            root,
            settings,
            reference,
            choice,
            request.session,
            request.host,
        );
    }
    let criteria = match request.criteria {
        Some(criteria @ ("clear" | "unclear" | "missing")) => criteria,
        _ => return Err("ticket requires --criteria clear, unclear or missing".into()),
    };
    let questions = match request.questions {
        None => 0,
        Some(count) => count
            .parse::<usize>()
            .map_err(|_| "--questions must be a whole number")?,
    };
    if request.paths.len() > PATHS || request.labels.len() > LABELS {
        return Err(format!("ticket reads at most {PATHS} paths and {LABELS} labels").into());
    }
    let modules = modules(settings, request.paths)?;
    let mut contracts: Vec<Value> = request
        .paths
        .iter()
        .filter_map(|path| Some(json!({"path": path, "kind": ticket::contract(path)?})))
        .collect();
    contracts.extend(
        request
            .affects
            .iter()
            .map(|kind| json!({"kind": kind, "stated": true})),
    );
    let kinds: Vec<String> = contracts
        .iter()
        .filter_map(|contract| contract["kind"].as_str().map(str::to_owned))
        .collect();
    let mut decision = ticket::decide(&ticket::Facts {
        criteria,
        questions,
        labels: request.labels,
        modules: &modules,
        contracts: &kinds,
    });
    let line = ticket::line(&decision);
    // The session keeps the user's choice; without one, the agent carries the review offer.
    let keep = |then: &str| {
        match request.session {
        Some(session) => {
            let host = request.host.map(|host| format!(" --host {host}")).unwrap_or_default();
            let accept = |choice| {
                format!("craft ticket --ticket {reference} --accept {choice} --session {session}{host}")
            };
            format!("{then} run `{}` or `{}`.", accept("plan"), accept("direct"))
        }
        None => "If they choose the plan, offer them a review with craft-delivery:review-code when the change is done.".to_owned(),
    }
    };
    let tool = request.host.and_then(crate::hook::question_tool);
    let next = match (decision["mode"].as_str(), tool.as_deref()) {
        (Some("plan"), Some(tool)) => format!(
            "Ask the user with {tool} before editing any file: question \"{line}. Plan first?\", options \"Plan first (Recommended)\" (plan, implement, then a review offer) and \"Implement directly\". {}",
            keep("With their answer,")
        ),
        (Some("plan"), None) => format!(
            "Tell the user this line as is and ask in your reply whether to plan first: {line}. Edit no file until they answer. {}",
            keep("With their answer,")
        ),
        (Some("ask"), _) => format!(
            "Ask the user the ticket's open questions{} and edit no file until they are answered; then decide again with `craft ticket`.",
            tool.map(|tool| format!(" with {tool}")).unwrap_or_default()
        ),
        _ => {
            let kept = request.session.map_or(String::new(), |session| {
                format!(" Run `craft ticket --ticket {reference} --accept direct --session {session}` so the session keeps the choice.")
            });
            format!("Tell the user this line as is, then implement directly: {line}.{kept}")
        }
    };
    decision["ticket"] = json!(reference);
    decision["modules"] = json!(modules);
    decision["contracts"] = json!(contracts);
    decision["line"] = json!(line);
    decision["next"] = json!(next);
    Ok(decision)
}

/// The modules `paths` fall under. Where the project declares none, each top-level folder
/// counts as one; a file at the root belongs to none.
fn modules(settings: &Value, paths: &[String]) -> Result<Vec<String>, Failure> {
    let files: Vec<Value> = paths.iter().map(|path| json!(path)).collect();
    let scope = routing::scope(settings, &files)?;
    let mut names: Vec<String> = if routing::modules(settings).is_empty() {
        scope
            .files
            .iter()
            .filter_map(|file| file.split_once('/').map(|(top, _)| top.to_owned()))
            .collect()
    } else {
        scope
            .modules
            .iter()
            .filter_map(|module| module["name"].as_str().map(str::to_owned))
            .collect()
    };
    names.sort();
    names.dedup();
    Ok(names)
}

/// Keep the user's choice for the session. A planned change is recorded as a work plan the
/// user approves before any file is edited, and ends with a review offer.
fn accept(
    root: &Path,
    settings: &Value,
    reference: &str,
    choice: &str,
    session: Option<&str>,
    host: Option<&str>,
) -> Result<Value, Failure> {
    if !matches!(choice, "plan" | "direct") {
        return Err("--accept takes plan or direct".into());
    }
    let session = session
        .filter(|session| !session.is_empty())
        .ok_or("--accept requires --session")?;
    let mut record = json!({"formatVersion": 1, "ticket": reference, "delivery": choice,
        "acceptedAt": crate::telemetry::now_ms()});
    // What the session's hooks saw before this command ran stays evidence of who approves.
    if let Ok(earlier) =
        record_file(root, session, false).and_then(|f| read_record(&f, RECORD_BYTES))
    {
        for key in [HOOKS_SEEN, AGENT_APPROVAL] {
            if let Some(seen) = earlier.get(key) {
                record[key] = seen.clone();
            }
        }
    }
    if choice == "direct" {
        state::write(
            &record_file(root, session, true)?,
            &record,
            RECORD_BYTES,
            false,
        )?;
        let next = "Implement with craft-delivery:apply-change and verify against the ticket's acceptance criteria.";
        return Ok(
            json!({"ticket": reference, "delivery": choice, "recorded": true, "next": next}),
        );
    }
    let plan = ticket::plan_id(reference);
    record["plan"] = json!(plan);
    // What the worktree holds now is not a change made while the plan waits for approval.
    crate::hook::stop_review::approval_baseline(root, session, settings)?;
    state::write(
        &record_file(root, session, true)?,
        &record,
        RECORD_BYTES,
        false,
    )?;
    let saved = crate::plan::read(root, &plan).ok().flatten();
    let write = match &saved {
        Some(saved) => format!(
            "Plan {plan} exists (revision {}): read it with `craft plan --plan {plan} --session {session}` and change it with `craft plan-update --plan {plan} --expected-revision {} --data <changes.json>` where the ticket needs it.",
            saved["revision"], saved["revision"]
        ),
        None => format!(
            "Plan with craft-delivery:plan-change and record it with `craft plan-create --plan {plan} --reference {} --goal \"<outcome>\" --data <tasks.json>`, tasks as {{\"tasks\": [{{\"id\", \"goal\", \"dependsOn\", \"acceptance\"}}]}}.",
            ticket::plan_reference(reference)
        ),
    };
    let observed = host.and_then(|host| observed_approval_tool(host, &record));
    let next = format!(
        "{write} Then {} Edits wait for that approval.",
        approval_step(
            reference,
            session,
            &plan,
            saved.as_ref().and_then(|s| s["revision"].as_u64()),
            host,
            observed.as_deref()
        )
    );
    Ok(
        json!({"ticket": reference, "delivery": choice, "recorded": true, "plan": plan, "next": next}),
    )
}

/// The host's own tool whose answer Craft records as the plan's approval in this session: the
/// planning table makes the host's path active and the session's hooks are seen to run.
/// Otherwise the approval is recorded by command.
fn observed_approval_tool(host: &str, record: &Value) -> Option<String> {
    (record[HOOKS_SEEN] == true)
        .then(|| crate::hook::host_path(host, "approval"))
        .flatten()
}

/// How the agent gets the user's approval of the plan on this host and how it is recorded:
/// from the answer to the host's own question when Craft observes it (`observed`), otherwise
/// by the agent's command. One path only, so the agent is never asked for both.
fn approval_step(
    reference: &str,
    session: &str,
    plan: &str,
    revision: Option<u64>,
    host: Option<&str>,
    observed: Option<&str>,
) -> String {
    if let Some(tool) = observed {
        let (question, named) = match revision {
            Some(revision) => (ticket::approval_question(plan, revision), ""),
            None => (
                ticket::approval_question(plan, "<revision>"),
                " with the plan's current revision",
            ),
        };
        return format!(
            "show the user the plan and ask with {tool} the question \"{question}\"{named}, options \"{}\" and \"Change the plan\", unless they already approved this revision. Craft records the approval from their answer; run no approval command. Once it is approved: {}",
            ticket::APPROVE,
            implement_step(session, plan)
        );
    }
    let asked = match host.and_then(crate::hook::question_tool) {
        Some(tool) => format!("ask the user with {tool} to approve the plan"),
        None => "ask the user in your reply to approve the plan".to_owned(),
    };
    format!(
        "show the user the plan and {asked}, unless they already approved this plan; with their approval run `craft ticket --ticket {reference} --approve --session {session}`."
    )
}

/// How the approved plan is implemented, task by task, and the review offered at the end.
fn implement_step(session: &str, plan: &str) -> String {
    format!(
        "Implement with craft-delivery:apply-change: claim each task with `craft task-start --session {session} --plan {plan} --ready` and finish it with `craft task-step --session {session} --stage verify --target \"<task>\" --approach \"<how>\" --evidence \"<check run>\" --outcome passed --result \"<what changed>\" --complete`. When the change is done, offer the user a review with craft-delivery:review-code before reporting it complete."
    )
}

/// Record the user's approval of the ticket's plan as it stands now. A plan changed later
/// needs approval again.
fn approve(root: &Path, reference: &str, session: Option<&str>) -> Result<Value, Failure> {
    let session = session
        .filter(|session| !session.is_empty())
        .ok_or("--approve requires --session")?;
    let file = record_file(root, session, false)?;
    let mut record = read_record(&file, RECORD_BYTES)?;
    if record["ticket"] != reference || record["delivery"] != "plan" {
        return Err(format!(
            "no planned delivery of {reference} in this session; run `craft ticket --ticket {reference} --accept plan --session {session}` first"
        )
        .into());
    }
    let plan = record["plan"]
        .as_str()
        .map_or_else(|| ticket::plan_id(reference), str::to_owned);
    let saved = crate::plan::read(root, &plan)?.ok_or_else(|| {
        format!("no plan {plan} yet; record it with `craft plan-create --plan {plan}` first")
    })?;
    let digest = craft_engine::plan::approval(&saved).ok_or_else(|| {
        format!("plan {plan} has no tasks to approve; add them with `craft plan-update`")
    })?;
    let next = implement_step(session, &plan);
    // The user's answer the host showed already approved the plan as it stands: an agent's
    // command does not replace it with its report. The agent's run is consumed all the same,
    // so it never labels a later approval.
    if record["approved"]["digest"] == digest.as_str()
        && ticket::approval_source(&record["approved"]) == ticket::HOST_OBSERVED
    {
        if let Some(fields) = record.as_object_mut()
            && fields.remove(AGENT_APPROVAL).is_some()
        {
            state::write(&file, &record, RECORD_BYTES, false)?;
        }
        return Ok(
            json!({"ticket": reference, "plan": plan, "approved": record["approved"], "next": next}),
        );
    }
    let source = ticket::command_source(
        record.get(AGENT_APPROVAL).is_some(),
        record[HOOKS_SEEN] == true,
    );
    if let Some(fields) = record.as_object_mut() {
        fields.remove(AGENT_APPROVAL);
    }
    record["plan"] = json!(plan);
    record["approved"] = json!({"revision": saved["revision"], "digest": digest,
        "source": source, "at": crate::telemetry::now_ms()});
    state::write(&file, &record, RECORD_BYTES, false)?;
    Ok(json!({"ticket": reference, "plan": plan, "approved": record["approved"], "next": next}))
}

/// Set on the session's ticket record once one of its hooks saw a tool call: from then on an
/// approval no agent shell command recorded is the user's own run.
const HOOKS_SEEN: &str = "hooksSeen";
/// When the session's hook saw an agent shell command record a plan approval; the approval
/// it records takes it as its source.
const AGENT_APPROVAL: &str = "agentApproval";

/// Before one of the session's tool calls runs: what its hook sees of who approves the plan
/// of a ticket delivered plan first. A session without such a ticket keeps no record.
pub(crate) fn tool_seen(root: &Path, session: &str, command: Option<&str>) -> Result<(), Failure> {
    // The agent records the user's delivery choice: the step it is told next depends on
    // whether this session's hooks run, so the record keeps that they do.
    let accepting = command.is_some_and(ticket::accept_command);
    // A session with no state yet has no ticket record.
    let Ok(file) = record_file(root, session, accepting) else {
        return Ok(());
    };
    let mut record = read_record(&file, RECORD_BYTES)?;
    if record["delivery"] != "plan" {
        if !accepting || record[HOOKS_SEEN] == true {
            return Ok(());
        }
        if record.is_null() {
            record = json!({"formatVersion": 1});
        }
        record[HOOKS_SEEN] = json!(true);
        return Ok(state::write(&file, &record, RECORD_BYTES, false)?);
    }
    let agent = command.is_some_and(ticket::approval_command);
    if record[HOOKS_SEEN] == true && !agent {
        return Ok(());
    }
    record[HOOKS_SEEN] = json!(true);
    if agent {
        record[AGENT_APPROVAL] = json!(crate::telemetry::now_ms());
    }
    Ok(state::write(&file, &record, RECORD_BYTES, false)?)
}

/// After the host's own approval question ran: the user's answer approving the revision of
/// the session's ticket plan it named is recorded as observed by the host. An answer for a
/// revision the plan has moved past, another answer or another plan records nothing.
pub(crate) fn answered(root: &Path, session: &str, answers: &Value) -> Result<(), Failure> {
    let Ok(file) = record_file(root, session, false) else {
        return Ok(());
    };
    let mut record = read_record(&file, RECORD_BYTES)?;
    let Some(plan) = record["plan"]
        .as_str()
        .filter(|_| record["delivery"] == "plan")
    else {
        return Ok(());
    };
    let Some(revision) = ticket::observed_approval(answers, plan) else {
        return Ok(());
    };
    let Some(saved) = crate::plan::read(root, plan)? else {
        return Ok(());
    };
    let digest = craft_engine::plan::approval(&saved);
    let Some(digest) = digest.filter(|_| saved["revision"].as_u64() == Some(revision)) else {
        return Ok(());
    };
    record["approved"] = json!({"revision": revision, "digest": digest,
        "source": ticket::HOST_OBSERVED, "at": crate::telemetry::now_ms()});
    Ok(state::write(&file, &record, RECORD_BYTES, false)?)
}

/// The session's approval of plan `plan` as views show it, with who gave it; `None` when the
/// session approved no such plan or its record cannot be read.
pub(crate) fn approval(root: &Path, session: &str, plan: &Value) -> Option<Value> {
    let file = record_file(root, session, false).ok()?;
    let record = read_record(&file, RECORD_BYTES).ok()?;
    if record["delivery"] != "plan" || record["plan"] != plan["id"] {
        return None;
    }
    let current = craft_engine::plan::approval(plan);
    ticket::approval_view(&record["approved"], current.as_deref())
}

/// Why file edits wait in this session and the step that lets them through.
pub(crate) struct Due {
    pub why: String,
    pub next: String,
}

/// Why file edits wait in this session, with the step that lets them through: its ticket was
/// planned and the plan is missing, has no tasks, or changed since the user approved it.
/// `None` when nothing waits, including when the record or plan cannot be read.
pub(crate) fn approval_due(root: &Path, session: &str, host: &str) -> Option<String> {
    let Due { why, next } = awaiting(root, session, host)?;
    Some(format!("Craft holds file edits: {why}. {next}"))
}

/// The plan approval the session waits for, `None` when nothing waits.
pub(crate) fn awaiting(root: &Path, session: &str, host: &str) -> Option<Due> {
    let file = record_file(root, session, false).ok()?;
    let record = read_record(&file, RECORD_BYTES).ok()?;
    if record["delivery"] != "plan" || record["plan"].is_null() {
        return None;
    }
    let reference = record["ticket"].as_str()?;
    let plan = record["plan"].as_str()?;
    let saved = crate::plan::read(root, plan).ok()?;
    let current = saved.as_ref().and_then(craft_engine::plan::approval);
    let approved = record["approved"]["digest"].as_str();
    let why = match (&saved, &current, approved) {
        (_, Some(current), Some(approved)) if current == approved => return None,
        (None, _, _) => {
            format!("the user chose to plan {reference} first and plan {plan} is not recorded yet")
        }
        (Some(_), None, _) => format!("plan {plan} for {reference} has no tasks yet"),
        (Some(_), Some(_), None) => {
            format!("the user has not approved plan {plan} for {reference} yet")
        }
        (Some(_), Some(_), Some(_)) => format!(
            "plan {plan} for {reference} changed since {}",
            ticket::approved_by(ticket::approval_source(&record["approved"]))
        ),
    };
    let observed = observed_approval_tool(host, &record);
    let revision = saved.as_ref().and_then(|s| s["revision"].as_u64());
    let next = format!(
        "Record the plan's tasks, {} Only if the user now chooses to implement directly, run `craft ticket --ticket {reference} --accept direct --session {session}` instead.",
        approval_step(
            reference,
            session,
            plan,
            revision,
            Some(host),
            observed.as_deref()
        )
    );
    Some(Due { why, next })
}

/// Whether the session's ticket is delivered by a plan, whether or not its review was offered.
pub(crate) fn plan_first(root: &Path, session: &str) -> bool {
    record_file(root, session, false)
        .and_then(|file| read_record(&file, RECORD_BYTES))
        .is_ok_and(|record| record["delivery"] == "plan")
}

/// The ticket whose delivery the session recorded, `None` without one.
pub(crate) fn recorded(root: &Path, session: &str) -> Option<String> {
    let file = record_file(root, session, false).ok()?;
    let record = read_record(&file, RECORD_BYTES).ok()?;
    record["ticket"].as_str().map(str::to_owned)
}

/// The ticket whose planned delivery the session accepted and whose review was not yet
/// offered, `None` without one.
pub(crate) fn planned(root: &Path, session: &str) -> Option<String> {
    let file = record_file(root, session, false).ok()?;
    let record = read_record(&file, RECORD_BYTES).ok()?;
    (record["delivery"] == "plan" && record["reviewOffered"] != true)
        .then(|| record["ticket"].as_str().map(str::to_owned))
        .flatten()
}

/// The ticket whose review is due: the planned change changed files, at this turn (`changed`)
/// or an earlier one, and the review was not yet offered. A turn whose stop named something
/// else first, such as a check to run, keeps it due for a later turn.
pub(crate) fn review_due(
    root: &Path,
    session: &str,
    changed: bool,
) -> Result<Option<String>, Failure> {
    let Some(ticket) = planned(root, session) else {
        return Ok(None);
    };
    let file = record_file(root, session, false)?;
    let mut record = read_record(&file, RECORD_BYTES)?;
    if record["reviewDue"] != true {
        if !changed {
            return Ok(None);
        }
        record["reviewDue"] = json!(true);
        state::write(&file, &record, RECORD_BYTES, false)?;
    }
    Ok(Some(ticket))
}

/// The review was offered to the user: it is not offered again.
pub(crate) fn review_offered(root: &Path, session: &str) -> Result<(), Failure> {
    let file = record_file(root, session, false)?;
    let mut record = read_record(&file, RECORD_BYTES)?;
    record["reviewOffered"] = json!(true);
    Ok(state::write(&file, &record, RECORD_BYTES, false)?)
}
