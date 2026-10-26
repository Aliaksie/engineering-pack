//! Recommended actions for the bound task (review, compact, delegate): eligibility from the
//! current source, policy, status and caller-declared host facts; recorded offers, explicit
//! decisions and readiness checks. Nothing here dispatches, approves or grants permission.
use crate::contracts::safe_integer;
use crate::session::{self, BLOCKERS, text, unresolved};
use crate::{digest, models, timestamp, workers};
use serde_json::{Map, Value, json};

type Result<T = ()> = std::result::Result<T, String>;

const ACTIONS: &[&str] = &["review", "delegate", "compact"];
const FACTS: &[&str] = &[
    "observations",
    "workers",
    "independentTasks",
    "allowed",
    "supported",
    "reuseAvailable",
];
/// Recorded offers per task.
const OFFERS: usize = 16;

fn fields<'a>(value: &'a Value, allowed: &[&str], label: &str) -> Result<&'a Map<String, Value>> {
    value
        .as_object()
        .filter(|m| m.keys().all(|k| allowed.contains(&k.as_str())))
        .ok_or_else(|| format!("invalid {label}"))
}

fn truthy(value: Option<&Value>) -> bool {
    match value {
        None | Some(Value::Null | Value::Bool(false)) => false,
        Some(Value::String(s)) => !s.is_empty(),
        Some(Value::Number(n)) => n.as_f64().is_some_and(|f| f != 0.0),
        Some(_) => true,
    }
}

/// Validated caller-declared host facts; absent means none.
pub fn facts(value: &Value) -> Result<&Map<String, Value>> {
    let facts = fields(value, FACTS, "action facts")?;
    if let Some(count) = facts.get("independentTasks")
        && !safe_integer(count).is_some_and(|n| (0..=64).contains(&n))
    {
        return Err("invalid independent task count".into());
    }
    for name in ["allowed", "supported", "reuseAvailable"] {
        if facts.get(name).is_some_and(|v| !v.is_boolean()) {
            return Err(format!("invalid action {name}"));
        }
    }
    Ok(facts)
}

/// What an action is judged against: the task state and source, effective policy, the
/// unified status and fresh caller facts.
pub struct Context<'a> {
    pub settings: &'a Value,
    pub repository: &'a Value,
    pub status: &'a Value,
    pub facts: &'a Value,
    pub session: Option<&'a str>,
    pub now: i64,
}

fn fail(reason: &str) -> Value {
    json!({"eligible": false, "reason": reason})
}

fn with_budget(mut value: Value, budget: &Value) -> Value {
    value["budget"] = budget.clone();
    value
}

