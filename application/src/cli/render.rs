//! Compact receipts and views: what a routine command writes unless `--json` or `--verbose`
//! asks for the full structured result.
use crate::render::shown;
use craft_engine::values::truthy;
use serde_json::{Value, json};

pub(super) fn render(value: &Value) -> String {
    format!(
        "{}\n",
        serde_json::to_string_pretty(value).expect("JSON values serialize")
    )
}

fn nonempty(value: &Value) -> Option<&str> {
    value.as_str().filter(|text| !text.is_empty())
}

/// A saved, unchanged, disabled or unavailable checkpoint in one line.
fn checkpoint_line(saved: &Value) -> String {
    if let Some(status) = nonempty(&saved["status"]) {
        return match nonempty(&saved["reason"]) {
            Some(reason) => format!("Checkpoint: {status} — {reason}"),
            None => format!("Checkpoint: {status}"),
        };
    }
    let incomplete = if saved["state"]["fingerprints"]["complete"] == false {
        " (repository inspection incomplete)"
    } else {
        ""
    };
    let status = if saved["unchanged"] == true {
        "unchanged"
    } else {
        "saved"
    };
    let mut text = format!("Checkpoint: {status}{incomplete}");
    if let Some(file) = nonempty(&saved["file"]) {
        text.push_str(&format!(" — {file}"));
    }
    text
}

fn joined(value: &Value) -> Option<String> {
    let items: Vec<&str> = value.as_array()?.iter().filter_map(Value::as_str).collect();
    (!items.is_empty()).then(|| items.join(", "))
}

/// The compact session view: context, guidance, activity, checkpoint and the next action.
pub(super) fn session_view(status: &Value) -> String {
    let usage = &status["usage"];
    let activity = &status["activity"];
    let pack = &status["packContext"];
    let suggestion = Some(&status["recommendation"])
        .filter(|s| !s.is_null())
        .unwrap_or(&status["suggestion"]);
    let observed = usage["status"] == "observed";
    let context = if observed {
        let percent = (usage["usedRatio"].as_f64().unwrap_or(0.0) * 100.0 + 0.5).floor();
        format!(
            "{}% ({}/{}); {}; {}",
            shown(&json!(percent)),
            shown(&usage["currentTokens"]),
            shown(&usage["capacityTokens"]),
            shown(&status["pressure"]),
            shown(&usage["source"])
        )
    } else {
        format!("unknown — {}", shown(&usage["reason"]))
    };
    let tokens = match &pack["estimatedTokens"] {
        Value::Null => "unknown".to_string(),
        tokens => format!("~{}", shown(tokens)),
    };
    let profile = nonempty(&status["modelAdvice"]["profile"])
        .map_or(String::new(), |p| format!("; configured profile {p}"));
    let reason = match suggestion["reason"].as_str() {
        Some(reason) => reason,
        None if usage["status"] == "unknown" => {
            "No reliable occupancy observation; continue useful work without inferring context pressure."
        }
        None => "Continue useful work; no context change is indicated.",
    };
    let or_unknown = |value: &Value| value.as_str().unwrap_or("unknown").to_string();
    let mut lines = vec![
        "Craft Capabilities — Session".to_string(),
        format!("Pack version: {}", shown(&status["packVersion"])),
        format!(
            "Host: {} | Model: {}",
            shown(&status["host"]["name"]),
            shown(&status["host"]["model"])
        ),
        format!("Context: {context}"),
        format!(
            "Pack: {tokens} estimated tokens selected; {} skills / {} references",
            shown(&pack["skills"]),
            shown(&pack["references"])
        ),
        format!(
            "Task: {} | Attempts: {}/{}",
            shown(&status["operation"]),
            shown(&activity["attempts"]),
            shown(&activity["maxAttempts"])
        ),
        format!(
            "Discovery: {} reused / {} fresh / {} batches (pack activity)",
            shown(&activity["reuseHits"]),
            shown(&activity["freshLookups"]),
            shown(&activity["batchedOperations"])
        ),
        format!(
            "Capability: {} (current: {}){profile}",
            or_unknown(&status["capability"]["class"]),
            or_unknown(&status["capability"]["currentClass"])
        ),
        format!(
            "Checkpoint: {}",
            or_unknown(&status["checkpoint"]["status"])
        ),
        String::new(),
        format!(
            "Advice: {}",
            suggestion["action"].as_str().unwrap_or("continue")
        ),
        format!("Reason: {reason}"),
    ];
    let consumed = &status["hostContext"]["consumption"];
    if consumed["kind"] == "cumulative-session" {
        lines.insert(
            4,
            format!(
                "Consumed: {} input + {} output = {} tokens (host session cumulative; observed {})",
                shown(&consumed["inputTokens"]),
                shown(&consumed["outputTokens"]),
                shown(&consumed["totalTokens"]),
                shown(&consumed["observedAt"])
            ),
        );
    }
    let compaction = &status["hostContext"]["compaction"];
    if let Some(phase) = compaction["status"].as_str() {
        lines.insert(
            4,
            format!(
                "Compaction: {phase} (observed {}; verify task continuation separately)",
                shown(&compaction["observedAt"])
            ),
        );
    }
    lines.extend(acceptance_lines(status, shown));
    if let Some(advice) = nonempty(&status["capability"]["recommendation"]) {
        lines.push(format!("Model advice: {advice}"));
    }
    if let Some(repositories) = status["repositories"].as_array() {
        let names: Vec<String> = repositories.iter().map(shown).collect();
        lines.push(format!("Repositories: {}", names.join(", ")));
    }
    let workspace = &status["workspace"];
    if workspace["status"] == "available" {
        lines.push(format!(
            "Workspace: scratch {}; {} evidence artifacts; {} bytes",
            shown(&workspace["scratch"]),
            shown(&workspace["evidence"]["artifacts"]),
            shown(&workspace["diskBytes"])
        ));
    } else if workspace["status"] == "retained" {
        lines.push(format!(
            "Workspace: retained — {}",
            shown(&workspace["reason"])
        ));
    }
    if let Some(after) = nonempty(&suggestion["afterCheckpoint"]) {
        lines.push(format!("After checkpoint: {after}"));
    }
    if let Some(boundary) = nonempty(&suggestion["atBoundary"]) {
        lines.push(format!("At boundary: {boundary}"));
    }
    format!("{}\n", lines.join("\n"))
}

