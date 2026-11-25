//! `task-start`: bind one task to the session for the current checkout, route its stage,
//! files and risks, and snapshot its selected source. Nothing is accepted, delegated or
//! switched.
use crate::failure::Failure;
use crate::scratch::{self, Action};
use crate::session::{self as records, BINDING_BYTES};
use crate::telemetry::{Operation, Recorder};
use craft_engine::values::{safe_integer, truthy};
use craft_engine::{actions, repository, routing, session};
use craft_platform::{repository::snapshot, state};
use serde_json::{Map, Value, json};
use std::path::Path;

pub struct Start<'a> {
    pub root: &'a Path,
    pub session: &'a str,
    pub task: &'a str,
    /// The engine start input without `id`.
    pub input: Map<String, Value>,
    pub host: Option<&'a str>,
    pub settings: &'a Value,
    /// The work plan the task belongs to.
    pub plan: Option<Planned<'a>>,
    /// The issue the task implements, named by its URL, number or key.
    pub issue: Option<&'a str>,
}

/// A plan task to claim: `take_over` moves it from the session that holds it.
pub struct Planned<'a> {
    pub id: &'a str,
    pub take_over: bool,
}

/// Bind and route the task under the active binding lock.
pub fn start(
    request: Start,
    recorder: &mut Recorder,
    parent: &Operation,
) -> Result<Value, Failure> {
    let Start {
        root,
        session: name,
        task,
        mut input,
        host,
        settings,
        plan: planned,
        issue: named,
    } = request;
    if named.is_some_and(|reference| craft_engine::ticket::issue_id(reference).is_none()) {
        return Err("--issue takes the issue's URL, number or key".into());
    }
    input.insert("id".into(), json!(task));
    let mut input = Value::Object(input);
    let file = records::binding_file(root, name, true)?;
    let Some(_lock) = state::lock(&file)? else {
        return Err("active task writer is busy".into());
    };
    let scope = |previous: &Value| -> Result<(Option<Value>, Value), Failure> {
        let repositories = match input.get("repositories") {
            None => previous["taskRecord"].get("repositories").cloned(),
            Some(value) => Some(json!(repository::scopes(value)?)),
        };
        let repository = snapshot::workspace(root, repositories.as_ref(), false)?;
        Ok((repositories, repository))
    };
    let mut previous = records::read_state(root, name, task, recorder, parent)?;
    // A plan task starts a new record when this session's earlier task under its id is over.
    let fresh = match &planned {
        Some(planned) => superseded(root, name, &previous, planned.id, input.get("goal"))?,
        None => false,
    };
    if fresh {
        previous = session::empty();
    }
    let (mut repositories, mut repository) = scope(&previous)?;
    let mut claimed = Value::Null;
    if let Some(planned) = &planned {
        // Refuse an invalid start before the plan records this session as the task's holder.
        session::start(&mut previous.clone(), &input, &repository)?;
        let from = crate::plan::claim(
            root,
            planned.id,
            task,
            name,
            planned.take_over,
            repositories.as_ref(),
        )?;
        claimed = json!({"id": planned.id, "task": task});
        if let Some(from) = from {
            let carried = crate::handoff::carry(root, name, &from, task, recorder, parent)?;
            // An orphaned claim's session is gone; the task continues from the plan alone.
            claimed["takenOver"] = if carried.is_null() {
                json!({"from": from, "carried": false, "reason": "the holding session left no record of this task"})
            } else {
                carried
            };
            previous = records::read_state(root, name, task, recorder, parent)?;
            (repositories, repository) = scope(&previous)?;
        }
    }
    let issue = issue(root, name, named, planned.as_ref().map(|p| p.id));
    // The criteria the issue states become the contract of a task tied to the issue, unless
    // one is given, the task already carries one, or it is a plan task that states its own
    // part of the acceptance.
    let own_acceptance = planned.as_ref().is_some_and(|planned| {
        crate::plan::task(root, planned.id, task).is_ok_and(
            |planned| match &planned["acceptance"] {
                Value::Array(items) => !items.is_empty(),
                other => truthy(other),
            },
        )
    });
    let tied = issue
        .as_ref()
        .is_some_and(|issue| tied(root, name, named, planned.as_ref().map(|p| p.id), issue));
    if input.get("engineeringContract").is_none()
        && tied
        && !own_acceptance
        && !truthy(&previous["taskRecord"]["engineeringContract"])
        && let Some(contract) = issue
            .as_ref()
            .and_then(craft_engine::ticket::criteria::contract)
    {
        input["engineeringContract"] = contract;
    }
    // The contract keeps the criteria; the task's issue keeps its identity.
    let issue = issue.map(|mut issue| {
        if let Some(fields) = issue.as_object_mut() {
            fields.remove("criteria");
            fields.remove("criteriaOmitted");
        }
        issue
    });
    let mut started = previous;
    let intent = session::start(&mut started, &input, &repository)?;
    let record = &started["taskRecord"];
    let files = record["files"].as_array().cloned().unwrap_or_default();
    let risks: Vec<String> = record["risks"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|risk| risk.as_str().map(str::to_string))
        .collect();
    let route = routing::Request {
        intent: Some(intent),
        files: &files,
        risks: &risks,
        host: host.unwrap_or("agents"),
        delegate: false,
        task: None,
        verification_mode: None,
        artifact: None,
        packages: None,
    };
    let plan = crate::guidance::route(settings, &route, recorder, parent)?;
    // Revalidate under the task-state lock; a concurrent step must not be overwritten.
    records::update_state(root, name, task, recorder, parent, |state| {
        if fresh && state["taskRecord"].get("goal") != input.get("goal") {
            *state = session::empty();
        }
        session::start(state, &input, &repository)?;
        if let Some(planned) = &planned {
            state["plan"] = json!(planned.id);
        }
        if let Some(issue) = &issue {
            state["issue"] = issue.clone();
        }
        let routed = Map::from_iter([("routingDecisions".to_string(), json!(1))]);
        Ok(session::add_activity(state, &routed)?)
    })?;
    let mut binding = match &planned {
        Some(planned) => json!({"formatVersion": 2, "task": task, "plan": planned.id}),
        None => json!({"formatVersion": 1, "task": task}),
    };
    if let Some(checkout) = repository.get("checkout") {
        binding["checkout"] = checkout.clone();
    }
    if let Some(repositories) = repositories.as_ref().filter(|r| truthy(*r)) {
        binding["repositories"] = repositories.clone();
    }
    state::write(&file, &binding, BINDING_BYTES, false)?;
    let current = records::read_state(root, name, task, recorder, parent)?;
    let context = session::context(&current, &repository)?;
    // Optional housekeeping must never prevent useful task work.
    let maintain = scratch::Request {
        root,
        session: Some(name),
        task: Some(task),
        action: Action::Maintain,
        input: &json!({}),
        settings,
    };
    let workspace = scratch::run(maintain, recorder, parent).unwrap_or_else(|failure| {
        recorder.report("state", "STATE_UNAVAILABLE", Map::new(), parent);
        json!({"status": "retained", "reason": failure.message})
    });
    let checkpoint = if truthy(&settings["checkpoint"]["enabled"]) {
        (|| -> Result<Value, Failure> {
            let state = records::read_state(root, name, task, recorder, parent)?;
            let record = Some(&state["taskRecord"]).filter(|r| truthy(*r));
            let metadata = json!({"taskId": task, "taskContract": session::contract(record)?});
            crate::checkpoint::save(root, name, &metadata, None, recorder, parent)
        })()
        .unwrap_or_else(|failure| json!({"status": "unavailable", "reason": failure.message}))
    } else {
        json!({"status": "disabled"})
    };
    let mut result = json!({"session": name, "task": task});
    if let Some(issue) = &current.get("issue").filter(|i| i.is_object()) {
        result["issue"] = (*issue).clone();
    }
    if let Some(repositories) = repositories.filter(|r| truthy(r)) {
        result["repositories"] = repositories;
    }
    if !claimed.is_null() {
        if let Some(planned) = &planned {
            // The task's context in the plan; a plan too large to summarize never blocks work.
            claimed["envelope"] = crate::plan::envelope(root, planned.id, task, None)
                .unwrap_or_else(
                    |failure| json!({"status": "unavailable", "reason": failure.message}),
                );
        }
        result["plan"] = claimed;
    }
    // Every reader of the task's contract sees the same criteria through the session.
    if context["acceptance"]["criteria"]
        .as_array()
        .is_some_and(|criteria| !criteria.is_empty())
    {
        result["views"] = json!(format!(
            "craft acceptance-view --view qa|product|architecture --session {name}"
        ));
    }
    for (key, value) in [
        ("routing", plan),
        ("acceptance", context["acceptance"].clone()),
        ("evidenceDigest", context["evidenceDigest"].clone()),
        ("taskContract", context["taskContract"].clone()),
        ("binding", context["binding"].clone()),
        ("checkpoint", checkpoint),
        ("workspace", workspace),
        (
            "authorization",
            json!(
                "Local task binding and snapshot only; no acceptance, worker, model switch or remote action was inferred."
            ),
        ),
    ] {
        result[key] = value;
    }
    Ok(result)
}

