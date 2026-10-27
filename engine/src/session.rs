//! Task-scoped session records: identities, validation, attempts and the bound task context.
use crate::values::{lower_hex, sha256, text, truthy};
use crate::{
    contracts::relative_file,
    digest, repository,
    routing::{member, private_path},
    timestamp,
    values::{js_length, safe_integer},
};
use serde_json::{Map, Value, json};

pub mod acceptance;
pub mod status;
pub mod step;
pub mod views;

type Result<T = ()> = std::result::Result<T, String>;

pub const BLOCKERS: &[&str] = &["permission-denied", "tool-unavailable", "blocked"];
pub const OUTCOMES: &[&str] = &[
    "passed",
    "quality-failure",
    "permission-denied",
    "tool-unavailable",
    "blocked",
    "prerequisite-resolved",
];
const ATTEMPT_FIELDS: &[&str] = &[
    "operation",
    "target",
    "approach",
    "evidence",
    "outcome",
    "profile",
    "maxAttempts",
    "escalationReason",
];
/// Saved attempts per task; also the most that may be open at once.
const ATTEMPT_CAPACITY: usize = 24;
pub const FEATURES: &[&str] = &[
    "routing",
    "hint-deduplication",
    "lookup",
    "batching",
    "services",
    "attempts",
    "loop-detection",
    "model-advice",
    "delegation",
    "checkpoints",
    "continuation",
    "context-advice",
    "next-step",
    "compaction",
    "context-accounting",
    "review",
    "telemetry",
    "evaluation",
];
const FEATURE_STATUSES: &[&str] = &[
    "used",
    "eligible-but-skipped",
    "disabled",
    "unsupported",
    "not-applicable",
    "failed",
];
/// Task stages and the routing intent each selects.
pub const STAGES: &[(&str, &str)] = &[
    ("plan", "plan"),
    ("implement", "implement"),
    ("verify", "verify"),
    ("review", "review"),
    ("locate", "locate"),
    ("explore", "verify"),
    ("architecture", "investigate"),
    ("story", "story"),
];

pub fn identity(session: &Value, task: &Value) -> Result {
    if text(session) && text(task) {
        Ok(())
    } else {
        Err("session activity requires bounded session and task identities".into())
    }
}

pub fn stage(value: &Value) -> Result<&'static str> {
    value
        .as_str()
        .and_then(|s| STAGES.iter().find(|(name, _)| *name == s))
        .map(|(name, _)| *name)
        .ok_or_else(|| "invalid task stage".into())
}

/// A new record for a session and task that has no saved state.
pub fn empty() -> Value {
    json!({"formatVersion": 1, "activity": {}, "lookups": {}, "attempts": []})
}

pub fn add_activity(state: &mut Value, values: &Map<String, Value>) -> Result {
    let activity = state["activity"]
        .as_object_mut()
        .ok_or("invalid session state")?;
    for (key, value) in values {
        let next = activity
            .get(key)
            .map_or(Some(0), safe_integer)
            .zip(safe_integer(value))
            .and_then(|(a, b)| a.checked_add(b))
            .filter(|n| (0..=9_007_199_254_740_991).contains(n))
            .ok_or("session counter overflow")?;
        activity.insert(key.clone(), json!(next));
    }
    Ok(())
}

/// Indices of attempts no later pass (or, for blockers, resolution) of the same
/// operation and target has closed.
pub fn unresolved(records: &[Value]) -> Vec<usize> {
    let key = |r: &Value| (r["operation"].to_string(), r["target"].to_string());
    let last = |outcome: &str, key_of: &(String, String)| {
        records
            .iter()
            .rposition(|r| r["outcome"] == outcome && &key(r) == key_of)
    };
    (0..records.len())
        .filter(|&index| {
            let record = &records[index];
            let outcome = record["outcome"].as_str().unwrap_or_default();
            if ["passed", "prerequisite-resolved"].contains(&outcome) {
                return false;
            }
            let k = key(record);
            let pass = last("passed", &k);
            let resolution = BLOCKERS
                .contains(&outcome)
                .then(|| last("prerequisite-resolved", &k))
                .flatten();
            pass.max(resolution).is_none_or(|closed| index > closed)
        })
        .collect()
}