/// Acceptance assurance and criteria, task contract, evidence digest and binding source;
/// `field` renders an assurance, criterion ID or status.
pub(super) fn acceptance_lines(result: &Value, field: impl Fn(&Value) -> String) -> Vec<String> {
    let mut lines = Vec::new();
    let acceptance = &result["acceptance"];
    if acceptance.is_object() {
        lines.push(format!("Acceptance: {}", field(&acceptance["assurance"])));
        for criterion in acceptance["criteria"].as_array().into_iter().flatten() {
            let optional = if criterion["required"] == true {
                ""
            } else {
                " (optional)"
            };
            lines.push(format!(
                "Criterion: {} — {}{optional}",
                field(&criterion["id"]),
                field(&criterion["status"])
            ));
        }
    }
    if let Some(contract) = nonempty(&result["taskContract"]) {
        lines.push(format!("Task contract: {contract}"));
    }
    if let Some(digest) = nonempty(&result["evidenceDigest"]) {
        lines.push(format!("Evidence digest: {digest}"));
    }
    if result["binding"].is_object() {
        lines.push(format!(
            "Source: {}",
            nonempty(&result["binding"]["identity"]).unwrap_or("unknown")
        ));
    }
    lines
}

fn offer_lines(result: &Value) -> Vec<String> {
    let offer = &result["offer"];
    if !offer.is_object() {
        let reason = result["applicability"]["reason"]
            .as_str()
            .unwrap_or("No action proposed.");
        return vec![format!("Offer: unavailable — {reason}")];
    }
    let suppressed = if result["suppressed"] == true {
        " (unchanged; suppressed)"
    } else {
        ""
    };
    vec![
        format!("Offer: {}", shown(&offer["id"])),
        format!(
            "Action: {} | Scope: {}",
            shown(&offer["action"]),
            shown(&offer["scope"])
        ),
        format!("Decision: {}{suppressed}", shown(&offer["decision"])),
        format!(
            "Execution: {}",
            offer["execution"].as_str().unwrap_or("not-observed")
        ),
    ]
}

