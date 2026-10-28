//! Session advice and unified status: the next useful action from explicit task state, a
//! current-context observation and recorded activity. Nothing here compacts, resets,
//! switches models, writes state or reads a transcript.
use super::{attempt_status, contract, text};
use crate::values::{safe_integer, truthy};
use crate::{digest, models, routing, timestamp};
use serde_json::{Map, Value, json};

type Result<T = ()> = std::result::Result<T, String>;

/// Execution roles by operation or stage.
const ROLES: &[(&str, &str)] = &[
    ("locate", "locator"),
    ("discover", "locator"),
    ("implement", "implementer"),
    ("verify", "verifier"),
    ("explore", "verifier"),
    ("review", "reviewer"),
    ("plan", "planner"),
    ("architecture", "planner"),
];
const INPUT_FIELDS: &[&str] = &[
    "sessionId",
    "task",
    "usage",
    "policy",
    "previous",
    "host",
    "activity",
    "packContext",
];
const TASK_FIELDS: &[&str] = &[
    "id",
    "revision",
    "verificationRevision",
    "verificationMode",
    "stage",
    "status",
    "verification",
    "goalComplete",
    "reviewRequested",
    "files",
    "handoffSaved",
    "safeBoundary",
    "phaseChanged",
    "staleAssumptions",
    "compacted",
    "almostComplete",
    "checkpointFresh",
    "risks",
    "operation",
    "nextStep",
];
const TASK_TEXTS: &[&str] = &[
    "id",
    "revision",
    "verificationRevision",
    "operation",
    "nextStep",
];
const TASK_FLAGS: &[&str] = &[
    "goalComplete",
    "reviewRequested",
    "handoffSaved",
    "safeBoundary",
    "phaseChanged",
    "staleAssumptions",
    "compacted",
    "almostComplete",
    "checkpointFresh",
];
const ADVICE_STAGES: &[&str] = &[
    "plan",
    "implement",
    "verify",
    "review",
    "explore",
    "story",
    "architecture",
    "locate",
    "discover",
];
pub const ACTIVITY: &[&str] = &[
    "discoveryRequests",
    "repeatedLookups",
    "batchedOperations",
    "reuseHits",
    "checkpoints",
    "attempts",
    "maxAttempts",
    "retries",
    "modelEscalations",
    "noProgressAttempts",
    "freshLookups",
    "routingDecisions",
    "filesRequested",
    "referencesRequested",
    "operationsAttempted",
];
const DEFAULT_WINDOW_MS: i64 = 300_000;

fn role(operation: Option<&str>) -> Option<&'static str> {
    let operation = operation?;
    ROLES.iter().find(|(o, _)| *o == operation).map(|(_, r)| *r)
}

fn fields<'a>(value: &'a Value, allowed: &[&str], label: &str) -> Result<&'a Map<String, Value>> {
    value
        .as_object()
        .filter(|m| m.keys().all(|k| allowed.contains(&k.as_str())))
        .ok_or_else(|| format!("invalid {label} fields"))
}

/// `value ?? fallback`, where JSON null is absent.
fn or<'a>(value: Option<&'a Value>, fallback: &'a Value) -> &'a Value {
    value.filter(|v| !v.is_null()).unwrap_or(fallback)
}

fn count(value: &Value) -> bool {
    safe_integer(value).is_some_and(|n| n >= 0)
}

fn number(value: &Value) -> Option<f64> {
    value.as_f64().filter(|n| n.is_finite())
}

/// A ratio as `JSON.stringify` writes it: integral values without a fraction.
fn ratio(value: f64) -> Value {
    if value.fract() == 0.0 {
        json!(value as i64)
    } else {
        json!(value)
    }
}

fn suggest(
    action: &str,
    reason: &str,
    effect: &str,
    prerequisites: &[&str],
    extra: Value,
) -> Value {
    let mut suggestion = json!({"action": action, "reason": reason, "effect": effect, "prerequisites": prerequisites});
    for (key, value) in extra.as_object().into_iter().flatten() {
        suggestion[key] = value.clone();
    }
    suggestion
}

/// The suggestion key identifies a session, declared task and suggestion; an absent session
/// is omitted, as `JSON.stringify` omits `undefined`.
fn notice_key(session: Option<&Value>, task: &Value, suggestion: &Value) -> String {
    let mut keyed = Map::new();
    if let Some(session) = session {
        keyed.insert("session".into(), session.clone());
    }
    keyed.insert("task".into(), task.clone());
    keyed.insert("suggestion".into(), suggestion.clone());
    digest(&Value::Object(keyed))
}

/// `window` is the observed model's declared window, used only when the host reports none.
fn usage(
    observed: Option<&Value>,
    session: Option<&Value>,
    now: i64,
    max_age: i64,
    window: Option<i64>,
) -> Result<Value> {
    let unknown = |reason: &str| json!({"status": "unknown", "confidence": "unknown", "reason": reason, "currentTokens": null, "capacityTokens": null, "usedRatio": null});
    let Some(observed) = observed else {
        return Ok(unknown("No current-context observation supplied."));
    };
    let fields = fields(
        observed,
        &[
            "kind",
            "currentTokens",
            "capacityTokens",
            "observedAt",
            "source",
            "sessionId",
        ],
        "usage",
    )?;
    let field = |name: &str| fields.get(name).unwrap_or(&Value::Null);
    let at = field("observedAt").as_str().and_then(timestamp::parse);
    let declared = window
        .filter(|_| field("capacityTokens").is_null())
        .map(|w| json!(w));
    let reported = declared.as_ref().unwrap_or(field("capacityTokens"));
    let (current, capacity) = (safe_integer(field("currentTokens")), safe_integer(reported));
    Ok(if field("kind") != "current-context" {
        unknown(
            "Cumulative or unspecified token counts do not establish current context occupancy.",
        )
    } else if !count(field("currentTokens"))
        || !count(reported)
        || capacity == Some(0)
        || current > capacity
    {
        unknown("Current-context counts are invalid or incomplete.")
    } else if !text(field("source")) || at.is_none_or(|at| at > now || now - at > max_age) {
        unknown("Observation source or freshness is unavailable, stale or invalid.")
    } else if !truthy(session) || fields.get("sessionId") != session {
        unknown("Observation does not identify this session.")
    } else {
        let (current, capacity) = (current.unwrap(), capacity.unwrap());
        let mut usage = json!({
            "status": "observed", "confidence": "observed", "source": field("source"), "observedAt": field("observedAt"),
            "currentTokens": current, "capacityTokens": capacity, "usedRatio": ratio(current as f64 / capacity as f64),
        });
        if declared.is_some() {
            usage["capacitySource"] = json!("model-context-limit");
        }
        usage
    })
}