/// A meaningful attempt as a receipt bound to the source it was observed at; attempts at the
/// same operation, target and approach against unchanged source share a signature.
pub fn attempt_receipt(attempt: &Value, repository: &Value, id: &str, at: &str) -> Result<Value> {
    let invalid = || "invalid meaningful attempt receipt".to_string();
    let fields = attempt.as_object().ok_or_else(invalid)?;
    let valid = fields
        .keys()
        .all(|key| ATTEMPT_FIELDS.contains(&key.as_str()))
        && ["operation", "target", "approach", "evidence"]
            .iter()
            .all(|key| fields.get(*key).is_some_and(text))
        && fields
            .get("outcome")
            .and_then(Value::as_str)
            .is_some_and(|outcome| OUTCOMES.contains(&outcome))
        && fields.get("profile").is_none_or(text)
        && fields
            .get("escalationReason")
            .is_none_or(|reason| text(reason) && fields.contains_key("profile"))
        && fields
            .get("maxAttempts")
            .is_none_or(|limit| safe_integer(limit).is_some_and(|n| (1..=5).contains(&n)));
    if !valid {
        return Err(invalid());
    }
    let source = source(repository);
    let signature = match &source {
        Value::Null => Value::Null,
        source => json!(digest(&json!([
            fields["operation"],
            fields["target"],
            fields["approach"],
            source
        ]))),
    };
    let mut receipt = fields.clone();
    receipt.insert("id".into(), json!(id));
    receipt.insert("source".into(), source);
    receipt.insert("signature".into(), signature);
    if let Some(head) = repository.get("head") {
        receipt.insert("revision".into(), head.clone());
    }
    receipt.insert("at".into(), json!(at));
    Ok(Value::Object(receipt))
}

/// Append a receipt. A resolution needs a matching open blocker; open records are never
/// evicted, so recording fails once more than the capacity would be open, and only closed
/// history (oldest first) makes room.
pub fn append_attempt(state: &mut Value, receipt: Value) -> Result {
    let mut records = state["attempts"]
        .as_array()
        .ok_or("invalid session state")?
        .clone();
    let same = |record: &Value| {
        record["operation"] == receipt["operation"] && record["target"] == receipt["target"]
    };
    if receipt["outcome"] == "prerequisite-resolved"
        && !unresolved(&records).into_iter().any(|index| {
            let record = &records[index];
            same(record) && BLOCKERS.contains(&record["outcome"].as_str().unwrap_or_default())
        })
    {
        return Err("prerequisite resolution requires a matching unresolved blocker".into());
    }
    let mut counters = Map::from_iter([("operationsAttempted".to_string(), json!(1))]);
    if receipt.get("escalationReason").is_some_and(truthy) {
        counters.insert("modelEscalations".into(), json!(1));
    }
    records.push(receipt);
    let open = unresolved(&records);
    if open.len() > ATTEMPT_CAPACITY {
        return Err("session attempt capacity reached: preserve evidence and resolve an active operation before recording more attempts".into());
    }
    add_activity(state, &counters)?;
    let mut discard = records.len().saturating_sub(ATTEMPT_CAPACITY);
    let mut index = 0;
    records.retain(|_| {
        let evict = discard > 0 && !open.contains(&index);
        discard -= usize::from(evict);
        index += 1;
        !evict
    });
    state["attempts"] = Value::Array(records);
    Ok(())
}

pub fn validate_features(records: Option<&Value>) -> Result {
    let Some(records) = records else {
        return Ok(());
    };
    let records = records
        .as_object()
        .filter(|r| r.len() <= FEATURES.len())
        .ok_or("invalid task feature outcomes")?;
    for (feature, record) in records {
        let valid = FEATURES.contains(&feature.as_str())
            && record.as_object().is_some_and(|r| {
                r.keys().all(|k| {
                    ["status", "reason", "evidence", "provenance", "source", "at"]
                        .contains(&k.as_str())
                })
            })
            && record["status"]
                .as_str()
                .is_some_and(|s| FEATURE_STATUSES.contains(&s))
            && text(&record["reason"])
            && text(&record["evidence"])
            && ["agent-declared", "runtime-observed"]
                .iter()
                .any(|p| record["provenance"] == *p)
            && text(&record["at"])
            && timestamp::parse(record["at"].as_str().unwrap_or_default()).is_some()
            && (record.get("source") == Some(&Value::Null)
                || record.get("source").is_some_and(sha256));
        if !valid {
            return Err("invalid feature outcome record".into());
        }
    }
    Ok(())
}