/// Compact task-step and action receipts: the receipt, next action or offer, acceptance and
/// any warnings.
pub(super) fn action_receipt(action: &str, result: &Value) -> String {
    let mut lines = Vec::new();
    if action == "task-step" {
        let receipt = &result["recorded"];
        lines.push(format!(
            "Step recorded: {} / {} — {}",
            shown(&receipt["operation"]),
            shown(&receipt["target"]),
            shown(&receipt["outcome"])
        ));
        lines.push(checkpoint_line(&result["checkpoint"]));
        let plan = &result["plan"];
        if let Some(id) = nonempty(&plan["plan"]) {
            lines.push(match nonempty(&plan["reason"]) {
                Some(reason) => format!("Plan {id}: not updated — {reason}"),
                None => format!(
                    "Plan {id}: task {} {}",
                    shown(&plan["task"]),
                    shown(&plan["state"])
                ),
            });
        }
        if receipt["outcome"] != "passed" {
            lines.push(format!("Result: {}", shown(&result["result"])));
        }
        if result["complete"] == true {
            lines.push(if result["acceptance"]["assurance"] == "attributed" {
                "Task: complete (attributed criterion evidence)".into()
            } else {
                "Task: complete (agent-declared)".into()
            });
        }
        let next = &result["nextStep"];
        if next.is_object() {
            let reason = nonempty(&next["reason"]).map_or(String::new(), |r| format!(" — {r}"));
            lines.push(format!("Next: {}{reason}", shown(&next["action"])));
            for check in next["checks"].as_array().into_iter().flatten() {
                let mut argv = vec![check["command"].clone()];
                argv.extend(check["args"].as_array().cloned().unwrap_or_default());
                lines.push(format!(
                    "Check: {} {}",
                    shown(&check["cwd"]),
                    serde_json::to_string(&argv).expect("JSON values serialize")
                ));
            }
            if let Some(after) = nonempty(&next["afterCheckpoint"]) {
                lines.push(format!("After checkpoint: {after}"));
            }
        }
        let offer = &result["actionOffer"];
        if offer.is_object() && offer["suppressed"] != true {
            lines.extend(offer_lines(offer));
        }
    } else {
        lines.extend(offer_lines(result));
        if action == "task-action-check" {
            lines.push(format!("Ready: {}", result["ready"] == true));
            lines.push(result["reason"].as_str().unwrap_or_default().to_string());
            lines.push("Readiness is not execution or a permission grant.".into());
        } else {
            lines.push("No action dispatched.".into());
        }
    }
    lines.extend(acceptance_lines(result, shown));
    let status = &result["status"];
    if result["workspace"]["status"] == "retained"
        && let Some(reason) = nonempty(&result["workspace"]["reason"])
    {
        lines.push(format!("Workspace retained: {reason}"));
    }
    if truthy(&status["taskEvidenceStale"]) {
        lines.push("Warning: task evidence is stale.".into());
    }
    let mut warnings: Vec<&Value> = vec![
        &result["activityDiagnostic"],
        &status["diagnostic"],
        &status["actionDiagnostic"],
        &status["noticeDiagnostic"],
    ];
    warnings.extend(status["diagnostics"].as_array().into_iter().flatten());
    for warning in warnings.into_iter().filter(|w| truthy(*w)) {
        lines.push(format!("Warning: {}", shown(warning)));
    }
    lines.retain(|line| !line.is_empty());
    format!("{}\n", lines.join("\n"))
}