fn eligibility(action: &str, input: &Value, state: &Value, context: &Context) -> Result<Value> {
    let Context {
        settings,
        repository,
        status,
        facts,
        session,
        now,
    } = *context;
    let source = session::source(repository);
    if source.is_null() {
        return Ok(fail("Complete repository evidence is unavailable."));
    }
    if facts["allowed"] == false {
        return Ok(fail(
            "Current session or consumer policy prohibits this action.",
        ));
    }
    if facts["supported"] == false {
        return Ok(fail("The host does not support this action."));
    }
    let observed = &facts["observations"];
    let efficiency = &settings["efficiency"];
    if action == "review" {
        if efficiency["review"]["mode"] == "manual" {
            return Ok(fail("Automatic review recommendations are disabled."));
        }
        let record = &state["taskRecord"];
        let verified = record.get("source") == Some(&source)
            && record["verification"] == "passed"
            && !truthy(status.get("taskEvidenceStale"));
        let risks = observed["task"]["risks"]
            .as_array()
            .is_some_and(|r| !r.is_empty());
        if !truthy(input.get("priorAuthorization"))
            && !truthy(observed["task"].get("reviewRequested"))
            && !(verified && risks)
        {
            return Ok(fail(
                "Review needs a requested scope or current verified acceptance and a declared risk.",
            ));
        }
        return Ok(
            json!({"eligible": true, "reason": "A scoped review is requested or risk justifies an additional review of verified source."}),
        );
    }
    if action == "compact" {
        let task = &status["task"];
        if efficiency["context"]["action"] == "manual" {
            return Ok(fail("Automatic context recommendations are disabled."));
        }
        if observed["host"]["reliableCompaction"] != true {
            return Ok(fail(
                "Reliable host compaction is unsupported or unverified.",
            ));
        }
        if truthy(task.get("goalComplete")) {
            return Ok(fail(
                "The task is complete; return its outcome rather than changing context.",
            ));
        }
        let boundary = task["safeBoundary"] == true
            || (task.get("safeBoundary").is_none() && task["status"] == "completed");
        if !boundary {
            return Ok(fail("Compaction requires a safe task boundary."));
        }
        if truthy(task.get("almostComplete")) {
            return Ok(fail(
                "Finish the almost-complete task before changing context.",
            ));
        }
        if !["high", "saturated"]
            .iter()
            .any(|p| status["pressure"] == *p)
            || status["checkpoint"]["status"] != "fresh"
        {
            return Ok(fail(
                "Fresh observed pressure and a matching fresh continuation checkpoint are required.",
            ));
        }
        return Ok(
            json!({"eligible": true, "reason": "Fresh host occupancy and a fresh continuation support compaction at this boundary."}),
        );
    }
    if efficiency["delegation"]["mode"] == "manual" {
        return Ok(fail("Automatic worker recommendations are disabled."));
    }
    let attempts = state["attempts"].as_array().map_or(&[][..], Vec::as_slice);
    if unresolved(attempts)
        .into_iter()
        .any(|i| BLOCKERS.iter().any(|b| attempts[i]["outcome"] == *b))
    {
        return Ok(fail(
            "Resolve the recorded prerequisite; workers cannot bypass a permission or tool blocker.",
        ));
    }
    let budget = workers::budget(
        settings,
        facts.get("workers").unwrap_or(&json!({})),
        session,
        now,
    )?;
    let number = |name: &str| budget[name].as_i64();
    let occupied = number("occupiedTaskWorkers");
    let limit = number("effectiveLimit");
    let reusable = facts["reuseAvailable"] == true
        && limit.is_some_and(|l| l > 0)
        && occupied.is_some_and(|o| o > 0 && Some(o) <= limit);
    let available = number("available").unwrap_or(0);
    if available < 1 && !reusable {
        return Ok(with_budget(
            fail("No authorized, observed worker capacity is available."),
            &budget,
        ));
    }
    let independent = facts
        .get("independentTasks")
        .and_then(safe_integer)
        .unwrap_or(0);
    if independent < 2 {
        return Ok(with_budget(
            fail("Bounded independent work does not justify delegation."),
            &budget,
        ));
    }
    let role = input["role"].as_str();
    let profile = input
        .get("profile")
        .filter(|p| !p.is_null())
        .or_else(|| role.and_then(|r| settings["execution"]["roles"][r].get("profile")))
        .filter(|p| truthy(Some(p)));
    let Some(profile) = profile else {
        return Ok(with_budget(
            fail("A configured logical worker profile is required."),
            &budget,
        ));
    };
    let host = observed["host"]["name"].as_str();
    let execution = match (role, host) {
        (Some(role), Some(host)) => models::delegation(settings, role, host, profile.as_str()),
        _ => None,
    };
    let Some(execution) = execution else {
        return Ok(with_budget(
            fail("An exact supported host/role/model mapping is unavailable."),
            &budget,
        ));
    };
    let workers = independent.min(if reusable {
        available.max(1)
    } else {
        available
    });
    Ok(json!({
        "eligible": true, "reason": "Independent assignments fit the shared task-wide worker ceiling.",
        "budget": budget, "execution": execution, "workerCount": workers,
    }))
}