/// Pure next-step advice over caller-declared task state and observations.
pub fn advice(input: &Value, settings: &Value, now: i64) -> Result<Value> {
    let input = fields(input, INPUT_FIELDS, "session input")?;
    let field = |name: &str| input.get(name);
    let session = field("sessionId");
    if session.is_some_and(|s| !text(s)) {
        return Err("invalid sessionId".into());
    }
    let context = &settings["efficiency"]["context"];
    let threshold = |name: &str, fallback: f64| match field("policy").and_then(|p| p.get(name)) {
        Some(value) => value.clone(),
        None => or(context.get(name), &json!(fallback)).clone(),
    };
    let elevated = threshold("elevatedAt", 0.6);
    let compact = threshold("compactAt", 0.75);
    let restart = threshold("restartAt", 0.85);
    let window = |name: &str| {
        field("policy")
            .and_then(|p| p.get(name))
            .cloned()
            .unwrap_or(json!(DEFAULT_WINDOW_MS))
    };
    let (max_age, cooldown) = (window("maxAgeMs"), window("cooldownMs"));
    if let Some(policy) = field("policy") {
        fields(
            policy,
            &[
                "elevatedAt",
                "compactAt",
                "restartAt",
                "maxAgeMs",
                "cooldownMs",
            ],
            "session policy",
        )?;
    }
    let ordered = match (number(&elevated), number(&compact), number(&restart)) {
        (Some(e), Some(c), Some(r)) => e > 0.0 && e < c && c < r && r <= 1.0,
        _ => false,
    };
    if !ordered || !count(&max_age) || !count(&cooldown) {
        return Err("invalid session thresholds".into());
    }
    let (elevated, compact, restart) = (
        number(&elevated).unwrap(),
        number(&compact).unwrap(),
        number(&restart).unwrap(),
    );
    let (max_age, cooldown) = (
        safe_integer(&max_age).unwrap(),
        safe_integer(&cooldown).unwrap(),
    );
    let empty = json!({});
    let task_value = or(field("task"), &empty);
    let task = fields(task_value, TASK_FIELDS, "task")?;
    let declared = |name: &str| task.get(name);
    for name in TASK_TEXTS {
        if declared(name).is_some_and(|v| !text(v)) {
            return Err(format!("invalid task {name}"));
        }
    }
    let is = |name: &str, values: &[&str]| {
        declared(name).is_none_or(|v| v.as_str().is_some_and(|s| values.contains(&s)))
    };
    if !is("stage", ADVICE_STAGES) {
        return Err("invalid task stage".into());
    }
    if declared("verificationMode").is_some()
        && (declared("stage") != Some(&json!("verify"))
            || !is("verificationMode", &["manual", "automation", "test-code"]))
    {
        return Err("invalid task verificationMode".into());
    }
    if !is(
        "status",
        &["working", "completed", "failed", "blocked", "unknown"],
    ) {
        return Err("invalid task status".into());
    }
    if !is("verification", &["passed", "failed", "pending", "unknown"]) {
        return Err("invalid verification".into());
    }
    for name in TASK_FLAGS {
        if declared(name).is_some_and(|v| !v.is_boolean()) {
            return Err(format!("invalid task {name}"));
        }
    }
    if declared("risks").is_some_and(|r| {
        !r.as_array()
            .is_some_and(|r| r.len() <= 16 && r.iter().all(text))
    }) {
        return Err("invalid task risks".into());
    }
    if declared("files").is_some_and(|f| {
        !f.as_array()
            .is_some_and(|f| f.len() <= 100 && f.iter().all(Value::is_string))
    }) {
        return Err("invalid task files".into());
    }
    let files = declared("files")
        .and_then(Value::as_array)
        .map_or(&[][..], Vec::as_slice);
    let checks = routing::route(
        settings,
        &routing::Request {
            intent: Some("run-check"),
            files,
            risks: &[],
            host: "agents",
            delegate: false,
            task: None,
            verification_mode: None,
        },
    )?["checks"]
        .clone();
    let host = fields(
        or(field("host"), &empty),
        &[
            "name",
            "model",
            "reliableCompaction",
            "windowTokens",
            "budgetTokens",
            "otherModels",
        ],
        "host",
    )?;
    for name in ["name", "model"] {
        if host.get(name).is_some_and(|v| !text(v)) {
            return Err(format!("invalid host {name}"));
        }
    }
    let tokens = |value: &Value| safe_integer(value).filter(|n| *n > 0);
    for name in ["windowTokens", "budgetTokens"] {
        if host.get(name).is_some_and(|v| tokens(v).is_none()) {
            return Err(format!("invalid host {name}"));
        }
    }
    let others = match host.get("otherModels") {
        None => &[][..],
        Some(others) => others
            .as_array()
            .filter(|others| {
                others.len() <= OTHER_MODELS
                    && others.iter().all(|other| {
                        fields(other, &["model", "windowTokens"], "other model").is_ok()
                            && text(&other["model"])
                            && tokens(&other["windowTokens"]).is_some()
                    })
            })
            .ok_or("invalid host otherModels")?
            .as_slice(),
    };
    let window = host.get("windowTokens").and_then(tokens);
    let usage = usage(field("usage"), session, now, max_age, window)?;
    // A working budget is the consumer's own cost line, below the window; none is assumed.
    let budget = host.get("budgetTokens").and_then(tokens);
    let over_budget = usage["status"] == "observed"
        && budget.is_some_and(|b| usage["currentTokens"].as_i64().is_some_and(|c| c >= b));
    if host
        .get("reliableCompaction")
        .is_some_and(|v| !v.is_boolean())
    {
        return Err("invalid host reliableCompaction".into());
    }
    let activity = fields(or(field("activity"), &empty), ACTIVITY, "activity")?;
    if let Some((name, _)) = activity.iter().find(|(_, v)| !count(v)) {
        return Err(format!("invalid activity {name}"));
    }
    if activity.get("maxAttempts").and_then(safe_integer) == Some(0) {
        return Err("maxAttempts must be positive".into());
    }
    if let Some(pack) = field("packContext") {
        let pack = fields(
            pack,
            &["estimatedTokens", "skills", "references"],
            "pack context",
        )?;
        if let Some((name, _)) = pack.iter().find(|(_, v)| !count(v)) {
            return Err(format!("invalid pack context {name}"));
        }
    }
    let tally = |name: &str| activity.get(name).and_then(safe_integer).unwrap_or(0);
    let pressure = match usage["usedRatio"]
        .as_f64()
        .filter(|_| usage["status"] == "observed")
    {
        None => "unknown",
        Some(r) if r >= restart => "saturated",
        Some(r) if r >= compact => "high",
        Some(r) if r >= elevated => "elevated",
        Some(_) => "healthy",
    };
    let exhausted =
        activity.contains_key("maxAttempts") && tally("attempts") >= tally("maxAttempts");
    let repeated_discovery = (tally("repeatedLookups") - tally("reuseHits")).max(0);
    let looping = tally("noProgressAttempts") >= 3;
    let flag = |name: &str| declared(name) == Some(&json!(true));
    let text_of = |name: &str| declared(name).and_then(Value::as_str);
    let boundary = flag("safeBoundary")
        || (declared("safeBoundary").is_none() && text_of("status") == Some("completed"));
    let checkpoint_ready = flag("handoffSaved") && flag("checkpointFresh");
    let reset = || {
        if checkpoint_ready {
            suggest(
                "start-new-session",
                "A fresh continuation checkpoint is declared saved; resume from it and current repository evidence.",
                "session-reset",
                &["Read checkpoint, inspect current diff and route only necessary guidance."],
                json!({"approvalRequired": true, "execution": "not-observed"}),
            )
        } else {
            suggest(
                "save-handoff",
                "Preserve a small continuation checkpoint before replacing session context.",
                "local-state-write",
                &[
                    "Save goal, state, completed work, approach, evidence identifiers, changed paths, validation, remaining work, blockers and next step.",
                ],
                json!({"afterCheckpoint": "start-new-session"}),
            )
        }
    };
    let stage = text_of("stage");
    let built = ["implement", "verify"].contains(&stage.unwrap_or_default());
    let mut suggestion = if exhausted || looping {
        Some(suggest(
            "return-control",
            "The declared attempt budget is exhausted or three attempts made no progress.",
            "read",
            &[
                "Preserve failure evidence. Use model-advice for evidence-bound escalation; do not repeat the same attempt.",
            ],
            json!({}),
        ))
    } else if text_of("status") == Some("failed") || text_of("verification") == Some("failed") {
        Some(suggest(
            "inspect-failure",
            "Preserve and diagnose the observed failure before rerunning or advancing.",
            "read",
            &[],
            json!({}),
        ))
    } else if text_of("status") == Some("blocked") {
        Some(suggest(
            "resolve-blocker",
            "A recorded prerequisite is missing; identify it without inventing completion.",
            "read",
            &[],
            json!({}),
        ))
    } else if flag("goalComplete") {
        Some(suggest(
            "summarize-evidence",
            "The task is declared complete; return the outcome and evidence.",
            "read",
            &[],
            json!({}),
        ))
    } else if ["high", "saturated"].contains(&pressure) {
        // Listed models whose window holds more than the current one.
        let capacity = usage["capacityTokens"].as_i64().unwrap_or(i64::MAX);
        let larger: Vec<&Value> = others
            .iter()
            .filter(|other| other["windowTokens"].as_i64().is_some_and(|w| w > capacity))
            .collect();
        let mut suggestion = if !boundary || flag("almostComplete") {
            let at_boundary = if pressure == "saturated" {
                "checkpoint-then-reconsider-context"
            } else {
                "reconsider-compaction"
            };
            suggest(
                "continue-bounded-step",
                "Finish the useful bounded step before changing context at a safe boundary.",
                "read",
                &[],
                json!({"atBoundary": at_boundary}),
            )
        } else if pressure == "saturated"
            && (truthy(declared("phaseChanged"))
                || truthy(declared("staleAssumptions"))
                || (truthy(declared("compacted")) && repeated_discovery >= 3))
        {
            reset()
        } else if host.get("reliableCompaction") == Some(&json!(true)) {
            if checkpoint_ready {
                suggest(
                    "compact",
                    "Context pressure is high and a safe task boundary is available.",
                    "session-context-change",
                    &["Use the host compaction control; retain the continuation checkpoint."],
                    json!({"approvalRequired": true, "execution": "not-observed"}),
                )
            } else {
                suggest(
                    "save-handoff",
                    "Save continuation evidence at this boundary before compaction.",
                    "local-state-write",
                    &[],
                    json!({"afterCheckpoint": "compact"}),
                )
            }
        } else {
            reset()
        };
        if !larger.is_empty() {
            suggestion["largerWindowModels"] = json!(larger);
        }
        Some(suggestion)
    } else if over_budget {
        Some(if !boundary || flag("almostComplete") {
            suggest(
                "continue-bounded-step",
                "Finish the useful bounded step; context is past the working budget.",
                "read",
                &[],
                json!({"atBoundary": "resume-from-checkpoint"}),
            )
        } else {
            let mut restart = reset();
            restart["reason"] = json!(if checkpoint_ready {
                "Context is past the working budget and every turn resends it; resume from the fresh checkpoint in a new session."
            } else {
                "Context is past the working budget and every turn resends it; save a continuation checkpoint, then resume in a new session."
            });
            restart
        })
    } else if tally("noProgressAttempts") > 0 {
        Some(suggest(
            "retry-differently",
            "A no-progress attempt is recorded; inspect evidence and change the approach before retrying.",
            "read",
            &[],
            json!({}),
        ))
    } else if text_of("status") == Some("completed") && !flag("goalComplete") {
        let verified = text_of("verification") == Some("passed")
            && truthy(declared("revision"))
            && declared("verificationRevision") == declared("revision");
        if built && !verified {
            let checks_known = checks.as_array().is_some_and(|c| !c.is_empty());
            Some(
                if text_of("verificationMode") != Some("manual") && checks_known {
                    suggest(
                        "run-check",
                        "Implementation is recorded complete; affected declared checks still need evidence.",
                        "command-execution",
                        &[
                            "Original task authorization and current command/environment policy still apply.",
                        ],
                        json!({"checks": checks}),
                    )
                } else {
                    suggest(
                        "design-verification",
                        "Verification for the current target still needs an appropriate check or observation.",
                        "read",
                        &[],
                        json!({"skill": "craft-engineering:workflow-verify-feature"}),
                    )
                },
            )
        } else if built && verified && flag("reviewRequested") {
            Some(suggest(
                "review",
                "Verification is recorded passed and review was explicitly requested.",
                "read",
                &["Review the same revision covered by the evidence."],
                json!({"skill": "craft-review:review-change"}),
            ))
        } else if built
            && verified
            && declared("risks")
                .and_then(Value::as_array)
                .is_some_and(|r| !r.is_empty())
            && settings["efficiency"]["review"]["mode"] != "manual"
        {
            Some(suggest(
                "review",
                "Current acceptance is verified and declared change risks justify an additional review.",
                "recommendation-only",
                &[
                    "Ask through the supported user-question mechanism unless the exact review scope is already authorized.",
                ],
                json!({"approvalRequired": true, "execution": "not-observed"}),
            ))
        } else if let Some(next) = text_of("nextStep") {
            Some(suggest(
                "continue-task",
                next,
                "recommendation-only",
                &["Continue only within existing task scope and authorization."],
                json!({}),
            ))
        } else if stage != Some("plan") {
            Some(suggest(
                "summarize-evidence",
                "Return the completed outcome, acceptance evidence and remaining limits.",
                "read",
                &[],
                json!({}),
            ))
        } else {
            None
        }
    } else {
        None
    };
    let previous = match field("previous") {
        Some(previous) => {
            let fields = fields(previous, &["key", "at"], "previous suggestion")?;
            let at = fields
                .get("at")
                .and_then(Value::as_str)
                .and_then(timestamp::parse);
            if !fields.get("key").is_some_and(text) || at.is_none() {
                return Err("invalid previous suggestion".into());
            }
            Some((previous, at.unwrap()))
        }
        None => None,
    };
    let advice_settings = &settings["efficiency"]["advice"];
    if advice_settings["enabled"] == false || advice_settings["nextStep"] == false {
        suggestion = None;
    }
    if context["action"] == "manual"
        && suggestion.as_ref().is_some_and(|s| {
            ["compact", "start-new-session", "save-handoff"]
                .iter()
                .any(|a| s["action"] == *a)
        })
    {
        suggestion = None;
    }
    let key = suggestion
        .as_ref()
        .map(|s| notice_key(session, task_value, s));
    let suppressed = match (&key, previous) {
        (Some(key), Some((previous, at))) => {
            previous["key"] == key.as_str() && now >= at && now - at < cooldown
        }
        _ => false,
    };
    let notice = match (&key, previous) {
        (Some(_), Some((previous, _))) if suppressed => previous.clone(),
        (Some(key), _) => json!({"key": key, "at": timestamp::format(now)}),
        (None, _) => Value::Null,
    };
    let mut pack = json!({"scope": "caller-declared selected guidance; estimated tokens, not occupancy", "estimatedTokens": null, "skills": null, "references": null});
    for (name, value) in field("packContext")
        .and_then(Value::as_object)
        .into_iter()
        .flatten()
    {
        pack[name] = value.clone();
    }
    let mut recorded =
        json!({"scope": "caller-declared Craft Capabilities activity; not provider usage"});
    for name in ACTIVITY {
        recorded[*name] = activity.get(*name).cloned().unwrap_or(Value::Null);
    }
    let operation = or(
        declared("operation"),
        or(declared("stage"), &json!("unknown")),
    )
    .clone();
    let suggestion = suggestion.unwrap_or(Value::Null);
    let mut status = json!({
        "formatVersion": 1, "mode": "recommend", "usage": usage, "pressure": pressure, "operation": operation,
        "host": {
            "name": or(host.get("name"), &json!("unknown")), "model": or(host.get("model"), &json!("unknown")),
            "reliableCompaction": or(host.get("reliableCompaction"), &Value::Null),
        },
        "packContext": pack, "activity": recorded, "recommendation": suggestion,
        "suggestion": if suppressed { Value::Null } else { suggestion.clone() }, "suppressed": suppressed, "notice": notice,
        "authorization": "advice-only; no command, compaction, reset or checkpoint write was executed",
        "evidence": "Task completion and verification are caller declarations, not inferred or independently certified.",
    });
    for name in ["windowTokens", "budgetTokens"] {
        if let Some(value) = host.get(name) {
            status["host"][name] = value.clone();
        }
    }
    if let Some(budget) = budget {
        let known = usage["status"] == "observed";
        status["budget"] = json!({"budgetTokens": budget, "exceeded": if known { json!(over_budget) } else { Value::Null }});
    }
    Ok(status)
}