/// The compact task-start receipt: binding, route, checkpoint, advice and acceptance.
pub(super) fn adopt_receipt(result: &Value) -> String {
    let text = |value: &Value| value.as_str().unwrap_or_default().to_string();
    let context = &result["context"];
    let mut lines = vec![
        format!(
            "Task {}: {} (from {})",
            if result["adopted"] == true {
                "adopted"
            } else {
                "already bound"
            },
            text(&result["task"]),
            text(&result["from"])
        ),
        format!(
            "Stage: {} / {}{}",
            text(&context["task"]["stage"]),
            text(&context["task"]["status"]),
            if context["stale"] == true {
                " (stale: source changed)"
            } else {
                ""
            }
        ),
    ];
    if let Some(next) = nonempty(&context["nextStep"]) {
        lines.push(format!("Next: {next}"));
    }
    match &result["checkpoint"]["changed"] {
        Value::Null if result["checkpoint"].is_null() => {
            lines.push("Checkpoint: none carried".into())
        }
        Value::Bool(false) => lines.push("Checkpoint: carried; tree unchanged since".into()),
        Value::Bool(true) => {
            lines.push("Checkpoint: carried; tree changed since, recheck evidence".into())
        }
        _ => lines.push("Checkpoint: carried; change unknown".into()),
    }
    if let Some(families) = joined(&result["notCarried"]) {
        lines.push(format!(
            "Not carried (stay with the earlier session): {families}"
        ));
    }
    lines.join("\n")
}

/// A plan as its Markdown view, or one line per plan of a listing.
pub(super) fn plan_receipt(result: &Value) -> String {
    let Some(plans) = result["plans"].as_array() else {
        let mut text = craft_engine::plan::markdown(result);
        if let Some(view) = nonempty(&result["view"]) {
            text.push_str(&format!("\nView: {view}\n"));
        }
        return text;
    };
    if plans.is_empty() {
        return "No plans\n".into();
    }
    let lines: Vec<String> = plans
        .iter()
        .map(|plan| {
            let counts = &plan["counts"];
            let count = |state: &str| counts[state].as_i64().unwrap_or(0);
            format!(
                "{} [{}] {} — ready {}, running {}, completed {} (revision {})",
                plan["id"].as_str().unwrap_or_default(),
                plan["status"].as_str().unwrap_or_default(),
                plan["goal"].as_str().unwrap_or_default(),
                count("ready"),
                count("running") + count("verifying") + count("blocked"),
                count("completed"),
                plan["revision"]
            )
        })
        .collect();
    format!("{}\n", lines.join("\n"))
}

/// The queue as short lines: what to start next, then each non-empty group.
pub(super) fn queue_receipt(result: &Value) -> String {
    let plan = result["plan"].as_str().unwrap_or_default();
    let ids = |group: &str| -> Vec<String> {
        result[group]
            .as_array()
            .into_iter()
            .flatten()
            .map(|item| {
                let id = item
                    .as_str()
                    .or_else(|| item["task"].as_str())
                    .unwrap_or_default();
                match group {
                    "elsewhere" => {
                        let mut text = format!(
                            "{id} ({}, idle {} min",
                            item["state"].as_str().unwrap_or_default(),
                            item["idleMinutes"]
                        );
                        if item["orphaned"] == true {
                            text.push_str(", orphaned");
                        } else if item["stale"] == true {
                            text.push_str(", stale");
                        }
                        text + ")"
                    }
                    "waiting" => {
                        let on: Vec<String> = item["on"]
                            .as_array()
                            .into_iter()
                            .flatten()
                            .map(|d| {
                                format!(
                                    "{} {}",
                                    d["id"].as_str().unwrap_or_default(),
                                    d["state"].as_str().unwrap_or_default()
                                )
                            })
                            .collect();
                        format!(
                            "{id} (on {}{})",
                            on.join(", "),
                            if item["stuck"] == true { ", stuck" } else { "" }
                        )
                    }
                    "retry" | "exhausted" => {
                        format!("{id} ({} of {})", item["attempts"], item["maxAttempts"])
                    }
                    _ => id.to_string(),
                }
            })
            .collect()
    };
    let mut lines = vec![match result["next"].as_str() {
        Some(task) => {
            format!("Next: {task} — craft task-start --plan {plan} --task {task} (or --ready)")
        }
        None => format!("Next: none ready in plan {plan}"),
    }];
    for (group, label) in [
        ("ready", "Ready"),
        ("held", "Held here"),
        ("elsewhere", "Held elsewhere"),
        ("waiting", "Waiting"),
        ("retry", "Failed, may retry"),
        ("exhausted", "Out of attempts"),
        ("parents", "Children done, complete the parent"),
        (
            "integrate",
            "Completed in worktrees, integrate with plan-integrate",
        ),
        ("needsUser", "Integration needs the user"),
    ] {
        let listed = ids(group);
        if !listed.is_empty() {
            lines.push(format!("{label}: {}", listed.join("; ")));
        }
    }
    let stale = result["elsewhere"]
        .as_array()
        .into_iter()
        .flatten()
        .any(|i| i["stale"] == true || i["orphaned"] == true);
    if stale {
        lines.push(format!("Stale or orphaned claims continue only with: craft task-start --plan {plan} --task <task> --take-over"));
    }
    format!("{}\n", lines.join("\n"))
}

