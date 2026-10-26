//! Durable worker assignments and reconciliation of explicitly supplied host evidence.
use crate::{
    contracts::{js_length, safe_integer},
    digest, session, timestamp,
};
use serde_json::{Value, json};
use std::collections::HashSet;
type Result<T> = std::result::Result<T, String>;
const STATES: &[&str] = &[
    "reserved",
    "dispatching",
    "cancelling",
    "observed",
    "uncertain",
];
const STATUSES: &[&str] = &[
    "running",
    "idle",
    "completed",
    "failed",
    "cancelled",
    "released",
    "not-started",
    "unknown",
];
fn text(v: &Value) -> bool {
    v.as_str().is_some_and(|s| {
        (1..=512).contains(&js_length(s)) && !s.chars().any(|c| c <= '\u{1f}' || c == '\u{7f}')
    })
}
fn identifier(v: &Value) -> bool {
    v.as_str().is_some_and(|s| {
        !s.is_empty()
            && s.len() <= 96
            && s.bytes().next().is_some_and(|b| b.is_ascii_alphanumeric())
            && s.bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"._:-".contains(&b))
    })
}
fn hash(v: &Value) -> bool {
    v.as_str().is_some_and(|s| {
        s.len() == 64
            && s.bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    })
}
fn one_of(v: &Value, values: &[&str]) -> bool {
    v.as_str().is_some_and(|v| values.contains(&v))
}
fn fields(v: &Value, allowed: &[&str], label: &str) -> Result<()> {
    if v.as_object()
        .is_none_or(|o| o.keys().any(|k| !allowed.contains(&k.as_str())))
    {
        return Err(format!("invalid {label}"));
    }
    Ok(())
}
fn array<'a>(v: &'a Value, field: &str) -> &'a [Value] {
    v[field].as_array().map_or(&[], Vec::as_slice)
}
pub fn input(v: &Value) -> Result<Value> {
    fields(
        v,
        &["id", "offer", "scope", "parent", "reuseWorker", "brief"],
        "worker assignment",
    )?;
    if !identifier(&v["id"])
        || !hash(&v["offer"])
        || !text(&v["scope"])
        || v.get("parent").is_some_and(|v| !identifier(v))
        || v.get("reuseWorker").is_some_and(|v| !text(v))
    {
        return Err("invalid worker assignment identity/scope".into());
    }
    let brief = &v["brief"];
    fields(
        brief,
        &[
            "goal",
            "sources",
            "acceptance",
            "restrictions",
            "returnShape",
        ],
        "worker brief",
    )?;
    if !text(&brief["goal"]) || !text(&brief["returnShape"]) {
        return Err("worker goal and return shape are required".into());
    }
    for field in ["sources", "acceptance", "restrictions"] {
        if brief[field].as_array().is_none_or(|a| {
            a.len() > 16 || field == "acceptance" && a.is_empty() || a.iter().any(|v| !text(v))
        }) {
            return Err(format!("invalid worker brief {field}"));
        }
    }
    Ok(v.clone())
}
pub fn state(v: &Value, tree: &Value) -> Result<Value> {
    if !hash(tree) {
        return Err("invalid worker tree identity".into());
    }
    if v.is_null() {
        return Ok(json!({"formatVersion":1,"tree":tree,"requests":[]}));
    }
    fields(v, &["formatVersion", "tree", "requests"], "worker state")?;
    if ![json!(1), json!(2)].contains(&v["formatVersion"])
        || v["tree"] != *tree
        || v["requests"].as_array().is_none_or(|a| a.len() > 64)
    {
        return Err("invalid worker tree state".into());
    }
    let mut seen = HashSet::new();
    for r in array(v, "requests") {
        fields(
            r,
            &[
                "id",
                "key",
                "inputHash",
                "offer",
                "scope",
                "source",
                "policy",
                "host",
                "model",
                "role",
                "profile",
                "parent",
                "reuseWorker",
                "state",
                "status",
                "workerId",
                "occupied",
                "generation",
                "evidence",
                "modelMatches",
                "cancelKey",
                "revision",
                "at",
                "taskContract",
            ],
            "worker request state",
        )?;
        if r.get("taskContract")
            .is_some_and(|c| v["formatVersion"] != 2 || !hash(c))
        {
            return Err("invalid worker task contract or incompatible worker state version".into());
        }
        let nullable = |name: &str, check: fn(&Value) -> bool| {
            r.get(name).is_some_and(|v| v.is_null() || check(v))
        };
        let revision = |v: &Value| {
            v.as_str().is_some_and(|s| {
                (40..=64).contains(&s.len())
                    && s.bytes()
                        .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
            })
        };
        if !identifier(&r["id"])
            || !seen.insert(r["id"].clone())
            || !hash(&r["key"])
            || r["key"] != digest(&json!([tree, r["id"]]))
            || !["inputHash", "offer", "source", "policy"]
                .iter()
                .all(|f| hash(&r[f]))
            || !["scope", "host", "model", "role", "profile", "at"]
                .iter()
                .all(|f| text(&r[f]))
            || r["at"].as_str().and_then(timestamp::parse).is_none()
            || !one_of(&r["state"], STATES)
            || !one_of(&r["status"], STATUSES)
            || !r["occupied"].is_boolean()
            || !r["modelMatches"].is_boolean()
            || safe_integer(&r["generation"]).is_none_or(|n| n < 0)
            || !nullable("workerId", text)
            || !nullable("evidence", text)
            || !nullable("cancelKey", hash)
            || !nullable("revision", revision)
            || !nullable("parent", identifier)
            || !nullable("reuseWorker", text)
        {
            return Err("invalid saved worker request".into());
        }
    }
    Ok(v.clone())
}
fn observation(v: &Value) -> Result<()> {
    if !one_of(&v["status"], STATUSES)
        || !v["occupiesSlot"].is_boolean()
        || !text(&v["evidence"])
        || v.get("model").is_none_or(|v| !v.is_null() && !text(v))
        || (one_of(&v["status"], &["running", "idle", "unknown"]) && v["occupiesSlot"] != true)
        || (one_of(&v["status"], &["released", "not-started"]) && v["occupiesSlot"] == true)
    {
        return Err("invalid native worker observation".into());
    }
    Ok(())
}
pub fn snapshot(v: &Value, session: &str, now: i64) -> Result<Value> {
    fields(
        v,
        &[
            "host",
            "capacity",
            "occupiedSlots",
            "taskWorkers",
            "allowed",
            "sessionId",
            "observedAt",
            "source",
            "workers",
            "requests",
        ],
        "host worker snapshot",
    )?;
    if !text(&v["host"])
        || v["workers"].as_array().is_none_or(|a| a.len() > 256)
        || v["requests"].as_array().is_none_or(|a| a.len() > 128)
    {
        return Err("bounded worker inventory is required".into());
    }
    let count = |f: &str, min: i64| safe_integer(&v[f]).filter(|n| *n >= min);
    if v["sessionId"] != session
        || !text(&v["source"])
        || v["observedAt"]
            .as_str()
            .and_then(timestamp::parse)
            .is_none_or(|at| at > now || now - at > 30_000)
        || !v["allowed"].is_boolean()
        || count("capacity", 1).is_none()
        || count("occupiedSlots", 1).is_none()
        || count("taskWorkers", 0).is_none()
        || count("occupiedSlots", 1) > count("capacity", 1)
    {
        return Err("fresh matching host capacity is required".into());
    }
    let mut ids = HashSet::new();
    let mut operations = HashSet::new();
    for worker in array(v, "workers") {
        fields(
            worker,
            &["id", "status", "occupiesSlot", "model", "evidence"],
            "host worker",
        )?;
        observation(worker)?;
        if !text(&worker["id"]) || !ids.insert(worker["id"].clone()) {
            return Err("invalid or duplicate host worker id".into());
        }
    }
    for op in array(v, "requests") {
        fields(
            op,
            &["key", "status", "workerId", "evidence", "fenced", "settled"],
            "host dispatch lookup",
        )?;
        if !hash(&op["key"])
            || !operations.insert(op["key"].clone())
            || !one_of(&op["status"], &["not-started", "started"])
            || !text(&op["evidence"])
            || op["status"] == "started"
                && (!text(&op["workerId"])
                    || !ids.contains(&op["workerId"])
                    || op["settled"] != true)
            || op["status"] == "not-started" && (!op["workerId"].is_null() || op["fenced"] != true)
        {
            return Err("a negative dispatch lookup must fence late execution".into());
        }
    }
    let occupied = array(v, "workers")
        .iter()
        .filter(|w| w["occupiesSlot"] == true)
        .count() as i64;
    if count("taskWorkers", 0) != Some(occupied)
        || occupied > count("occupiedSlots", 1).unwrap() - 1
    {
        return Err("host task worker counts must match the complete tree inventory".into());
    }
    Ok(v.clone())
}
pub fn targets(state: &Value) -> Value {
    json!(array(state,"requests").iter().map(|r|json!({"key":r["key"],"cancelKey":r["cancelKey"],"generation":r["generation"],"workerId":r["workerId"]})).collect::<Vec<_>>())
}
pub fn reconcile(state: &mut Value, snapshot: &Value, targets: &Value) -> Result<Value> {
    let requests = state["requests"]
        .as_array_mut()
        .ok_or("invalid worker tree state")?;
    for r in requests {
        if r["host"] != snapshot["host"]
            || !targets.as_array().is_some_and(|a| {
                a.iter()
                    .any(|t| t["key"] == r["key"] && t["generation"] == r["generation"])
            })
        {
            continue;
        }
        let lookup = array(snapshot, "requests")
            .iter()
            .find(|o| o["key"] == r["key"]);
        let cancel = array(snapshot, "requests")
            .iter()
            .find(|o| !r["cancelKey"].is_null() && o["key"] == r["cancelKey"]);
        let pending = r["state"] != "observed";
        if r["workerId"].is_null() && lookup.is_some_and(|o| o["status"] == "not-started") {
            r["state"] = json!("observed");
            r["status"] = json!("not-started");
            r["occupied"] = json!(false);
            r["evidence"] = lookup.unwrap()["evidence"].clone();
            bump(r)?;
            continue;
        }
        let worker_id = if r["workerId"].is_null() {
            lookup
                .filter(|o| o["status"] == "started")
                .map(|o| &o["workerId"])
        } else {
            Some(&r["workerId"])
        };
        if let Some(found) = array(snapshot, "workers")
            .iter()
            .find(|w| Some(&w["id"]) == worker_id)
        {
            if pending
                && found["occupiesSlot"] == true
                && (if r["cancelKey"].is_null() {
                    lookup
                } else {
                    cancel
                })
                .is_none()
            {
                continue;
            }
            r["workerId"] = found["id"].clone();
            r["state"] = json!("observed");
            r["status"] = if pending
                && r["cancelKey"].is_null()
                && lookup.is_some_and(|o| o["status"] == "not-started")
            {
                json!("not-started")
            } else {
                found["status"].clone()
            };
            r["occupied"] = found["occupiesSlot"].clone();
            r["modelMatches"] = json!(found["model"] == r["model"]);
            r["evidence"] = found["evidence"].clone();
            bump(r)?;
        }
    }
    Ok(state.clone())
}
fn bump(r: &mut Value) -> Result<()> {
    let n = safe_integer(&r["generation"]).ok_or("invalid worker generation")?;
    r["generation"] = json!(n + 1);
    Ok(())
}
fn slot(r: &Value) -> String {
    if let Some(id) = r["workerId"].as_str() {
        format!("worker:{id}")
    } else {
        format!("request:{}", r["key"].as_str().unwrap_or_default())
    }
}
pub struct Admission<'a> {
    pub input: &'a Value,
    pub checked: &'a Value,
    pub snapshot: &'a Value,
    pub settings: &'a Value,
    pub session: &'a str,
    pub now: i64,
    pub at: &'a Value,
    pub revision: &'a Value,
}
pub fn admit(state: &mut Value, a: Admission) -> Result<Value> {
    let Admission {
        input: v,
        checked,
        snapshot,
        settings,
        session,
        now,
        at,
        revision,
    } = a;
    input(v)?;
    let input_hash = digest(v);
    if let Some(r) = array(state, "requests").iter().find(|r| r["id"] == v["id"]) {
        if r["inputHash"] != input_hash {
            return Err("worker assignment id already has different content".into());
        }
        return Ok(json!({"state":state,"request":r,"duplicate":true}));
    }
    let offer = &checked["offer"];
    if checked["ready"] != true
        || offer["action"] != "delegate"
        || offer["scope"] != v["scope"]
        || offer["id"] != v["offer"]
        || offer["host"] != snapshot["host"]
    {
        return Err("current scoped worker approval and host mapping are required".into());
    }
    let requests = array(state, "requests");
    if requests
        .iter()
        .any(|r| r["occupied"] == true && r["host"] != snapshot["host"])
    {
        return Err("occupied workers on another host require that host to reconcile before switching transport".into());
    }
    let mut held: HashSet<_> = requests
        .iter()
        .filter(|r| r["occupied"] == true)
        .map(slot)
        .collect();
    for w in array(snapshot, "workers")
        .iter()
        .filter(|w| w["occupiesSlot"] == true)
    {
        held.remove(&format!("worker:{}", w["id"].as_str().unwrap_or_default()));
    }
    let mut facts = json!({});
    for field in [
        "capacity",
        "occupiedSlots",
        "taskWorkers",
        "allowed",
        "sessionId",
        "observedAt",
        "source",
    ] {
        facts[field] = snapshot[field].clone();
    }
    for field in ["occupiedSlots", "taskWorkers"] {
        facts[field] = json!(
            safe_integer(&facts[field]).ok_or("invalid worker capacity facts")? + held.len() as i64
        );
    }
    let budget = super::budget(settings, &facts, Some(session), now)?;
    if let Some(parent) = v.get("parent")
        && !requests.iter().any(|r| {
            r["id"] == *parent
                && r["status"] == "running"
                && r["occupied"] == true
                && r["modelMatches"] == true
        })
    {
        return Err("nested dispatch requires a current observed parent in the same tree".into());
    }
    if budget["effectiveLimit"]
        .as_i64()
        .is_none_or(|n| n < 1 || facts["taskWorkers"].as_i64().unwrap_or(0) > n)
    {
        return Err("worker ceiling prohibits new assignments".into());
    }
    if let Some(reuse) = v.get("reuseWorker") {
        let known = requests.iter().rev().any(|r| r["workerId"] == *reuse);
        let worker = array(snapshot, "workers")
            .iter()
            .find(|w| w["id"] == *reuse);
        if !known
            || worker.is_none_or(|w| {
                w["occupiesSlot"] != true || w["status"] != "idle" || w["model"] != offer["model"]
            })
            || requests.iter().any(|r| {
                r["workerId"] == *reuse
                    && one_of(
                        &r["state"],
                        &["reserved", "dispatching", "cancelling", "uncertain"],
                    )
            })
        {
            return Err(
                "reuse requires an observed idle mapped worker without pending assignments".into(),
            );
        }
    } else if budget["available"].as_i64().unwrap_or(0) < 1 {
        return Err("worker capacity is reserved or occupied".into());
    }
    let busy: HashSet<_> = requests
        .iter()
        .filter(|r| {
            r["offer"] == offer["id"]
                && r["occupied"] == true
                && (r["status"] == "running" || r["state"] != "observed")
        })
        .map(slot)
        .collect();
    if busy.len() as i64 >= offer["proposedWorkers"].as_i64().unwrap_or(0) {
        return Err("approved concurrent assignment scope is full".into());
    }
    if requests.len() >= 64 {
        return Err("worker evidence capacity reached; no records were evicted".into());
    }
    let mut r = json!({"id":v["id"],"key":digest(&json!([state["tree"],v["id"]])),"inputHash":input_hash,"offer":offer["id"],"scope":v["scope"],"source":offer["source"],"policy":offer["policy"],"host":offer["host"],"model":offer["model"],"role":offer["role"],"profile":offer["profile"],"revision":revision,"parent":v["parent"],"reuseWorker":v["reuseWorker"],"state":"reserved","status":"unknown","workerId":v["reuseWorker"],"occupied":true,"generation":0,"evidence":null,"modelMatches":false,"cancelKey":null,"at":at});
    if let Some(contract) = offer.get("taskContract") {
        r["taskContract"] = contract.clone();
        state["formatVersion"] = json!(2);
    }
    state["requests"]
        .as_array_mut()
        .ok_or("invalid worker tree state")?
        .push(r.clone());
    Ok(json!({"state":state,"request":r,"duplicate":false,"budget":budget}))
}
pub fn transition(state: &mut Value, id: &Value, result: &Value) -> Result<Value> {
    let requests = state["requests"]
        .as_array_mut()
        .ok_or("invalid worker tree state")?;
    let index = requests
        .iter()
        .position(|r| r["id"] == *id)
        .ok_or("unknown worker assignment")?;
    let r = &requests[index];
    match result["kind"].as_str() {
        Some("cancelling") => {
            if r["workerId"].is_null()
                || r["occupied"] != true
                || r["state"] == "cancelling"
                || r["state"] == "uncertain" && !r["cancelKey"].is_null()
                || requests
                    .iter()
                    .rposition(|x| x["workerId"] == r["workerId"])
                    != Some(index)
            {
                return Err("cancellation requires the latest occupied worker assignment".into());
            }
            let key = digest(&json!([r["key"], "cancel", r["generation"]]));
            let r = &mut requests[index];
            r["cancelKey"] = json!(key);
            r["state"] = json!("cancelling");
            bump(r)?;
        }
        Some("dispatching") => {
            if r["state"] != "reserved" {
                return Err("worker dispatch was already attempted".into());
            }
            let r = &mut requests[index];
            r["state"] = json!("dispatching");
            bump(r)?;
        }
        Some("uncertain") => {
            let r = &mut requests[index];
            r["state"] = json!("uncertain");
            r["status"] = json!("unknown");
            r["occupied"] = json!(true);
            bump(r)?;
        }
        _ => {
            fields(
                result,
                &[
                    "kind",
                    "workerId",
                    "status",
                    "occupiesSlot",
                    "model",
                    "evidence",
                ],
                "native worker result",
            )?;
            if result["kind"] != "observed" || !text(&result["workerId"]) {
                return Err("a native worker result requires its actual id".into());
            }
            observation(result)?;
            if !r["workerId"].is_null() && result["workerId"] != r["workerId"] {
                return Err("native worker identity changed".into());
            }
            if r["reuseWorker"].is_null()
                && requests.iter().enumerate().any(|(i, r)| {
                    i != index && r["occupied"] == true && r["workerId"] == result["workerId"]
                })
            {
                return Err("native worker id is already assigned".into());
            }
            let r = &mut requests[index];
            r["workerId"] = result["workerId"].clone();
            r["state"] = json!("observed");
            r["status"] = result["status"].clone();
            r["occupied"] = result["occupiesSlot"].clone();
            r["modelMatches"] = json!(result["model"] == r["model"]);
            r["evidence"] = result["evidence"].clone();
            bump(r)?;
        }
    }
    let r = requests[index].clone();
    Ok(json!({"state":state,"request":r}))
}
pub fn evidence(state: &Value, task: &Value, repository: &Value) -> Result<Value> {
    if !task.is_object() {
        return Err("invalid worker task state".into());
    }
    let source = session::source(repository);
    let contract = session::contract(task.get("taskRecord").filter(|r| !r.is_null()))?;
    let requests = array(state, "requests").iter().map(|request| {
        let bound = request.get("taskContract").unwrap_or(&task["recommendations"][request["offer"].as_str().unwrap_or_default()]["taskContract"]);
        let stale = !source.is_null() && request["source"] != source
            || !bound.is_null() && !contract.is_null() && *bound != contract;
        let freshness = if stale { "stale" }
            else if !source.is_null() && !bound.is_null() && !contract.is_null() { "current" }
            else { "unknown" };
        let mut request = request.clone();
        request["taskContract"] = bound.clone();
        request["freshness"] = json!(freshness);
        request
    }).collect::<Vec<_>>();
    Ok(json!(requests))
}
pub fn execution_known(r: &Value) -> bool {
    r["state"] == "observed"
        && !r["workerId"].is_null()
        && !one_of(&r["status"], &["unknown", "not-started"])
}