/// Whether `previous`, this session's record under the task id, belongs to earlier work that
/// is over: a task with another goal that completed, or that was claimed from another plan
/// where this session no longer holds it. A record of work still open is kept, and its
/// different goal refused.
fn superseded(
    root: &Path,
    session: &str,
    previous: &Value,
    plan: &str,
    goal: Option<&Value>,
) -> Result<bool, Failure> {
    let record = &previous["taskRecord"];
    if !truthy(record) || record.get("goal") == goal {
        return Ok(false);
    }
    if record["status"] == "completed" || record["goalComplete"] == true {
        return Ok(true);
    }
    let Some(earlier) = previous["plan"].as_str().filter(|earlier| *earlier != plan) else {
        return Ok(false);
    };
    let id = record["id"].as_str().unwrap_or_default();
    Ok(crate::plan::held(root, earlier, id, session).is_err())
}

/// The issue a starting task implements: the one `named`, else the ticket its plan links,
/// else the issue the session linked last. A linked issue of the same id brings what its
/// tracker returned; otherwise the task carries the reference alone.
fn issue(root: &Path, session: &str, named: Option<&str>, plan: Option<&str>) -> Option<Value> {
    use craft_engine::ticket::issue_id;
    let linked = crate::ticket::linked_issue(root, session);
    let Some(reference) = named
        .map(str::to_owned)
        .or_else(|| plan_ticket(root, plan?))
    else {
        return linked;
    };
    let same = |issue: &&Value| {
        issue["reference"]
            .as_str()
            .and_then(issue_id)
            .zip(issue_id(&reference))
            .is_some_and(|(a, b)| a.eq_ignore_ascii_case(&b))
    };
    Some(
        linked
            .as_ref()
            .filter(same)
            .cloned()
            .unwrap_or_else(|| json!({"reference": reference})),
    )
}