pub(super) fn assign_receipt(result: &Value) -> String {
    let text = |value: &Value| value.as_str().unwrap_or_default().to_string();
    let mut lines = vec![format!(
        "Assigned: {} of plan {} to worker session {} (revision {})",
        text(&result["task"]),
        text(&result["plan"]),
        text(&result["worker"]["session"]),
        result["revision"]
    )];
    for repository in result["work"]["repositories"]
        .as_array()
        .into_iter()
        .flatten()
    {
        lines.push(format!(
            "Worktree: {} on {}",
            text(&repository["worktree"]),
            text(&repository["branch"])
        ));
    }
    if result["isolated"] != true {
        lines.push("Shared checkout: the worker changes only its owned paths here".into());
    }
    lines.push(format!(
        "Worker starts with: {}",
        text(&result["worker"]["start"])
    ));
    format!("{}\n", lines.join("\n"))
}

pub(super) fn integrate_receipt(result: &Value) -> String {
    let text = |value: &Value| value.as_str().unwrap_or_default().to_string();
    let task = text(&result["task"]);
    let mut lines = Vec::new();
    if result["integrated"] == true {
        lines.push(format!(
            "Integrated: {task} merged into the coordinator checkout"
        ));
        let kept = result["worktrees"]["kept"]
            .as_array()
            .cloned()
            .unwrap_or_default();
        lines.extend(kept.iter().map(|k| format!("Kept: {}", text(k))));
    } else if result["conflicts"].is_array() {
        let paths: Vec<String> = result["conflicts"]
            .as_array()
            .into_iter()
            .flatten()
            .map(text)
            .collect();
        lines.push(format!(
            "{}: {task} conflicts on {}",
            if result["integration"]["state"] == "escalated" {
                "Needs the user"
            } else {
                "Conflict"
            },
            paths.join(", ")
        ));
        lines.push(format!("Next: {}", text(&result["next"])));
    } else if result["state"] == "pending" {
        lines.push(format!("Discarded: {task} is pending again"));
        let kept = result["worktrees"]["kept"]
            .as_array()
            .cloned()
            .unwrap_or_default();
        lines.extend(kept.iter().map(|k| format!("Kept: {}", text(k))));
    } else {
        lines.push(format!(
            "Integration of {task}: {}",
            text(&result["integration"]["state"])
        ));
    }
    format!("{}\n", lines.join("\n"))
}

pub(super) fn release_receipt(result: &Value) -> String {
    format!(
        "Released: {} of plan {} is pending (revision {}){}\n",
        result["task"].as_str().unwrap_or_default(),
        result["plan"].as_str().unwrap_or_default(),
        result["revision"],
        if result["unbound"] == true {
            "; this session's binding removed"
        } else {
            ""
        }
    )
}