/// Evaluate a proposed action and record a new offer when eligible; an identical offer is
/// returned as suppressed. Returns `{offer, suppressed, applicability, executed}`.
pub fn propose(state: &mut Value, input: &Value, context: &Context) -> Result<Value> {
    let map = fields(
        input,
        &[
            "action",
            "scope",
            "benefit",
            "evidence",
            "role",
            "profile",
            "priorAuthorization",
        ],
        "action proposal",
    )?;
    let action = map
        .get("action")
        .and_then(Value::as_str)
        .filter(|a| ACTIONS.contains(a));
    let Some(action) = action.filter(|_| {
        ["scope", "benefit", "evidence"]
            .iter()
            .all(|f| map.get(*f).is_some_and(text))
    }) else {
        return Err("action, bounded scope, benefit and evidence are required".into());
    };
    for name in ["role", "profile", "priorAuthorization"] {
        if map.get(name).is_some_and(|v| !text(v)) {
            return Err(format!("invalid action {name}"));
        }
    }
    facts(context.facts)?;
    let applicable = eligibility(action, input, state, context)?;
    let result = |offer: Value, suppressed: bool, applicable: &Value| json!({"offer": offer, "suppressed": suppressed, "applicability": applicable, "executed": false});
    if applicable["eligible"] != true {
        return Ok(result(Value::Null, true, &applicable));
    }
    let source = session::source(context.repository);
    let policy = json!(digest(context.settings));
    let task_contract = session::contract(state.get("taskRecord"))?;
    let mut risks: Vec<&Value> = Vec::new();
    for risk in context.facts["observations"]["task"]["risks"]
        .as_array()
        .into_iter()
        .flatten()
    {
        if !risks.contains(&risk) {
            risks.push(risk);
        }
    }
    risks.sort_by(|a, b| {
        a.as_str()
            .unwrap_or_default()
            .encode_utf16()
            .cmp(b.as_str().unwrap_or_default().encode_utf16())
    });
    let host = context.facts["observations"]["host"]
        .get("name")
        .filter(|h| !h.is_null())
        .cloned()
        .unwrap_or(Value::Null);
    let model = applicable["execution"]
        .get("model")
        .cloned()
        .unwrap_or(Value::Null);
    let role = map.get("role").cloned().unwrap_or(Value::Null);
    let profile = applicable["execution"]
        .get("profile")
        .or_else(|| map.get("profile"))
        .cloned()
        .unwrap_or(Value::Null);
    let id = digest(&json!([
        state["taskRecord"]["id"],
        action,
        map["scope"],
        source,
        policy,
        role,
        profile,
        risks,
        host,
        model,
        task_contract
    ]));
    if state.get("recommendations").is_none_or(Value::is_null) {
        state["recommendations"] = json!({});
    }
    let offers = state["recommendations"]
        .as_object_mut()
        .ok_or("invalid recommendation state")?;
    if let Some(existing) = offers.get(&id) {
        return Ok(result(existing.clone(), true, &applicable));
    }
    if offers.len() >= OFFERS {
        let old = offers
            .iter()
            .find(|(_, offer)| {
                offer.get("executionReceipt").is_none()
                    && (offer.get("source") != Some(&source)
                        || offer.get("taskContract") != Some(&task_contract)
                        || offer["decision"] == "declined")
            })
            .map(|(key, _)| key.clone());
        let Some(old) = old else {
            return Err("task recommendation capacity reached; resolve existing proposals before adding more".into());
        };
        offers.shift_remove(&old);
    }
    let prior = map.get("priorAuthorization");
    let mut offer = json!({
        "id": id, "action": action, "scope": map["scope"], "benefit": map["benefit"], "evidence": map["evidence"],
        "source": source, "policy": policy, "taskContract": task_contract, "risks": risks, "host": host, "model": model,
        "role": role, "profile": profile,
        "decision": if prior.is_some() { "already-authorized" } else { "pending" },
        "decisionEvidence": prior.cloned().unwrap_or(Value::Null), "at": timestamp::format(context.now),
        "execution": "not-observed", "trigger": applicable["reason"],
    });
    if let Some(budget) = applicable.get("budget") {
        offer["workerBudget"] = budget.clone();
        offer["proposedWorkers"] = applicable["workerCount"].clone();
    }
    offers.insert(id, offer.clone());
    Ok(result(offer, false, &applicable))
}

/// The recorded offer `id`, unless its source, policy or task contract changed.
fn offer<'a>(
    state: &'a Value,
    id: &Value,
    settings: &Value,
    repository: &Value,
) -> Result<&'a Value> {
    let known = id
        .as_str()
        .filter(|id| {
            id.len() == 64
                && id
                    .bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        })
        .and_then(|id| state["recommendations"].get(id))
        .filter(|offer| truthy(Some(offer)));
    let offer = known.ok_or("unknown action proposal")?;
    if offer.get("source") != Some(&session::source(repository))
        || offer.get("policy") != Some(&json!(digest(settings)))
    {
        return Err("action proposal is stale; source or effective policy changed".into());
    }
    let record = state.get("taskRecord");
    if (offer.get("taskContract").is_some()
        || truthy(record.and_then(|r| r.get("engineeringContract"))))
        && offer.get("taskContract") != Some(&session::contract(record)?)
    {
        return Err(
            "action proposal is stale; task contract changed or its binding is unavailable".into(),
        );
    }
    Ok(offer)
}