/// The ticket plan `plan` links, `None` without one or when the plan cannot be read.
fn plan_ticket(root: &Path, plan: &str) -> Option<String> {
    let saved = crate::plan::read(root, plan).ok()??;
    saved["references"]
        .as_array()?
        .iter()
        .find_map(|reference| match reference {
            // Saved as `{kind, id}`; a reference kept as `kind:id` text reads the same.
            Value::Object(_) => matches!(reference["kind"].as_str(), Some("link" | "tracker"))
                .then(|| reference["id"].as_str().map(str::to_owned))
                .flatten(),
            _ => {
                let text = reference.as_str()?;
                text.strip_prefix("link:")
                    .or_else(|| text.strip_prefix("tracker:"))
                    .map(str::to_owned)
            }
        })
}

/// Whether a starting task implements `issue` itself, not only shares the session that
/// linked it: the task names the issue, its plan links it, or it is the ticket whose
/// delivery the session recorded.
fn tied(
    root: &Path,
    session: &str,
    named: Option<&str>,
    plan: Option<&str>,
    issue: &Value,
) -> bool {
    use craft_engine::ticket::issue_id;
    let Some(id) = issue["reference"].as_str().and_then(issue_id) else {
        return false;
    };
    let ticket = named
        .map(str::to_owned)
        .or_else(|| plan.and_then(|plan| plan_ticket(root, plan)))
        .or_else(|| crate::ticket::recorded(root, session));
    ticket
        .as_deref()
        .and_then(issue_id)
        .is_some_and(|ticket| ticket.eq_ignore_ascii_case(&id))
}