/// Save one feature outcome bound to `source` and return it. A declaration never replaces a
/// runtime-observed outcome; an observation replaces anything.
pub fn record_feature(
    state: &mut Value,
    input: &Value,
    source: &Value,
    at: &str,
    provenance: &str,
) -> Result<Value> {
    if !truthy(&state["taskRecord"]) {
        return Err("task feature outcomes require task-start".into());
    }
    let input = input
        .as_object()
        .filter(|input| {
            input
                .keys()
                .all(|k| ["feature", "status", "reason", "evidence"].contains(&k.as_str()))
        })
        .ok_or("unknown feature or invalid feature outcome")?;
    let feature = input
        .get("feature")
        .and_then(Value::as_str)
        .filter(|f| FEATURES.contains(f))
        .ok_or("unknown feature or invalid feature outcome")?;
    if provenance == "agent-declared"
        && state["featureOutcomes"]
            .get(feature)
            .map(|r| &r["provenance"])
            == Some(&json!("runtime-observed"))
    {
        return Err(
            "a runtime-observed feature outcome cannot be replaced by a declaration".into(),
        );
    }
    let mut next: Map<String, Value> = input
        .iter()
        .filter(|(k, _)| *k != "feature")
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();
    next.extend([
        ("source".to_string(), source.clone()),
        ("at".to_string(), json!(at)),
        ("provenance".to_string(), json!(provenance)),
    ]);
    let next = Value::Object(next);
    validate_features(Some(&json!({ feature: next })))?;
    validate_features(state.get("featureOutcomes"))?;
    state["featureOutcomes"][feature] = next.clone();
    let mut recorded = Map::from_iter([("feature".to_string(), json!(feature))]);
    recorded.extend(next.as_object().cloned().unwrap_or_default());
    Ok(Value::Object(recorded))
}

/// Every feature with its saved outcome against the current `source`, a runtime receipt that
/// implies use, or an unknown outcome.
pub fn feature_report(state: &Value, source: &Value) -> Result<Value> {
    validate_features(state.get("featureOutcomes"))?;
    let counter = |name: &str| state["activity"][name].as_f64().unwrap_or(0.0);
    let inferred = |feature: &str| match feature {
        "routing" if counter("routingDecisions") > 0.0 => Some((
            "Routing decisions recorded; resource loading remains unobserved.",
            "activity:routingDecisions",
        )),
        "lookup" if counter("freshLookups") + counter("reuseHits") > 0.0 => Some((
            "Task discovery receipts exist; inspect reuse/invalidation evidence separately.",
            "activity:discoveryRequests",
        )),
        "attempts" if counter("operationsAttempted") > 0.0 => Some((
            "Local attempt receipts exist; semantic acceptance is agent-declared.",
            "activity:operationsAttempted",
        )),
        _ => None,
    };
    let features: Map<String, Value> = FEATURES
        .iter()
        .map(|&feature| {
            let outcome = if let Some(saved) = state["featureOutcomes"].get(feature) {
                let status = match &saved["source"] {
                    Value::Null => "unknown",
                    _ if source.is_null() => "unknown",
                    saved if saved == source => "current",
                    _ => "stale",
                };
                let mut outcome = saved.clone();
                outcome["sourceStatus"] = json!(status);
                outcome
            } else if let Some((reason, evidence)) = inferred(feature) {
                json!({"status": "used", "reason": reason, "evidence": evidence, "provenance": "runtime-receipt", "sourceStatus": "historical"})
            } else {
                json!({"status": "unknown", "reason": "No outcome evidence retained for this feature.", "provenance": "unobserved", "sourceStatus": "unknown"})
            };
            (feature.to_string(), outcome)
        })
        .collect();
    let classified = features.values().all(|f| f["status"] != "unknown");
    Ok(json!({
        "features": features,
        "allClassified": classified,
        "limits": "Task-local observations/declarations, not a semantic verdict or proof of natural workflow adoption. Stale or historical outcomes do not certify current acceptance.",
    }))
}

