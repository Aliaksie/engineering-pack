//! Native host observations translated into portable session facts. No transport, clock,
//! private transcript access or permission grant lives here.
use crate::{
    timestamp,
    values::{safe_integer, text},
};
use serde_json::{Value, json};

type Result<T> = std::result::Result<T, String>;

/// A mapped hook supplies lifecycle evidence even when it supplies no usage. A resume
/// hook does not prove a correlated compaction completion or a continuation check.
pub fn lifecycle(host: &str, session: &str, phase: &str, at: &str) -> Result<Value> {
    if !text(&json!(host)) || !text(&json!(session)) || timestamp::parse(at).is_none() {
        return Err("invalid host context lifecycle identity or timestamp".into());
    }
    let status = match phase {
        "before-compact" => "started",
        "after-compact" => "resume-observed",
        _ => return Err("unsupported context lifecycle phase".into()),
    };
    Ok(
        json!({"formatVersion":1,"host":host,"sessionId":session,"observedAt":at,
        "compaction":{"status":status,"source":format!("{host}.hook.{phase}"),"observedAt":at}}),
    )
}

fn count(value: &Value) -> Option<i64> {
    safe_integer(value).filter(|n| *n >= 0)
}

/// Claude's status-line formula, shared by live rendering and explicit event intake.
/// Missing counts remain unavailable; output is excluded from current input occupancy.
pub fn claude_usage(payload: &Value, at: &str) -> Option<Value> {
    let window = &payload["context_window"];
    let current = window.get("current_usage").filter(|c| c.is_object())?;
    let total = [
        "input_tokens",
        "cache_creation_input_tokens",
        "cache_read_input_tokens",
    ]
    .iter()
    .try_fold(0i64, |sum, name| {
        count(&current[name]).and_then(|n| sum.checked_add(n))
    })?;
    let mut usage = json!({"kind":"current-context","currentTokens":total,"source":"claude-statusline.current_usage (input and cache tokens; excludes output)","observedAt":at});
    if let Some(value) = window.get("context_window_size") {
        usage["capacityTokens"] = value.clone();
    }
    if let Some(session) = payload.get("session_id") {
        usage["sessionId"] = session.clone();
    }
    Some(usage)
}

fn identity(value: &Value, expected: &str) -> Result<()> {
    if !text(value) || value != expected {
        return Err("native observation does not identify this session".into());
    }
    Ok(())
}

/// A live callback with no valid occupancy must invalidate a previous usable snapshot.
pub fn unavailable(host: &str, session: &str, at: &str) -> Result<Value> {
    if !text(&json!(host)) || !text(&json!(session)) || timestamp::parse(at).is_none() {
        return Err("invalid unavailable context observation".into());
    }
    Ok(json!({"formatVersion":1,"host":host,"sessionId":session,"observedAt":at,"usage":null}))
}

/// `at` is the transport's receive time, supplied separately from a replayable native event.
/// Codex's latest request input includes cached input; adding cached tokens double-counts it.
pub fn decode(host: &str, payload: &Value, session: &str, at: &str) -> Result<Value> {
    if timestamp::parse(at).is_none() || !text(&json!(session)) {
        return Err("native observation requires a session and valid receive timestamp".into());
    }
    let mut event = json!({"formatVersion":1,"host":host,"sessionId":session,"observedAt":at});
    let (current, capacity, source) = match host {
        "claude" => {
            identity(&payload["session_id"], session)?;
            match payload["hook_event_name"].as_str() {
                Some("PreCompact") => {
                    event["compaction"] =
                        json!({"status":"started","source":"claude.PreCompact","observedAt":at});
                    return Ok(event);
                }
                Some("SessionStart") if payload["source"] == "compact" => {
                    event["compaction"] = json!({"status":"resume-observed","source":"claude.SessionStart.compact","observedAt":at});
                    return Ok(event);
                }
                Some(_) => return Err("unsupported Claude context event".into()),
                None => {}
            }
            let usage = claude_usage(payload, at)
                .ok_or("Claude current context counts are unavailable or invalid")?;
            let total = count(&usage["currentTokens"])
                .ok_or("Claude context count exceeds the numeric bound")?;
            if let Some(model) = payload["model"]["id"].as_str().filter(|m| text(&json!(m))) {
                event["model"] = json!(model);
            }
            (
                total,
                usage["capacityTokens"].clone(),
                "claude-statusline.current_usage (input and cache tokens; excludes output)",
            )
        }
        "codex" => {
            let params = &payload["params"];
            identity(&params["threadId"], session)?;
            if matches!(
                payload["method"].as_str(),
                Some("item/started" | "item/completed")
            ) && params["item"]["type"] == "contextCompaction"
            {
                if !text(&params["item"]["id"]) || !text(&params["turnId"]) {
                    return Err("compaction event requires item and turn identities".into());
                }
                event["compaction"] = json!({"status":if payload["method"] == "item/started" {"started"} else {"completion-observed"},
                    "id":params["item"]["id"],"turnId":params["turnId"],"source":"codex-app-server.contextCompaction","observedAt":at});
                return Ok(event);
            }
            if payload["method"] != "thread/tokenUsage/updated" {
                return Err("unsupported Codex context event".into());
            }
            let usage = &params["tokenUsage"];
            let total = &usage["total"];
            let cumulative: Option<Vec<i64>> = ["inputTokens", "outputTokens", "totalTokens"]
                .iter()
                .map(|name| count(&total[name]))
                .collect();
            if let Some(counts) = cumulative {
                event["consumption"] = json!({"kind":"cumulative-session","inputTokens":counts[0],"outputTokens":counts[1],"totalTokens":counts[2],"source":"codex-app-server.tokenUsage.total","observedAt":at});
            }
            (
                count(&usage["last"]["inputTokens"])
                    .ok_or("Codex last input token count is unavailable or invalid")?,
                usage["modelContextWindow"].clone(),
                "codex-app-server.tokenUsage.last.inputTokens (includes cached input; excludes output)",
            )
        }
        _ => return Err("native context observation is unsupported by this adapter".into()),
    };
    let mut usage = json!({"kind":"current-context","currentTokens":current,"source":source,"sessionId":session,"observedAt":at});
    if !capacity.is_null() {
        if count(&capacity).is_none_or(|n| n == 0) {
            return Err("native context capacity is invalid".into());
        }
        usage["capacityTokens"] = capacity;
    }
    event["usage"] = usage;
    Ok(event)
}