/// The issue a reviewed change implements: the one `named`, else the issue of the task the
/// session holds, else the issue the session linked last.
pub(crate) fn change_issue(
    root: &Path,
    session: Option<&str>,
    named: Option<&str>,
    recorder: &mut Recorder,
    parent: &Operation,
) -> Result<Option<Value>, Failure> {
    if named.is_some_and(|reference| craft_engine::ticket::issue_id(reference).is_none()) {
        return Err("--issue takes the issue's URL, number or key".into());
    }
    let Some(session) = session else {
        return Ok(named.map(|reference| json!({"reference": reference})));
    };
    if named.is_some() {
        return Ok(issue(root, session, named, None));
    }
    // A task or checkout Craft cannot read leaves the session's own link.
    let bound = records::active_task(root, Some(session))
        .ok()
        .flatten()
        .and_then(|task| records::read_state(root, session, &task, recorder, parent).ok())
        .and_then(|state| state.get("issue").filter(|i| i.is_object()).cloned());
    Ok(bound.or_else(|| crate::ticket::linked_issue(root, session)))
}

pub struct Step<'a> {
    pub root: &'a Path,
    pub session: &'a str,
    pub task: &'a str,
    /// The engine step input; the attempt budget is added here.
    pub input: Map<String, Value>,
    pub host: Option<&'a str>,
    pub bundles: &'a [String],
}

const BOUND: &str = "task-step requires the active task binding; use task-start to switch tasks";