/// The open retry series for `operation`: the latest blocker's target, else the latest open
/// or recorded one. Failures count per profile; equal signatures made no progress.
pub fn attempt_status(records: &[Value], operation: &str) -> Value {
    let current: Vec<Value> = records
        .iter()
        .filter(|r| r["operation"] == operation)
        .cloned()
        .collect();
    let open: Vec<&Value> = unresolved(&current)
        .into_iter()
        .map(|i| &current[i])
        .collect();
    let blocker = |r: &&Value| BLOCKERS.contains(&r["outcome"].as_str().unwrap_or_default());
    let selected = open
        .iter()
        .rev()
        .find(|r| blocker(r))
        .or(open.last())
        .copied()
        .or(current.last());
    let target = selected.map(|s| s.get("target"));
    let aimed = |r: &&Value| Some(r.get("target")) == target;
    let targeted: Vec<&Value> = current.iter().filter(aimed).collect();
    let series: Vec<&Value> = open.iter().copied().filter(aimed).collect();
    let last = series.last().or(targeted.last());
    let profile = last.and_then(|l| l.get("profile")).filter(|p| truthy(*p));
    let failures: Vec<&Value> = series
        .iter()
        .copied()
        .filter(|r| profile.is_none() || r.get("profile") == profile)
        .collect();
    let equivalent = match last.map(|l| &l["signature"]).filter(|s| truthy(*s)) {
        Some(signature) => failures
            .iter()
            .filter(|r| &r["signature"] == signature)
            .count(),
        None => 0,
    };
    let max_attempts = last
        .and_then(|l| l.get("maxAttempts"))
        .filter(|m| !m.is_null())
        .cloned()
        .unwrap_or(json!(3));
    let records: Vec<&Value> = if series.is_empty() {
        targeted.last().into_iter().copied().collect()
    } else {
        series.clone()
    };
    json!({
        "attempts": failures.len(),
        "maxAttempts": max_attempts,
        "noProgressAttempts": equivalent,
        "retries": failures.len().saturating_sub(1),
        "blocked": series.iter().any(blocker),
        "evidence": failures.iter().map(|r| r.get("evidence").cloned().unwrap_or(Value::Null)).collect::<Vec<_>>(),
        "records": records,
    })
}

fn recommendations(value: &Value) -> Result {
    let offers = value
        .as_object()
        .filter(|o| o.len() <= 16)
        .ok_or("invalid recommendation state")?;
    for (id, offer) in offers {
        if !lower_hex(id, 64) || offer["id"] != id.as_str() {
            return Err("invalid recommendation identity".into());
        }
        if offer.get("taskContract").is_some() && !offer.get("taskContract").is_some_and(sha256) {
            return Err("invalid recommendation task contract".into());
        }
        let Some(receipt) = offer.get("executionReceipt") else {
            continue;
        };
        let nullable = |field: &str| {
            receipt.get(field) == Some(&Value::Null) || receipt.get(field).is_some_and(text)
        };
        let primitive = |v: Option<&Value>| !v.is_some_and(|v| v.is_object() || v.is_array());
        let valid = receipt.as_object().is_some_and(|r| {
            r.keys().all(|k| {
                [
                    "key",
                    "state",
                    "source",
                    "checkpoint",
                    "generation",
                    "evidence",
                    "acceptanceEvidence",
                ]
                .contains(&k.as_str())
            })
        }) && receipt["key"] == digest(&json!([id, "execute"]))
            && receipt.get("source") == offer.get("source")
            && primitive(receipt.get("source"))
            && [
                "dispatching",
                "started",
                "completed",
                "not-started",
                "uncertain",
            ]
            .iter()
            .any(|s| receipt["state"] == *s)
            && (receipt.get("checkpoint") == Some(&Value::Null)
                || receipt.get("checkpoint").is_some_and(sha256))
            && safe_integer(&receipt["generation"]).is_some_and(|g| g >= 0)
            && nullable("evidence")
            && nullable("acceptanceEvidence");
        if !valid {
            return Err("invalid native action receipt".into());
        }
    }
    Ok(())
}