/// The observations a status request needs: a routed guidance plan for its stage, whether the
/// saved continuation must be compared, and whether model advice needs a repository snapshot.
pub struct Needs {
    pub plan: Option<std::result::Result<Value, String>>,
    pub recover: bool,
    pub repository: bool,
}

/// The routing intent each task stage selects for guidance accounting.
fn stage_intent(stage: &str) -> &str {
    match stage {
        "architecture" => "investigate",
        "discover" => "locate",
        other => other,
    }
}

pub fn needs(input: &Value, settings: &Value, state: Option<&Value>, saved: &Value) -> Needs {
    let task = &input["task"];
    let last = state
        .and_then(|s| s["attempts"].as_array())
        .and_then(|a| a.last());
    let operation = [
        task.get("operation"),
        task.get("stage"),
        last.and_then(|l| l.get("operation")),
    ]
    .into_iter()
    .flatten()
    .find(|v| !v.is_null());
    let operation_text = operation.and_then(Value::as_str);
    let stage = match task.get("stage").filter(|s| !s.is_null()) {
        Some(stage) => Some(stage),
        None => operation.filter(|_| role(operation_text).is_some()),
    };
    let role = role(operation_text).or_else(|| role(stage.and_then(Value::as_str)));
    let id = task.get("id").filter(|id| truthy(*id));
    let same_task = id.is_some_and(|id| {
        saved.get("taskId") == Some(id) && saved["continuationSource"].get("taskId") == Some(id)
    });
    let plan = stage.filter(|s| truthy(*s)).map(|stage| {
        let files = match task.get("files").filter(|f| !f.is_null()) {
            None => &[][..],
            Some(files) => files
                .as_array()
                .map(Vec::as_slice)
                .ok_or("invalid task files")?,
        };
        let risks = match task.get("risks").filter(|r| !r.is_null()) {
            None => Vec::new(),
            Some(risks) => risks
                .as_array()
                .and_then(|r| {
                    r.iter()
                        .map(|r| r.as_str().map(str::to_string))
                        .collect::<Option<Vec<_>>>()
                })
                .ok_or("invalid capability operation/risks")?,
        };
        let intent = stage.as_str().map(stage_intent).unwrap_or("undefined");
        routing::route(
            settings,
            &routing::Request {
                intent: Some(intent),
                files,
                risks: &risks,
                host: input["host"]["name"].as_str().unwrap_or("agents"),
                delegate: false,
                task: None,
                verification_mode: task["verificationMode"].as_str(),
            },
        )
    });
    let attempted = state
        .and_then(|s| s["attempts"].as_array())
        .is_some_and(|a| !a.is_empty());
    let profiled = role.is_some_and(|r| truthy(settings["execution"]["roles"][r].get("profile")));
    Needs {
        plan,
        recover: truthy(saved.get("continuation")) && same_task,
        repository: attempted
            && truthy(operation)
            && id.is_some()
            && truthy(task.get("revision"))
            && truthy(input["host"].get("name"))
            && profiled,
    }
}