/// Record an explicit approval or decline of a current offer. Returns `{offer, executed, authorization}`.
pub fn decide(
    state: &mut Value,
    input: &Value,
    settings: &Value,
    repository: &Value,
    now: i64,
) -> Result<Value> {
    let map = fields(input, &["id", "decision", "evidence"], "action decision")?;
    let decision = map
        .get("decision")
        .filter(|d| *d == "approved" || *d == "declined");
    let (Some(decision), Some(evidence)) = (decision, map.get("evidence").filter(|e| text(e)))
    else {
        return Err("explicit approval/decline and decision evidence are required".into());
    };
    let id = map.get("id").cloned().unwrap_or(Value::Null);
    offer(state, &id, settings, repository)?;
    let offer = &mut state["recommendations"][id.as_str().expect("known offer")];
    offer["decision"] = decision.clone();
    offer["decisionEvidence"] = evidence.clone();
    offer["decidedAt"] = json!(timestamp::format(now));
    Ok(json!({
        "offer": offer, "executed": false,
        "authorization": "Caller-declared decision receipt; no host permission or completed execution is implied.",
    }))
}

/// Whether an offer's decision, current eligibility and fresh host facts allow the host to
/// perform it. Readiness is not execution or a permission grant.
pub fn check(state: &Value, input: &Value, context: &Context) -> Result<Value> {
    let map = fields(input, &["id"], "action check")?;
    facts(context.facts)?;
    let offer = offer(
        state,
        map.get("id").unwrap_or(&Value::Null),
        context.settings,
        context.repository,
    )?;
    if truthy(offer.get("host"))
        && context.facts["observations"]["host"].get("name") != offer.get("host")
    {
        return Ok(
            json!({"offer": offer, "ready": false, "executed": false, "reason": "Host identity changed or is unavailable; re-evaluate this action scope."}),
        );
    }
    let mut declared = offer.clone();
    declared["priorAuthorization"] = offer
        .get("decisionEvidence")
        .cloned()
        .unwrap_or(Value::Null);
    let action = offer["action"].as_str().unwrap_or_default();
    let applicable = eligibility(action, &declared, state, context)?;
    let approved = ["approved", "already-authorized"]
        .iter()
        .any(|d| offer["decision"] == *d);
    let eligible = applicable["eligible"] == true;
    let verified = context.facts["allowed"] == true && context.facts["supported"] == true;
    let count = |value: &Value| value.as_i64().unwrap_or(0);
    let limit = if action == "delegate" {
        json!(count(&offer["proposedWorkers"]).min(count(&applicable["workerCount"])))
    } else {
        Value::Null
    };
    let reason = if !approved {
        format!(
            "Action remains {}; silence is not approval.",
            offer["decision"].as_str().unwrap_or("undefined")
        )
    } else if !eligible {
        applicable["reason"]
            .as_str()
            .unwrap_or_default()
            .to_string()
    } else if !verified {
        "Fresh session policy and host support must be explicitly verified.".to_string()
    } else {
        "Scoped approval and current prerequisites are present; the host must perform and observe the action separately.".to_string()
    };
    Ok(json!({
        "offer": offer, "applicability": applicable, "workerLimit": limit, "ready": approved && eligible && verified, "executed": false,
        "authorization": "Decision and host facts are caller declarations, not a permission grant; verify the real conversation and host policy.",
        "reason": reason,
    }))
}