/// A session record as both runtimes read it; structured acceptance requires version 2.
pub fn validate(state: &Value) -> Result {
    let record = state.get("taskRecord").filter(|r| r.is_object());
    let field = |name: &str| record.and_then(|r| r.get(name));
    let version = state.get("formatVersion").and_then(safe_integer);
    if field("engineeringContract").is_some()
        || field("acceptanceEvidence").is_some()
        || version == Some(2)
    {
        let present = |v: Option<&Value>| v.is_some_and(truthy);
        if version != Some(2)
            || !present(field("engineeringContract"))
            || !present(field("acceptanceEvidence"))
        {
            return Err(
                "structured acceptance requires session state version 2; stop incompatible writers"
                    .into(),
            );
        }
        acceptance::validate_contract(&record.unwrap()["engineeringContract"])?;
        acceptance::validate_evidence(&record.unwrap()["acceptanceEvidence"])?;
    }
    validate_features(state.get("featureOutcomes"))?;
    if let Some(value) = state.get("recommendations") {
        recommendations(value)?;
    }
    let object = |name: &str| state.get(name).and_then(Value::as_object);
    let discovery = state.get("lastDiscovery").is_none_or(|d| {
        d.get("key").is_some_and(sha256) && safe_integer(&d["repetitions"]).is_some_and(|n| n >= 0)
    });
    let valid = matches!(version, Some(1 | 2))
        && object("activity")
            .is_some_and(|a| a.values().all(|v| safe_integer(v).is_some_and(|n| n >= 0)))
        && object("lookups").is_some_and(|l| l.len() <= 32)
        && state["attempts"]
            .as_array()
            .is_some_and(|a| a.len() <= ATTEMPT_CAPACITY)
        && discovery;
    if valid {
        Ok(())
    } else {
        Err("invalid session state".into())
    }
}

/// The saved active-task binding, or `None` when absent or bound to another checkout.
pub fn binding(value: &Value, checkout: Option<&Value>) -> Result<Option<Value>> {
    if value.is_null() {
        return Ok(None);
    }
    let map = value
        .as_object()
        .filter(|m| {
            m.keys().all(|k| {
                ["formatVersion", "task", "checkout", "repositories"].contains(&k.as_str())
            })
        })
        .ok_or("invalid task binding")?;
    if map.get("formatVersion").and_then(Value::as_f64) != Some(1.0) {
        return Err("invalid task binding version".into());
    }
    identity(&json!("binding"), map.get("task").unwrap_or(&Value::Null))?;
    if let Some(repositories) = map.get("repositories") {
        repository::scopes(repositories)?;
    }
    if let Some(saved) = map.get("checkout").filter(|c| !c.is_null())
        && !saved.as_str().is_some_and(|s| js_length(s) <= 1024)
    {
        return Err("invalid task checkout binding".into());
    }
    Ok(match checkout {
        Some(current) if map.get("checkout") != Some(current) => None,
        _ => Some(value.clone()),
    })
}

const START_FIELDS: &[&str] = &[
    "id",
    "goal",
    "stage",
    "files",
    "risks",
    "reviewAuthorization",
    "repositories",
    "engineeringContract",
];

/// `value ?? fallback`: JSON null is absent.
fn given(value: Option<&Value>) -> Option<&Value> {
    value.filter(|v| !v.is_null())
}

/// Serialized as `JSON.stringify` compares it; absent is distinct from null.
fn same(a: Option<&Value>, b: Option<&Value>) -> bool {
    a.map(Value::to_string) == b.map(Value::to_string)
}

fn optional_digest(contract: Option<&Value>) -> Result<Option<String>> {
    contract
        .filter(|c| truthy(*c))
        .map(acceptance::contract_digest)
        .transpose()
}