pub(super) fn task_receipt(result: &Value) -> String {
    let routing = &result["routing"];
    let text = |value: &Value| value.as_str().unwrap_or_default().to_string();
    let mut lines = vec![
        format!("Task bound: {}", text(&result["task"])),
        nonempty(&result["plan"]["id"]).map_or(String::new(), |plan| {
            let taken = nonempty(&result["plan"]["takenOver"]["from"])
                .map_or(String::new(), |from| format!(" (taken over from {from})"));
            format!("Plan: {plan}{taken}")
        }),
        format!(
            "Execution: {} / {}",
            text(&routing["execution"]["mode"]),
            text(&routing["execution"]["role"])
        ),
    ];
    for (label, value) in [
        ("Repositories", &result["repositories"]),
        ("Skills", &routing["skills"]),
        ("References", &routing["references"]),
    ] {
        if let Some(items) = joined(value) {
            lines.push(format!("{label}: {items}"));
        }
    }
    lines.extend(envelope_lines(&result["plan"]["envelope"]));
    lines.push(checkpoint_line(&result["checkpoint"]));
    let model = &result["modelAdvice"];
    if model.is_object() {
        let profile = nonempty(&model["profile"]).map_or(String::new(), |p| format!(" / {p}"));
        let limitation = nonempty(&model["mainModel"]["limitation"])
            .map_or(" (recommendation only)".to_string(), |l| format!(" — {l}"));
        lines.push(format!(
            "Model advice: {}{profile}{limitation}",
            text(&model["decision"])
        ));
    }
    for missing in routing["missing"].as_array().into_iter().flatten() {
        lines.push(format!("Missing: {}", text(missing)));
    }
    lines.extend(acceptance_lines(result, text));
    if result["workspace"]["status"] == "retained"
        && let Some(reason) = nonempty(&result["workspace"]["reason"])
    {
        lines.push(format!("Workspace retained: {reason}"));
    }
    lines.retain(|line| !line.is_empty());
    format!("{}\n", lines.join("\n"))
}

/// What a plan task's envelope adds to the receipt; `plan-envelope` shows all of it.
fn envelope_lines(envelope: &Value) -> Vec<String> {
    let text = |value: &Value| value.as_str().unwrap_or_default().to_string();
    let mut lines = Vec::new();
    let task = &envelope["task"];
    if let Some(paths) = joined(&task["scope"]["paths"]) {
        lines.push(format!("Owns: {paths}"));
    }
    if let Some(returns) = nonempty(&task["returns"]) {
        lines.push(format!("Returns: {returns}"));
    }
    if let Some(parent) = nonempty(&envelope["parent"]["id"]) {
        let next = nonempty(&envelope["parent"]["nextStep"])
            .map_or(String::new(), |n| format!(" — next: {n}"));
        lines.push(format!("Part of: {parent}{next}"));
    }
    for dependency in envelope["dependencies"].as_array().into_iter().flatten() {
        let result = nonempty(&dependency["result"]).map_or(String::new(), |r| format!(": {r}"));
        lines.push(format!(
            "After {} ({}){result}",
            text(&dependency["id"]),
            text(&dependency["state"])
        ));
    }
    lines
}

/// The compact checkpoint receipt; `--json` or `--verbose` returns the full result.
pub(super) fn attempt_receipt(result: &Value) -> String {
    let recorded = &result["recorded"];
    let field = |name: &str| recorded[name].as_str().unwrap_or_default().to_string();
    format!(
        "Attempt recorded: {} / {} — {}\n",
        field("operation"),
        field("target"),
        field("outcome")
    )
}