/// One bounded latest snapshot. A compaction transition invalidates pre-compaction usage;
/// a native completion proves the event, not checkpoint/continuation correctness.
pub fn merge(saved: &Value, event: &Value) -> Result<Value> {
    if saved.is_object()
        && (saved["sessionId"] != event["sessionId"] || saved["host"] != event["host"])
    {
        return Err("context observation belongs to another host or session".into());
    }
    if let (Some(old), Some(new)) = (
        saved["observedAt"].as_str().and_then(timestamp::parse),
        event["observedAt"].as_str().and_then(timestamp::parse),
    ) {
        if new < old {
            return Err("context observation is older than the saved snapshot".into());
        }
        if new == old {
            if saved["lastEvent"] == *event {
                return Ok(saved.clone());
            }
            // Never retain occupancy over a compaction/unavailable transition with the
            // same millisecond timestamp. Conflicting usage cannot revive old occupancy.
            if event.get("compaction").is_none() && !event.get("usage").is_some_and(Value::is_null)
            {
                return Err(
                    "context observation timestamp conflicts with the saved snapshot".into(),
                );
            }
        }
    }
    let mut next = if saved.is_object() {
        saved.clone()
    } else {
        json!({})
    };
    for name in [
        "formatVersion",
        "host",
        "sessionId",
        "observedAt",
        "model",
        "usage",
        "consumption",
    ] {
        if let Some(value) = event.get(name) {
            next[name] = value.clone();
        }
    }
    if let Some(compaction) = event.get("compaction") {
        next.as_object_mut().unwrap().remove("usage");
        let previous = saved
            .get("pendingCompaction")
            .unwrap_or(&saved["compaction"]);
        let mut compaction = compaction.clone();
        if compaction["status"] == "started" && compaction.get("id").is_some() {
            next["pendingCompaction"] = compaction.clone();
        }
        if compaction["status"] == "completion-observed"
            && previous["status"] == "started"
            && previous["id"] == compaction["id"]
            && previous["turnId"] == compaction["turnId"]
        {
            compaction["status"] = json!("completed");
            compaction["startedAt"] = previous["observedAt"].clone();
            next.as_object_mut().unwrap().remove("pendingCompaction");
        } else if compaction["status"] == "completion-observed"
            && saved["compaction"]["status"] == "completed"
            && saved["compaction"]["id"] == compaction["id"]
            && saved["compaction"]["turnId"] == compaction["turnId"]
        {
            compaction["status"] = json!("completed");
            compaction["startedAt"] = saved["compaction"]["startedAt"].clone();
        }
        if compaction.get("id").is_none() && saved["compaction"].get("id").is_some() {
            next["hookCompaction"] = compaction;
        } else {
            next["compaction"] = compaction;
        }
    } else if event.get("usage").is_some() && event.get("consumption").is_none() {
        next.as_object_mut().unwrap().remove("consumption");
    }
    next["lastEvent"] = event.clone();
    Ok(next)
}