/// Create or re-scope the task record for `input`; returns the routing intent of its stage.
/// A changed stage, file set, risk set, repository selection, contract or checkout reopens
/// the task and clears its verification and acceptance evidence.
pub fn start(state: &mut Value, input: &Value, repository: &Value) -> Result<&'static str> {
    let input = input
        .as_object()
        .filter(|m| m.keys().all(|k| START_FIELDS.contains(&k.as_str())))
        .ok_or("invalid task start")?;
    let field = |name: &str| input.get(name);
    identity(&json!("task"), field("id").unwrap_or(&Value::Null))?;
    if !text(field("goal").unwrap_or(&Value::Null)) {
        return Err("task goal must be 1..512 characters without control characters".into());
    }
    let previous = state.get("taskRecord").filter(|r| truthy(*r));
    if previous.is_some_and(|r| r.get("id") != field("id") || r.get("goal") != field("goal")) {
        return Err("task identity already has a different goal; use a new task id".into());
    }
    let saved = |name: &str| previous.and_then(|r| given(r.get(name)));
    if let Some(contract) = field("engineeringContract") {
        acceptance::validate_contract(contract)?;
    }
    let contract = given(field("engineeringContract"))
        .or_else(|| saved("engineeringContract"))
        .cloned();
    let selected = stage(
        given(field("stage"))
            .or_else(|| saved("stage"))
            .unwrap_or(&json!("implement")),
    )?;
    let files = given(field("files"))
        .or_else(|| saved("files"))
        .cloned()
        .unwrap_or_else(|| json!([]));
    let files = files
        .as_array()
        .filter(|f| f.len() <= 32)
        .ok_or("task files must be a bounded array")?
        .iter()
        .map(relative_file)
        .collect::<Result<Vec<_>>>()?;
    if files.iter().any(|f| private_path(f)) {
        return Err("private workspace files are not task source".into());
    }
    let repositories = match field("repositories") {
        None => previous.and_then(|r| r.get("repositories")).cloned(),
        Some(value) => Some(json!(repository::scopes(value)?)),
    };
    if let Some(scopes) = repositories.as_ref().filter(|r| truthy(*r)) {
        let scopes: Vec<&str> = scopes
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .collect();
        if !files
            .iter()
            .all(|f| scopes.iter().any(|scope| member(f, scope)))
        {
            return Err("task files must belong to a selected repository".into());
        }
    }
    if let Some(risks) = field("risks")
        && !risks
            .as_array()
            .is_some_and(|r| r.len() <= 16 && r.iter().all(text))
    {
        return Err("invalid task risks".into());
    }
    if field("reviewAuthorization").is_some_and(|a| !text(a)) {
        return Err("invalid review authorization evidence".into());
    }
    let risks = given(field("risks"))
        .or_else(|| saved("risks"))
        .cloned()
        .unwrap_or_else(|| json!([]));
    let files = json!(files);
    let checkout = given(repository.get("checkout"))
        .cloned()
        .unwrap_or(Value::Null);
    if previous.is_none() {
        state["taskRecord"] = json!({
            "id": field("id"), "goal": field("goal"), "status": "working", "goalComplete": false,
        });
    }
    let record = state["taskRecord"]
        .as_object_mut()
        .ok_or("invalid session state")?;
    let changed = record.get("stage").is_some_and(truthy)
        && (record.get("stage") != Some(&json!(selected))
            || !same(
                given(record.get("files")).or(Some(&json!([]))),
                Some(&files),
            )
            || !same(
                given(record.get("risks")).or(Some(&json!([]))),
                Some(&risks),
            )
            || !same(record.get("repositories"), repositories.as_ref())
            || optional_digest(contract.as_ref())?
                != optional_digest(record.get("engineeringContract"))?
            || record.get("checkout") != Some(&checkout));
    if changed {
        record.insert("status".into(), json!("working"));
        record.insert("goalComplete".into(), json!(false));
        record.shift_remove("verification");
        record.shift_remove("verificationRevision");
        if contract.as_ref().is_some_and(truthy) {
            record.insert(
                "acceptanceEvidence".into(),
                json!({"formatVersion": 1, "records": []}),
            );
        }
        record.shift_remove("completed");
        record.insert("safeBoundary".into(), json!(false));
    }
    record.insert("stage".into(), json!(selected));
    record.insert("files".into(), files);
    record.insert("risks".into(), risks);
    record.insert("checkout".into(), checkout);
    if let Some(authorization) = field("reviewAuthorization") {
        record.insert("reviewAuthorization".into(), authorization.clone());
        record.insert("reviewRequested".into(), json!(true));
    }
    if let Some(repositories) = repositories {
        record.insert("repositories".into(), repositories);
    }
    if let Some(contract) = contract.filter(|c| truthy(c)) {
        record.insert("engineeringContract".into(), contract);
        if given(record.get("acceptanceEvidence")).is_none() {
            record.insert(
                "acceptanceEvidence".into(),
                json!({"formatVersion": 1, "records": []}),
            );
        }
        state["formatVersion"] = json!(2);
    }
    Ok(STAGES
        .iter()
        .find(|(name, _)| *name == selected)
        .map(|(_, intent)| *intent)
        .expect("validated stage"))
}