pub(super) fn receipt(saved: &Value) -> String {
    let mut text = checkpoint_line(saved);
    if let Some(warning) = saved["activityDiagnostic"]
        .as_str()
        .filter(|w| !w.is_empty())
    {
        text.push_str(&format!("\nWarning: {warning}"));
    }
    text.push('\n');
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compact_receipts_keep_failed_outcomes_storage_diagnostics_and_action_boundaries() {
        for outcome in [
            "quality-failure",
            "permission-denied",
            "tool-unavailable",
            "blocked",
            "prerequisite-resolved",
        ] {
            let text = action_receipt(
                "task-step",
                &json!({"recorded": {"operation": "verify", "target": "acceptance:scope", "outcome": outcome},
                    "result": "Actual prerequisite detail",
                    "checkpoint": {"status": "unavailable", "reason": "EPERM: state write denied"},
                    "complete": false, "nextStep": null,
                    "status": {"diagnostics": ["Guidance unavailable"], "taskEvidenceStale": true}}),
            );
            assert!(text.contains(outcome));
            assert!(text.contains("Result: Actual prerequisite detail"));
            assert!(text.contains("Checkpoint: unavailable — EPERM"));
            assert!(text.contains("Warning: Guidance unavailable"));
            assert!(text.contains("evidence is stale"));
            assert!(!text.contains("Task: complete") && !text.contains("Next:"));
        }
        let offer = json!({"id": "offer-1", "action": "review", "scope": "review:current-source",
            "decision": "pending", "execution": "not-observed"});
        let blocked = action_receipt(
            "task-action-check",
            &json!({"offer": offer, "ready": false, "reason": "Approval is pending"}),
        );
        assert!(blocked.contains("Ready: false") && blocked.contains("Approval is pending"));
        assert!(blocked.contains("not execution or a permission grant"));
        let mut approved = offer.clone();
        approved["decision"] = json!("approved");
        let ready = action_receipt(
            "task-action-check",
            &json!({"offer": approved, "ready": true, "reason": "Current prerequisites present"}),
        );
        assert!(ready.contains("Ready: true") && ready.contains("Execution: not-observed"));
        let unavailable = action_receipt(
            "task-offer",
            &json!({"offer": null, "applicability": {"reason": "No observed capacity"}}),
        );
        assert!(unavailable.contains("No observed capacity"));
        let suppressed = action_receipt("task-offer", &json!({"offer": offer, "suppressed": true}));
        assert!(suppressed.contains("unchanged; suppressed"));
        assert!(suppressed.contains("No action dispatched"));
    }

    #[test]
    fn completion_and_status_views_show_attributed_criteria_and_suppressed_advice() {
        let acceptance = json!({"assurance": "attributed", "criteria": [
            {"id": "value", "status": "verified", "required": true},
            {"id": "error", "status": "gap", "required": false}]});
        let step = action_receipt(
            "task-step",
            &json!({"recorded": {"operation": "verify", "target": "acceptance:value", "outcome": "passed"},
                "complete": true, "acceptance": acceptance}),
        );
        assert!(step.contains("Task: complete (attributed criterion evidence)"));
        assert!(step.contains("Criterion: error — gap (optional)"));
        // A suppressed notice still shows the standing recommendation.
        let status = json!({"usage": {"status": "unknown", "reason": "no observation"}, "host": {"name": "codex", "model": "m"},
            "packContext": {"estimatedTokens": null}, "activity": {}, "operation": "fix-value",
            "suggestion": null, "recommendation": {"action": "compact"}, "acceptance": acceptance});
        let view = session_view(&status);
        assert!(view.contains("Advice: compact"));
        assert!(view.contains("Criterion: value — verified"));
    }

    #[test]
    fn acceptance_lines_render_missing_fields_by_view() {
        let result = json!({
            "acceptance": {"criteria": [{"id": "value", "status": null}]},
            "taskContract": "contract", "evidenceDigest": "", "binding": {}
        });
        let text = |value: &Value| value.as_str().unwrap_or_default().to_string();
        assert_eq!(
            acceptance_lines(&result, shown),
            [
                "Acceptance: unknown",
                "Criterion: value — unknown (optional)",
                "Task contract: contract",
                "Source: unknown"
            ]
        );
        assert_eq!(
            acceptance_lines(&result, text)[..2],
            ["Acceptance: ", "Criterion: value —  (optional)"]
        );
    }
}