/// What the shell observed for a status request; an error replaces its observation.
#[derive(Default)]
pub struct Observations {
    pub state: Option<Value>,
    pub state_error: Option<String>,
    /// The saved checkpoint record, or null.
    pub saved: Value,
    /// The continuation comparison, when the saved continuation matches the task.
    pub recovered: Option<Value>,
    pub checkpoint_error: Option<String>,
    pub repository: Option<Value>,
    pub pack: Option<Value>,
    pub plan: Option<Value>,
    pub pack_error: Option<String>,
    /// The bundled context limits of the observed host, by exact model name.
    pub host_limits: Option<Value>,
}

/// At most this many other models' windows are compared.
const OTHER_MODELS: usize = 16;

/// The observed model's window and budget, the project's declaration over the bundled table
/// field by field, and, for a listed model, the other listed models' windows. Declared host
/// facts take precedence.
fn context_limits(host: &mut Map<String, Value>, settings: &Value, bundled: Option<&Value>) {
    let (Some(name), Some(model)) = (
        host.get("name").and_then(Value::as_str),
        host.get("model").and_then(Value::as_str),
    ) else {
        return;
    };
    let project = &settings["execution"]["hosts"][name]["contextLimits"];
    let tables = [project, bundled.unwrap_or(&Value::Null)];
    let limit = |id: &str, field: &str| tables.iter().find_map(|t| t[id].get(field).cloned());
    let mut facts = Map::new();
    for field in ["windowTokens", "budgetTokens"] {
        if let Some(value) = limit(model, field) {
            facts.insert(field.into(), value);
        }
    }
    let ids: std::collections::BTreeSet<&String> = tables
        .iter()
        .flat_map(|t| t.as_object().into_iter().flatten().map(|(id, _)| id))
        .filter(|id| *id != model)
        .collect();
    let mut others: Vec<(&String, Value)> = ids
        .into_iter()
        .filter_map(|id| limit(id, "windowTokens").map(|w| (id, w)))
        .collect();
    // Only larger windows are ever named, so the largest are the ones worth keeping.
    others.sort_by_key(|(id, w)| (std::cmp::Reverse(w.as_i64()), *id));
    others.truncate(OTHER_MODELS);
    others.sort_by_key(|(id, _)| *id);
    let others: Vec<Value> = others
        .into_iter()
        .map(|(id, w)| json!({"model": id, "windowTokens": w}))
        .collect();
    // An unlisted model has no declared place among the others; nothing is compared.
    if !facts.is_empty() && !others.is_empty() {
        facts.insert("otherModels".into(), json!(others));
    }
    for (field, value) in facts {
        host.entry(field).or_insert(value);
    }
}