/// Record one acceptance step for the bound task under the active binding lock, save its
/// continuation, observe status and offer a review the status recommends. Nothing is
/// executed or approved.
pub fn step(
    request: Step,
    load: crate::actions::Load,
    recorder: &mut Recorder,
    parent: &Operation,
) -> Result<Value, Failure> {
    let Step {
        root,
        session: name,
        task,
        input,
        host,
        bundles,
    } = request;
    session::identity(&json!(name), &json!(task))?;
    if records::active_task(root, Some(name))?.as_deref() != Some(task) {
        return Err(BOUND.into());
    }
    let file = records::binding_file(root, name, true)?;
    let Some(_lock) = state::lock(&file)? else {
        return Err("active task writer is busy".into());
    };
    let binding = records::active_binding(root, name)?;
    if binding.as_ref().and_then(|b| b["task"].as_str()) != Some(task) {
        return Err(BOUND.into());
    }
    let planned = binding
        .as_ref()
        .and_then(|b| b["plan"].as_str())
        .map(str::to_string);
    if let Some(plan) = &planned {
        crate::plan::held(root, plan, task, name)?;
    }
    let settings = &load(recorder, parent)?;
    let saved = records::read_state(root, name, task, recorder, parent)?;
    let repositories = saved["taskRecord"].get("repositories");
    // A plan task completes only after its children and within the paths it owns;
    // refused before the step is recorded.
    let scope = match &planned {
        Some(plan) if input.get("complete") == Some(&json!(true)) => {
            crate::plan::completable(root, plan, task, repositories)?
        }
        _ => Value::Null,
    };
    let mut input = input;
    if let Some(ledger) = input.get_mut("acceptanceEvidence") {
        let kept = &saved["taskRecord"]["acceptanceEvidence"];
        crate::test_runs::fill_evidence(root, name, repositories, kept, ledger)?;
    }
    // A declared pass the latest observed run on this tree contradicts is refused, whether
    // the step names the verify stage or carries it over from the task.
    if session::step::records_passed_verification(&saved, &input)
        && let Some(status) = crate::test_runs::status(root, name, repositories)?
        && let Some(refusal) = session::test_runs::refutes_pass(&status)
    {
        return Err(refusal.into());
    }
    // The source the agent verified; a worktree completion is recorded against it, before
    // craft commits it.
    let repository = snapshot::workspace(root, repositories, false)?;
    let input = Value::Object(crate::session::bounded(settings, input));
    let result = input["result"].clone();
    let id = craft_platform::telemetry::uuid()?;
    let at = craft_engine::timestamp::format(crate::telemetry::now_ms());
    // A task in worktrees completes as commits on its branches, which integration merges.
    // The step is checked first, so a refused completion leaves the worktrees untouched.
    let heads = match &planned {
        Some(plan) if input.get("complete") == Some(&json!(true)) => {
            let found = crate::plan::task(root, plan, task)?;
            match found.get("work") {
                Some(work) => {
                    session::step::step(&mut saved.clone(), &input, &repository, &id, &at)?;
                    let goal: String = found["goal"]
                        .as_str()
                        .unwrap_or_default()
                        .chars()
                        .take(200)
                        .collect();
                    crate::delegation::commit(
                        &state::canonical_root(root)?,
                        work,
                        &format!("craft {plan}/{task}: {goal}"),
                    )?
                }
                None => Vec::new(),
            }
        }
        _ => Vec::new(),
    };
    let mut recorded = None;
    let updated = records::update_state(root, name, task, recorder, parent, |state| {
        recorded = Some(session::step::step(state, &input, &repository, &id, &at)?);
        Ok(())
    })?;
    let recorded = recorded.expect("recorded step");
    let checkpoint = if truthy(&settings["checkpoint"]["enabled"]) {
        (|| -> Result<Value, Failure> {
            let state = records::read_state(root, name, task, recorder, parent)?;
            let record = Some(&state["taskRecord"]).filter(|r| truthy(*r));
            let metadata = json!({"taskId": task, "taskContract": session::contract(record)?, "continuation": recorded.continuation});
            crate::checkpoint::save(root, name, &metadata, Some(&recorded.receipt["source"]), recorder, parent)
        })()
        .unwrap_or_else(|failure| json!({"status": "unavailable", "reason": failure.message}))
    } else {
        json!({"status": "disabled"})
    };
    let declared = match host {
        Some(host) => json!({"host": {"name": host}}),
        None => json!({}),
    };
    let observed = crate::status::Request {
        root,
        input: &declared,
        settings,
        session: Some(name),
        task: Some(task),
        bundles,
    };
    let mut status = crate::status::run(observed, recorder, parent)
        .unwrap_or_else(|failure| json!({"diagnostic": failure.message}));
    let mut offer = None;
    if status["recommendation"]["action"] == "review"
        && settings["efficiency"]["review"]["mode"] != "manual"
    {
        let proposed = records::update_state(root, name, task, recorder, parent, |state| {
            let mut proposal = json!({
                "action": "review", "scope": "review:current-source",
                "benefit": "Review the verified changes for declared risks.", "evidence": recorded.receipt["evidence"],
            });
            if let Some(prior) = state["taskRecord"]
                .get("reviewAuthorization")
                .filter(|p| truthy(*p))
            {
                proposal["priorAuthorization"] = prior.clone();
            }
            let facts = json!({"observations": {"task": status["task"]}});
            let context = actions::Context {
                settings,
                repository: &repository,
                status: &status,
                facts: &facts,
                session: Some(name),
                now: crate::telemetry::now_ms(),
            };
            offer = Some(actions::propose(state, &proposal, &context)?);
            Ok(())
        });
        if let Err(failure) = proposed {
            status["actionDiagnostic"] = json!(failure.message);
        }
    }
    if truthy(&status["notice"]) {
        let notice = status["notice"].clone();
        let written = records::update_state(root, name, task, recorder, parent, |state| {
            state["notice"] = notice;
            Ok(())
        });
        if let Err(failure) = written {
            status["noticeDiagnostic"] = json!(failure.message);
        }
    }
    let record = &updated["taskRecord"];
    // Committed worktree changes move the observed source past the one the step verified.
    let complete = record["goalComplete"] == true
        && (!truthy(&record["engineeringContract"])
            || !heads.is_empty()
            || status["task"]["goalComplete"] == true);
    let boundary = scratch::Request {
        root,
        session: Some(name),
        task: Some(task),
        action: if complete {
            Action::Complete
        } else {
            Action::Touch
        },
        input: &json!({}),
        settings,
    };
    let workspace = scratch::run(boundary, recorder, parent).unwrap_or_else(|failure| {
        recorder.report("state", "STATE_UNAVAILABLE", Map::new(), parent);
        json!({"status": "retained", "reason": failure.message})
    });
    status["workspace"] = workspace.clone();
    let planned = planned.map(|plan| {
        let text = |name: &str| input.get(name).and_then(Value::as_str);
        let activity = &status["activity"];
        let exhausted = matches!(
            (safe_integer(&activity["attempts"]), safe_integer(&activity["maxAttempts"])),
            (Some(made), Some(most)) if made >= most
        );
        // The source and engineering contract a completion was verified against.
        let verified = complete.then(|| {
            let contract = Some(&record["engineeringContract"])
                .filter(|c| truthy(*c))
                .and_then(|c| craft_engine::session::acceptance::contract_digest(c).ok());
            let revision = [&record["verificationRevision"], &record["revision"]]
                .into_iter()
                .find(|r| r.is_string())
                .cloned()
                .unwrap_or(Value::Null);
            json!({"revision": revision, "contract": contract, "at": at})
        });
        let progress = craft_engine::plan::Progress {
            stage: record["stage"].as_str().unwrap_or_default(),
            outcome: text("outcome"),
            complete,
            exhausted,
            verified,
            heads: &heads,
            result: text("result"),
            // The recorded one: a completion without `--next` names its hand-back.
            next_step: record["nextStep"].as_str(),
            evidence: text("evidence"),
        };
        crate::plan::advance(root, &plan, task, name, &progress).unwrap_or_else(
            |failure| json!({"plan": plan, "status": "unavailable", "reason": failure.message}),
        )
    });
    let suppressed = offer.as_ref().is_some_and(|o| o["suppressed"] == true);
    let acceptance = match status.get("acceptance") {
        Some(acceptance) if !acceptance.is_null() => acceptance.clone(),
        _ => recorded.acceptance,
    };
    let mut response = json!({
        "session": name, "task": task, "recorded": recorded.receipt, "result": result, "complete": complete,
        "acceptance": acceptance, "workspace": workspace, "checkpoint": checkpoint,
    });
    if let Some(mut planned) = planned {
        if !scope.is_null() {
            planned["scope"] = scope;
        }
        response["plan"] = planned;
    }
    if let Some(offer) = &offer {
        response["actionOffer"] = json!({"offer": offer["offer"], "suppressed": offer["suppressed"], "applicability": offer["applicability"], "executed": false});
    }
    response["nextStep"] = if suppressed {
        Value::Null
    } else {
        status.get("suggestion").cloned().unwrap_or(Value::Null)
    };
    response["status"] = status;
    response["authorization"] =
        json!("Agent-declared acceptance recorded locally; no recommended action was executed.");
    Ok(response)
}

/// Record one decision on the session's bound task: what, why, rejected alternatives and
/// who decided. Shown when the task resumes after compaction or in another session.
pub fn decide(
    root: &Path,
    name: &str,
    input: &Value,
    recorder: &mut Recorder,
    parent: &Operation,
) -> Result<Value, Failure> {
    let task = records::active_task(root, Some(name))?
        .ok_or("task-decide records on the session's bound task; start or adopt one first")?;
    let at = craft_engine::timestamp::format(crate::telemetry::now_ms());
    let mut recorded = Value::Null;
    let state = records::update_state(root, name, &task, recorder, parent, |state| {
        recorded = session::resume::decide(state, input, &at)?;
        Ok(())
    })?;
    let current = state["decisions"].as_array().map_or(0, |d| {
        d.iter().filter(|d| d.get("supersededBy").is_none()).count()
    });
    Ok(json!({"session": name, "task": task, "decision": recorded, "current": current}))
}