/// Identity of the task definition: goal, stage, files, risks, checkout, repositories, contract.
pub fn contract(record: Option<&Value>) -> Result<Value> {
    let Some(record) = record else {
        return Ok(Value::Null);
    };
    let or = |name: &str, default: Value| match record.get(name) {
        None | Some(Value::Null) => default,
        Some(value) => value.clone(),
    };
    let mut parts = vec![
        or("id", Value::Null),
        or("goal", Value::Null),
        or("stage", Value::Null),
        or("files", json!([])),
        or("risks", json!([])),
        or("checkout", Value::Null),
    ];
    if let Some(repositories) = record.get("repositories").filter(|r| truthy(*r)) {
        parts.push(repositories.clone());
    }
    if let Some(contract) = record.get("engineeringContract").filter(|c| truthy(*c)) {
        parts.push(json!(acceptance::contract_digest(contract)?));
    }
    Ok(json!(digest(&Value::Array(parts))))
}

/// A task's risks as offers record and match them: unique, in UTF-16 code unit order.
pub fn risks(value: &Value) -> Vec<&Value> {
    let mut risks: Vec<&Value> = Vec::new();
    for risk in value.as_array().into_iter().flatten() {
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
    risks
}

/// The comparable source identity of a complete repository snapshot.
pub fn source(repository: &Value) -> Value {
    if repository["gitAvailable"] == true && repository["fingerprints"]["complete"] == true {
        json!(digest(&json!([
            repository["head"],
            repository["fingerprints"]["index"],
            repository["fingerprints"]["workingTree"]
        ])))
    } else {
        Value::Null
    }
}

pub fn task_acceptance(state: &Value, repository: &Value) -> Result<Value> {
    let record = &state["taskRecord"];
    let attempts = state["attempts"].as_array().map_or(&[][..], Vec::as_slice);
    acceptance::coverage(
        record.get("engineeringContract"),
        record.get("acceptanceEvidence"),
        &json!({"kind": "repository", "identity": source(repository)}),
        !unresolved(attempts).is_empty(),
    )
}

const CONTEXT_FIELDS: &[&str] = &[
    "id",
    "stage",
    "status",
    "goalComplete",
    "files",
    "revision",
    "verification",
    "verificationRevision",
    "safeBoundary",
    "nextStep",
    "risks",
    "reviewRequested",
];

/// The saved task as current source sees it: stale or unaccepted work reads as working.
pub fn context(state: &Value, repository: &Value) -> Result<Value> {
    let record = &state["taskRecord"];
    if !truthy(record) {
        return Ok(json!({"task": {}, "stale": false}));
    }
    if !text(&record["id"]) || !text(&record["goal"]) {
        return Err("invalid saved task record".into());
    }
    stage(&record["stage"])?;
    let source = source(repository);
    let task_contract = contract(Some(record))?;
    let checkout = match &repository["checkout"] {
        Value::Null => Value::Null,
        other => other.clone(),
    };
    let stale = truthy(&record["summary"])
        && (source.is_null()
            || record.get("source") != Some(&source)
            || record.get("acceptedContract") != Some(&task_contract)
            || record.get("checkout") != Some(&checkout));
    let mut task: Map<String, Value> = CONTEXT_FIELDS
        .iter()
        .filter_map(|k| {
            record
                .get(*k)
                .filter(|v| !v.is_null())
                .map(|v| (k.to_string(), v.clone()))
        })
        .collect();
    let acceptance = task_acceptance(state, repository)?;
    if stale || (truthy(&record["engineeringContract"]) && acceptance["complete"] != true) {
        task.insert("status".into(), json!("working"));
        task.insert("goalComplete".into(), json!(false));
        task.insert("verification".into(), json!("unknown"));
        task.shift_remove("verificationRevision");
        task.insert("safeBoundary".into(), json!(false));
    }
    let nullable = |name: &str| record.get(name).cloned().unwrap_or(Value::Null);
    Ok(json!({
        "task": task,
        "stale": stale,
        "acceptance": acceptance,
        "evidenceDigest": digest(&nullable("acceptanceEvidence")),
        "taskContract": task_contract,
        "binding": {"kind": "repository", "identity": source},
        "summary": nullable("summary"),
        "nextStep": nullable("nextStep"),
    }))
}