pub struct Request<'a> {
    pub input: &'a Value,
    pub settings: &'a Value,
    pub session: Option<&'a str>,
    pub task: Option<&'a str>,
    pub now: i64,
    pub observations: &'a Observations,
}

/// `{...value}`: an object's fields, a string's or array's indices, or nothing.
fn spread(value: &Value) -> Map<String, Value> {
    match value {
        Value::Object(map) => map.clone(),
        Value::Array(items) => items
            .iter()
            .enumerate()
            .map(|(i, v)| (i.to_string(), v.clone()))
            .collect(),
        Value::String(text) => (0..text.encode_utf16().count())
            .map(|i| (i.to_string(), Value::Null))
            .collect(),
        _ => Map::new(),
    }
}

fn source(repository: Option<&Value>) -> Value {
    repository.map_or(Value::Null, super::source)
}

/// The session and task identities: an explicit one wins and must agree with a truthy
/// declared one in `input`; otherwise the declared one, if any.
pub fn identities(
    input: &Value,
    session: Option<&str>,
    task: Option<&str>,
) -> Result<(Option<Value>, Option<Value>)> {
    let declared_session = input.get("sessionId");
    let declared_task = input["task"].get("id");
    if session.is_some_and(|s| !s.is_empty())
        && truthy(declared_session)
        && declared_session != Some(&json!(session))
    {
        return Err("session input identity mismatch".into());
    }
    if task.is_some_and(|t| !t.is_empty())
        && truthy(declared_task)
        && declared_task != Some(&json!(task))
    {
        return Err("task input identity mismatch".into());
    }
    let session_id = session
        .map(|s| json!(s))
        .or_else(|| declared_session.cloned());
    let task_id = task.map(|t| json!(t)).or_else(|| declared_task.cloned());
    Ok((session_id, task_id))
}