/// Reserve one durable execution identity before calling a trusted host.
pub fn claim(
    state: &mut Value,
    input: &Value,
    context: &Context,
    checkpoint: &Value,
) -> Result<Value> {
    let checked = check(state, input, context)?;
    let offer = &checked["offer"];
    if offer["action"] != "review" && offer["action"] != "compact" {
        return Err("worker execution uses the shared worker controller".into());
    }
    if truthy(offer.get("executionReceipt")) {
        return Ok(json!({"state":state,"receipt":offer["executionReceipt"],"duplicate":true}));
    }
    if checked["ready"] != true {
        return Err(checked["reason"].as_str().unwrap_or_default().into());
    }
    if offer["action"] == "compact"
        && !checkpoint.as_str().is_some_and(|s| {
            s.len() == 64
                && s.bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        })
    {
        return Err("compaction requires a bound checkpoint identity".into());
    }
    let id = input["id"].as_str().expect("checked offer");
    let receipt = json!({"key":digest(&json!([offer["id"],"execute"])),"state":"dispatching","source":offer["source"],"checkpoint":checkpoint,"generation":0,"evidence":null,"acceptanceEvidence":null});
    state["recommendations"][id]["executionReceipt"] = receipt.clone();
    state["recommendations"][id]["execution"] = json!("dispatching");
    Ok(json!({"state":state,"receipt":receipt,"duplicate":false}))
}

/// Settle only the current generation; terminal and superseded receipts cannot regress.
pub fn finish(
    state: &mut Value,
    id: &Value,
    key: &Value,
    generation: &Value,
    result: &Value,
) -> Result<Value> {
    let offer = id
        .as_str()
        .and_then(|id| state["recommendations"].get(id))
        .ok_or("unknown action execution")?;
    let receipt = offer
        .get("executionReceipt")
        .filter(|r| r.get("key") == Some(key))
        .ok_or("unknown action execution")?;
    if receipt.get("generation") != Some(generation)
        || receipt["state"] == "completed"
        || receipt["state"] == "not-started"
    {
        return Ok(json!({"state":state,"receipt":receipt,"ignored":true}));
    }
    let map = fields(
        result,
        &[
            "state",
            "evidence",
            "source",
            "checkpoint",
            "resumed",
            "acceptanceEvidence",
            "fenced",
        ],
        "native action result",
    )?;
    if !["started", "completed", "not-started", "uncertain"]
        .iter()
        .any(|s| result["state"] == *s)
    {
        return Err("invalid native action state".into());
    }
    for field in ["evidence", "acceptanceEvidence"] {
        if map.get(field).is_some_and(|v| !v.is_null() && !text(v)) {
            return Err(format!("invalid native action {field}"));
        }
    }
    if map
        .get("source")
        .is_some_and(|v| Some(v) != receipt.get("source"))
        || map
            .get("checkpoint")
            .is_some_and(|v| Some(v) != receipt.get("checkpoint"))
        || ["resumed", "fenced"]
            .iter()
            .any(|f| map.get(*f).is_some_and(|v| !v.is_boolean()))
    {
        return Err("invalid native action binding".into());
    }
    if result["state"] != "uncertain"
        && (!result.get("evidence").is_some_and(text)
            || result.get("source") != receipt.get("source"))
    {
        return Err("native action result must identify its actual source/evidence".into());
    }
    if result["state"] == "not-started" && result["fenced"] != true {
        return Err("negative action lookup must fence late execution".into());
    }
    if result["state"] == "completed"
        && offer["action"] == "compact"
        && (result.get("checkpoint") != receipt.get("checkpoint")
            || result["resumed"] != true
            || !result.get("acceptanceEvidence").is_some_and(text))
    {
        return Err(
            "compaction completion requires matching resumed checkpoint and acceptance evidence"
                .into(),
        );
    }
    let next = safe_integer(&receipt["generation"])
        .filter(|generation| (0..9_007_199_254_740_991).contains(generation))
        .ok_or("invalid action generation")?
        + 1;
    let offer = &mut state["recommendations"][id.as_str().unwrap()];
    let receipt = &mut offer["executionReceipt"];
    receipt["state"] = result["state"].clone();
    receipt["evidence"] = result["evidence"].clone();
    receipt["acceptanceEvidence"] = result["acceptanceEvidence"].clone();
    receipt["generation"] = json!(next);
    let receipt = receipt.clone();
    offer["execution"] = if result["state"] == "completed" {
        json!("host-observed")
    } else {
        result["state"].clone()
    };
    Ok(json!({"state":state,"receipt":receipt,"ignored":false}))
}