/// Unified status: advice over the declared task merged with recorded activity, checkpoint
/// freshness, routed guidance, capability and model advice, blockers and recorded offers.
pub fn status(request: Request) -> Result<Value> {
    let Request {
        input,
        settings,
        session,
        task,
        now,
        observations,
    } = request;
    let (session_id, task_id) = identities(input, session, task)?;
    let mut normalized = input.as_object().cloned().unwrap_or_default();
    if let Some(id) = session_id.as_ref().filter(|s| truthy(*s)) {
        normalized.insert("sessionId".into(), id.clone());
    }
    let mut declared = spread(&input["task"]);
    if let Some(id) = task_id.as_ref().filter(|t| truthy(*t)) {
        declared.insert("id".into(), id.clone());
    }
    let mut diagnostics: Vec<String> = Vec::new();
    let mut activity: Option<Map<String, Value>> = None;
    let mut state = None;
    let mut attempts = None;
    let bound = truthy(session_id.as_ref()) && truthy(task_id.as_ref());
    if bound {
        let observed = match (&observations.state_error, &observations.state) {
            (Some(error), _) => Err(error.clone()),
            (None, None) => Err("No activity observation.".to_string()),
            (None, Some(observed)) => Ok(observed),
        };
        match observed {
            Err(error) => diagnostics.push(format!("Activity unavailable: {error}")),
            Ok(observed) => {
                state = Some(observed);
                let records = observed["attempts"]
                    .as_array()
                    .map_or(&[][..], Vec::as_slice);
                let operation = [
                    declared.get("operation"),
                    declared.get("stage"),
                    records.last().and_then(|r| r.get("operation")),
                ]
                .into_iter()
                .flatten()
                .find(|v| !v.is_null())
                .cloned();
                if let Some(operation) = operation.as_ref().filter(|o| truthy(*o)) {
                    declared.insert("operation".into(), operation.clone());
                }
                let operation_text = operation.as_ref().and_then(Value::as_str);
                if !truthy(declared.get("stage")) && role(operation_text).is_some() {
                    declared.insert("stage".into(), operation.clone().unwrap());
                }
                if !records.is_empty() && truthy(operation.as_ref()) {
                    attempts = Some(attempt_status(records, operation_text.unwrap_or_default()));
                }
                let mut merged = spread(&input["activity"]);
                for (key, value) in observed["activity"].as_object().into_iter().flatten() {
                    merged.insert(key.clone(), value.clone());
                }
                let repetitions = &observed["lastDiscovery"]["repetitions"];
                if ["locate", "discover"].contains(&operation_text.unwrap_or_default())
                    && repetitions.as_f64().is_some_and(|r| r >= 3.0)
                {
                    merged.insert("noProgressAttempts".into(), repetitions.clone());
                }
                if let Some(attempts) = &attempts {
                    let previous = merged
                        .get("noProgressAttempts")
                        .and_then(Value::as_f64)
                        .unwrap_or(0.0);
                    let no_progress = attempts["noProgressAttempts"]
                        .as_f64()
                        .unwrap_or(0.0)
                        .max(previous);
                    merged.insert("attempts".into(), attempts["attempts"].clone());
                    merged.insert("maxAttempts".into(), attempts["maxAttempts"].clone());
                    merged.insert("noProgressAttempts".into(), ratio(no_progress));
                    merged.insert("retries".into(), attempts["retries"].clone());
                    if attempts["blocked"] == true {
                        declared.insert("status".into(), json!("blocked"));
                    }
                }
                activity = Some(merged);
            }
        }
    }
    let unmatched = json!({"saved": observations.saved, "changed": null, "changeDetection": {"complete": false}});
    let mut repository_evidence: Option<&Value> = None;
    let mut checkpoint = json!({"status": "unknown", "reason": "No session checkpoint selected."});
    if truthy(session_id.as_ref()) {
        if let Some(error) = &observations.checkpoint_error {
            declared.insert("handoffSaved".into(), json!(false));
            declared.insert("checkpointFresh".into(), json!(false));
            diagnostics.push(format!("Checkpoint unavailable: {error}"));
        } else {
            let saved = &observations.saved;
            let continuation = saved.get("continuation").filter(|c| truthy(*c));
            let binding = &saved["continuationSource"];
            let same_task = truthy(task_id.as_ref())
                && saved.get("taskId") == task_id.as_ref()
                && binding.get("taskId") == task_id.as_ref();
            let recovered = match (continuation, &observations.recovered) {
                (Some(_), Some(recovered)) if same_task => recovered,
                _ => &unmatched,
            };
            let current = recovered.get("current");
            repository_evidence = current;
            let at = |name: &str| current.and_then(|c| c.get(name));
            let print = |name: &str| current.and_then(|c| c["fingerprints"].get(name));
            let record = state
                .and_then(|s| s.get("taskRecord"))
                .filter(|r| truthy(*r));
            let unchanged = binding["complete"] == true
                && match record {
                    None => true,
                    Some(record) => {
                        binding.get("taskContract") == Some(&contract(Some(record))?)
                            && record.get("checkout") == at("checkout")
                    }
                }
                && or(binding.get("checkout"), &Value::Null) == or(at("checkout"), &Value::Null)
                && at("head") == binding.get("head")
                && print("index") == binding.get("index")
                && print("workingTree") == binding.get("workingTree");
            let complete = recovered["changeDetection"].get("complete");
            let handoff = continuation.is_some() && same_task;
            let fresh = handoff && unchanged && recovered["changed"] == false && truthy(complete);
            declared.insert("handoffSaved".into(), json!(handoff));
            declared.insert("checkpointFresh".into(), json!(fresh));
            let status = if fresh {
                "fresh"
            } else if truthy(recovered.get("saved")) {
                "stale-or-incomplete"
            } else {
                "missing"
            };
            let reason = if fresh {
                "Task identity and complete repository fingerprints match the saved continuation."
            } else {
                "A matching bounded continuation and unchanged complete repository evidence are required."
            };
            let next = if same_task {
                or(continuation.and_then(|c| c.get("nextStep")), &Value::Null).clone()
            } else {
                Value::Null
            };
            let mut view = Map::new();
            view.insert("status".into(), json!(status));
            view.insert(
                "fileRevision".into(),
                or(recovered["saved"].get("head"), &Value::Null).clone(),
            );
            view.insert("taskMatches".into(), json!(same_task));
            if let Some(changed) = recovered.get("changed") {
                view.insert("changed".into(), changed.clone());
            }
            if let Some(complete) = complete {
                view.insert("complete".into(), complete.clone());
            }
            view.insert("nextStep".into(), next);
            view.insert("reason".into(), json!(reason));
            checkpoint = Value::Object(view);
        }
    } else {
        // A bare caller declaration cannot establish that a safe continuation exists on disk.
        declared.insert("handoffSaved".into(), json!(false));
        declared.insert("checkpointFresh".into(), json!(false));
    }
    let mut pack = None;
    if truthy(declared.get("stage")) {
        let selected = match (
            &observations.pack_error,
            &observations.pack,
            &observations.plan,
        ) {
            (Some(error), _, _) => Err(error.clone()),
            (None, Some(pack), Some(plan)) => Ok((pack, plan)),
            _ => Err("No guidance observation.".to_string()),
        };
        match selected {
            Err(error) => diagnostics.push(format!("Pack selection unavailable: {error}")),
            Ok((observed, plan)) => {
                let unique = |name: &str| {
                    let mut items: Vec<&Value> = Vec::new();
                    for item in plan[name].as_array().into_iter().flatten() {
                        if !items.contains(&item) {
                            items.push(item);
                        }
                    }
                    items.len()
                };
                normalized.insert(
                    "packContext".into(),
                    json!({"estimatedTokens": observed["totals"]["estimatedTokens"], "skills": unique("skills"), "references": unique("references")}),
                );
                pack = Some(observed);
            }
        }
    }
    if let Some(activity) = activity {
        normalized.insert("activity".into(), Value::Object(activity));
    }
    normalized.insert("task".into(), Value::Object(declared.clone()));
    if let Some(Value::Object(host)) = normalized.get_mut("host") {
        context_limits(host, settings, observations.host_limits.as_ref());
    }
    let mut status = advice(&Value::Object(normalized.clone()), settings, now)?;
    if let Some(observed) = pack {
        status["packContext"]["scope"] = json!("routed selected files; not observed host loads");
        status["packContext"]["source"] = json!("context-report");
        status["packContext"]["complete"] = observed["complete"].clone();
    }
    if state.is_some() {
        status["activity"]["scope"] = json!(
            "Craft Capabilities command receipts plus declarations for unobserved fields; not provider usage"
        );
    }
    let task = Value::Object(declared);
    let operation = or(
        task.get("operation"),
        or(task.get("stage"), &json!("unknown")),
    )
    .as_str()
    .unwrap_or("unknown")
    .to_string();
    let risks: Vec<String> = task["risks"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|r| r.as_str().map(str::to_string))
        .collect();
    let mut capability = routing::capability(&operation, &risks);
    let host = &normalized.get("host").cloned().unwrap_or(Value::Null);
    let models =
        host["name"].as_str().map_or(
            &Value::Null,
            |name| &settings["execution"]["hosts"][name]["models"],
        );
    let current_profile = match host.get("model").filter(|m| truthy(*m)) {
        Some(model) => {
            let matching: Vec<&String> = models
                .as_object()
                .into_iter()
                .flatten()
                .filter(|(_, name)| *name == model)
                .map(|(profile, _)| profile)
                .collect();
            (matching.len() == 1).then(|| matching[0].clone())
        }
        None => None,
    };
    let current_class = current_profile
        .as_ref()
        .and_then(|profile| settings["execution"]["profiles"][profile].get("class"))
        .filter(|c| !c.is_null())
        .cloned()
        .unwrap_or(json!("unknown"));
    let economical = capability["class"] == "economical";
    let recommendation = if economical && ["standard", "strong"].iter().any(|c| current_class == *c)
    {
        json!("use-deterministic-tool-or-economical-profile")
    } else {
        Value::Null
    };
    capability["currentProfile"] = current_profile.clone().map_or(Value::Null, Value::String);
    capability["currentClass"] = current_class;
    capability["recommendation"] = recommendation;
    let role = role(task["operation"].as_str()).or_else(|| {
        if economical {
            Some("locator")
        } else {
            role(task["stage"].as_str())
        }
    });
    let records = attempts
        .as_ref()
        .and_then(|a| a["records"].as_array())
        .map_or(&[][..], Vec::as_slice);
    let attempted = attempts
        .as_ref()
        .and_then(|a| a["attempts"].as_u64())
        .unwrap_or(0)
        > 0;
    let mut model = Value::Null;
    if let Some(role) = role
        && truthy(task_id.as_ref())
        && truthy(task.get("revision"))
        && truthy(host.get("name"))
    {
        let mut current_source = Value::Null;
        if !records.is_empty() && truthy(settings["execution"]["roles"][role].get("profile")) {
            repository_evidence = repository_evidence.or(observations.repository.as_ref());
            current_source = source(repository_evidence);
        }
        let revision = &task["revision"];
        let cited: Vec<Value> = records
            .iter()
            .filter(|r| {
                truthy(r.get("profile"))
                    && !current_source.is_null()
                    && r["source"] == current_source
                    && &r["revision"] == revision
                    && !["blocked", "prerequisite-resolved"].iter().any(|o| r["outcome"] == *o)
            })
            .map(|r| json!({"id": r["id"], "profile": r["profile"], "task": task_id, "revision": r["revision"], "outcome": r["outcome"], "evidence": r["evidence"]}))
            .collect();
        let mut advice = json!({"host": host["name"], "role": role, "task": task_id, "revision": revision, "capabilityClass": capability["class"]});
        if let Some(profile) = &current_profile {
            advice["currentProfile"] = json!(profile);
        }
        advice["attempts"] = json!(cited);
        model = models::advise(settings, &advice)?;
    }
    let blocked = attempts.as_ref().is_some_and(|a| a["blocked"] == true);
    let stopped = model["decision"] == "stop" && attempted;
    let last_profile = records
        .last()
        .and_then(|r| r.get("profile"))
        .filter(|p| truthy(*p));
    let reasons = || {
        model["reasons"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .collect::<Vec<_>>()
            .join(" ")
    };
    if blocked {
        status["recommendation"] = json!({"action": "stop-blocked", "reason": "A recorded permission, tool or prerequisite blocker requires resolution.", "effect": "read", "prerequisites": []});
    } else if stopped {
        status["recommendation"] = json!({"action": "return-control", "reason": reasons(), "effect": "read", "prerequisites": ["Configured quality/escalation limits are exhausted; preserve evidence and return control."]});
    } else if ["return-control", "retry-differently"]
        .iter()
        .any(|a| status["recommendation"]["action"] == *a)
        && model["decision"] == "recommend"
        && last_profile.is_some_and(|p| model.get("profile") != Some(p))
        && (model["mainModel"]["switchSupported"] == true
            || model["workerMappingAvailable"] == true)
    {
        status["recommendation"] = json!({"action": "escalate-model", "reason": reasons(), "effect": "recommendation-only", "prerequisites": ["Use only the configured host mapping and existing authorization; no switch was performed."]});
    }
    let action = status["recommendation"]["action"].clone();
    if ["stop-blocked", "escalate-model"]
        .iter()
        .any(|a| action == *a)
        || stopped
    {
        let key = notice_key(session_id.as_ref(), &task, &status["recommendation"]);
        let previous = input.get("previous");
        let at = previous
            .and_then(|p| p["at"].as_str())
            .and_then(timestamp::parse);
        let cooldown = input["policy"]
            .get("cooldownMs")
            .and_then(Value::as_f64)
            .unwrap_or(DEFAULT_WINDOW_MS as f64);
        let suppressed = previous.is_some_and(|p| p["key"] == key.as_str())
            && at.is_some_and(|at| now >= at && ((now - at) as f64) < cooldown);
        status["suppressed"] = json!(suppressed);
        status["notice"] = if suppressed {
            previous.cloned().unwrap()
        } else {
            json!({"key": key, "at": timestamp::format(now)})
        };
    }
    if ["review", "compact"].iter().any(|a| action == *a)
        && let Some(offers) = state
            .and_then(|s| s.get("recommendations"))
            .and_then(Value::as_object)
    {
        repository_evidence = repository_evidence.or(observations.repository.as_ref());
        let current = source(repository_evidence);
        let policy = json!(digest(settings));
        let risks = json!(super::risks(&task["risks"]));
        let review = action == "review";
        let offer = offers.values().rev().find(|offer| {
            offer["action"] == action
                && (!review || offer["scope"] == "review:current-source")
                && offer.get("source") == Some(&current)
                && offer.get("policy") == Some(&policy)
                && (!review || or(offer.get("risks"), &json!([])) == &risks)
        });
        if let Some(offer) = offer {
            let decision = offer.get("decision").cloned().unwrap_or(Value::Null);
            status["actionOffer"] = offer.clone();
            status["suppressed"] = json!(true);
            status["recommendation"]["approvalRequired"] = json!(
                !["approved", "already-authorized"]
                    .iter()
                    .any(|d| decision == *d)
            );
            status["recommendation"]["decision"] = decision;
            status["recommendation"]["execution"] =
                or(offer.get("execution"), &json!("not-observed")).clone();
        }
    }
    status["suggestion"] = if status["suppressed"] == true {
        Value::Null
    } else {
        status["recommendation"].clone()
    };
    let discovery = match state {
        Some(state) => {
            let counter = |name: &str| or(state["activity"].get(name), &Value::Null).clone();
            json!({"requested": counter("discoveryRequests"), "reused": counter("reuseHits"), "fresh": counter("freshLookups"), "batches": counter("batchedOperations")})
        }
        None => Value::Null,
    };
    let evidence = attempts
        .as_ref()
        .map_or(json!([]), |a| a["evidence"].clone());
    let activity_source = if state.is_some() {
        "Craft Capabilities command receipts; explicit declarations fill unobserved fields"
    } else {
        "caller declarations; host-wide activity is unknown"
    };
    for (key, value) in [
        ("checkpoint", checkpoint),
        ("capability", capability),
        ("modelAdvice", model),
        ("discovery", discovery),
        ("packReport", pack.cloned().unwrap_or(Value::Null)),
        ("attemptEvidence", evidence),
        ("diagnostics", json!(diagnostics)),
        ("activitySource", json!(activity_source)),
    ] {
        status[key] = value;
    }
    Ok(status)
}
