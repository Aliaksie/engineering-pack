//! Session advice over declared task state and host observations, at a fixed clock.
use craft_engine::contracts::effective_settings;
use craft_engine::session::status::advice as decide;
use serde_json::{Value, json};

/// 2026-10-18T12:00:00Z
const NOW: i64 = 1_792_324_800_000;

fn settings(project: &Value) -> Value {
    effective_settings(project, &json!({}), &json!({})).unwrap()["settings"].clone()
}

fn advice_with(input: Value, project: &Value) -> Value {
    decide(&input, &settings(project), NOW).unwrap()
}

fn advice(input: Value) -> Value {
    advice_with(input, &json!({"formatVersion": 1}))
}

fn refused(input: Value) -> String {
    decide(&input, &settings(&json!({"formatVersion": 1})), NOW).unwrap_err()
}

fn action(result: &Value) -> &str {
    result["suggestion"]["action"].as_str().unwrap_or("none")
}

fn project() -> Value {
    json!({"formatVersion": 1, "modules": [{"name": "app", "path": "app",
        "checks": [{"command": "consumer-check", "args": ["--scope", "literal; $(no-execution)"]}]}]})
}

fn observed(current: Value) -> Value {
    json!({"sessionId": "s1", "host": {"reliableCompaction": true},
        "task": {"safeBoundary": true, "handoffSaved": true, "checkpointFresh": true},
        "usage": {"kind": "current-context", "currentTokens": current, "capacityTokens": 100,
            "source": "fixture-host", "observedAt": "2026-10-18T12:00:00.000Z", "sessionId": "s1"}})
}

fn with(mut value: Value, patch: Value) -> Value {
    for (key, field) in patch.as_object().unwrap() {
        value[key] = field.clone();
    }
    value
}

#[test]
fn fresh_occupancy_drives_compact_save_and_new_session_advice() {
    assert_eq!(action(&advice(observed(json!(85)))), "compact");
    let phase = json!({"task": {"safeBoundary": true, "phaseChanged": true}});
    assert_eq!(
        action(&advice(with(observed(json!(96)), phase))),
        "save-handoff"
    );
    let saved = json!({"task": {"safeBoundary": true, "phaseChanged": true, "handoffSaved": true, "checkpointFresh": true}});
    assert_eq!(
        action(&advice(with(observed(json!(96)), saved))),
        "start-new-session"
    );
    assert_eq!(advice(observed(json!(20)))["suggestion"], Value::Null);
}

#[test]
fn invalid_cumulative_stale_or_foreign_observations_stay_unknown() {
    for change in [
        json!({"kind": "cumulative"}),
        json!({"observedAt": "2026-10-17T12:00:00Z"}),
        json!({"observedAt": "2026-10-19T12:00:00Z"}),
        json!({"source": ""}),
        json!({"sessionId": "other"}),
        json!({"currentTokens": -1}),
        json!({"currentTokens": 101}),
        json!({"currentTokens": null}),
        json!({"capacityTokens": 0}),
    ] {
        let mut input = observed(json!(85));
        input["usage"] = with(input["usage"].clone(), change.clone());
        let result = advice(input);
        assert_eq!(result["usage"]["status"], "unknown", "{change}");
        assert_eq!(result["suggestion"], Value::Null, "{change}");
    }
    assert_eq!(advice(json!({}))["usage"]["currentTokens"], Value::Null);
}

#[test]
fn next_commands_require_milestones_and_keep_declared_argv_unexecuted() {
    for task in [
        json!({"stage": "implement", "status": "working"}),
        json!({"stage": "implement", "status": "unknown"}),
        json!({"stage": "implement"}),
    ] {
        let result = advice_with(json!({"task": task}), &project());
        assert_eq!(result["suggestion"], Value::Null, "{task}");
    }
    let task = json!({"id": "T1", "stage": "implement", "status": "completed", "files": ["app/main.py"], "revision": "r2"});
    let next = advice_with(json!({"task": task}), &project());
    assert_eq!(action(&next), "run-check");
    assert_eq!(
        next["suggestion"]["checks"],
        json!([{"module": "app", "cwd": "app", "command": "consumer-check",
            "args": ["--scope", "literal; $(no-execution)"], "execution": "not-run"}])
    );
    assert_eq!(
        action(&advice(json!({"task": task}))),
        "design-verification"
    );
    let verified = |revision: &str, review: bool| {
        let mut task = with(
            task.clone(),
            json!({"verification": "passed", "verificationRevision": revision}),
        );
        if review {
            task["reviewRequested"] = json!(true);
        }
        action(&advice_with(json!({"task": task}), &project())).to_string()
    };
    assert_eq!(verified("r1", true), "run-check");
    assert_eq!(verified("r2", true), "review");
    assert_eq!(verified("r2", false), "summarize-evidence");
    let complete = with(task.clone(), json!({"goalComplete": true}));
    assert_eq!(
        action(&advice_with(json!({"task": complete}), &project())),
        "summarize-evidence"
    );
    assert_eq!(
        action(&advice(json!({"task": {"status": "failed"}}))),
        "inspect-failure"
    );
    assert_eq!(
        action(&advice(json!({"task": {"status": "blocked"}}))),
        "resolve-blocker"
    );
    for stage in ["explore", "story", "architecture"] {
        let input = json!({"task": {"stage": stage, "status": "completed"}});
        assert_eq!(action(&advice(input)), "summarize-evidence");
    }
    // A completed plan does not authorize implementation.
    let plan = json!({"task": {"stage": "plan", "status": "completed"}});
    assert_eq!(advice(plan)["suggestion"], Value::Null);
}

#[test]
fn notice_suppression_refreshes_after_cooldown_revision_or_action_change() {
    let first = advice(observed(json!(85)));
    let input = with(observed(json!(85)), json!({"previous": first["notice"]}));
    let suppressed = advice(input.clone());
    assert_eq!(suppressed["suppressed"], true);
    assert_eq!(suppressed["suggestion"], Value::Null);
    // Suppression does not extend the cooldown.
    assert_eq!(suppressed["notice"], first["notice"]);
    let mut later = input.clone();
    later["usage"]["observedAt"] = json!("2026-10-18T12:05:00.001Z");
    let later = decide(
        &later,
        &settings(&json!({"formatVersion": 1})),
        NOW + 300_001,
    )
    .unwrap();
    assert_eq!(later["suppressed"], false);
    let mut revised = input.clone();
    revised["task"]["revision"] = json!("new");
    assert_eq!(advice(revised)["suppressed"], false);
    let mut changed = with(
        input.clone(),
        json!({"task": {"safeBoundary": true, "phaseChanged": true}}),
    );
    changed["usage"]["currentTokens"] = json!(96);
    assert_eq!(action(&advice(changed)), "save-handoff");
    // Infinity reaches the engine as null through JSON.
    for policy in [
        json!({"elevatedAt": "0.6"}),
        json!({"compactAt": 1}),
        json!({"compactAt": "0.8"}),
        json!({"maxAgeMs": -1}),
        json!({"cooldownMs": null}),
    ] {
        assert!(
            refused(json!({"policy": policy})).contains("thresholds"),
            "{policy}"
        );
    }
    assert!(refused(json!({"task": {"files": ["../outside"]}})).contains("relative"));
    assert!(refused(json!({"command": "invented"})).contains("fields"));
}

#[test]
fn manual_verification_keeps_observation_scope_when_automation_exists() {
    let project = json!({"formatVersion": 1, "modules": [{"name": "app", "path": "src",
        "checks": [{"command": "node", "args": ["--test"]}]}]});
    let input = json!({"task": {"stage": "verify", "status": "completed", "verificationMode": "manual", "files": ["src/app.mjs"]}});
    let result = advice_with(input, &project);
    assert_eq!(action(&result), "design-verification");
    assert_eq!(result["suggestion"].get("checks"), None);
}

#[test]
fn pressure_respects_boundaries_controls_phase_and_checkpoint_freshness() {
    for (tokens, pressure) in [
        (59, "healthy"),
        (60, "elevated"),
        (75, "high"),
        (85, "saturated"),
    ] {
        assert_eq!(advice(observed(json!(tokens)))["pressure"], pressure);
    }
    let task = observed(json!(85))["task"].clone();
    let working = with(observed(json!(99)), json!({"task": {"status": "working"}}));
    assert_eq!(action(&advice(working)), "continue-bounded-step");
    let almost = with(
        observed(json!(99)),
        json!({"task": with(task.clone(), json!({"almostComplete": true}))}),
    );
    assert_eq!(action(&advice(almost)), "continue-bounded-step");
    let boundary = with(observed(json!(80)), json!({"task": {"safeBoundary": true}}));
    assert_eq!(advice(boundary)["suggestion"]["afterCheckpoint"], "compact");
    let unreliable = with(
        observed(json!(99)),
        json!({"host": {}, "task": {"safeBoundary": true, "handoffSaved": true}}),
    );
    assert_eq!(action(&advice(unreliable)), "save-handoff");
    let degraded = with(
        observed(json!(99)),
        json!({"task": with(task, json!({"compacted": true})), "activity": {"repeatedLookups": 3}}),
    );
    assert_eq!(action(&advice(degraded)), "start-new-session");
    assert_eq!(
        action(&advice(with(observed(json!(80)), json!({"host": {}})))),
        "start-new-session"
    );
    // Phase or session age alone does not justify a reset.
    let phase = json!({"task": {"safeBoundary": true, "phaseChanged": true}});
    assert_eq!(advice(phase)["suggestion"], Value::Null);
}

#[test]
fn execution_budget_stops_loops_and_keeps_unknown_fields_unknown() {
    assert_eq!(
        action(&advice(json!({"activity": {"noProgressAttempts": 1}}))),
        "retry-differently"
    );
    assert_eq!(
        action(&advice(json!({"activity": {"noProgressAttempts": 3}}))),
        "return-control"
    );
    let exhausted = with(
        observed(json!(99)),
        json!({"activity": {"attempts": 2, "maxAttempts": 2}}),
    );
    assert_eq!(action(&advice(exhausted)), "return-control");
    let empty = advice(json!({}));
    assert_eq!(empty["activity"]["attempts"], Value::Null);
    assert_eq!(empty["host"]["model"], "unknown");
    assert_eq!(empty["packContext"]["estimatedTokens"], Value::Null);
    for activity in [
        json!({"attempts": -1}),
        json!({"maxAttempts": 0}),
        json!({"retries": 1.5}),
        json!({"providerTokens": 100}),
    ] {
        let input = json!({"activity": activity});
        assert!(
            decide(&input, &settings(&json!({"formatVersion": 1})), NOW).is_err(),
            "{activity}"
        );
    }
}

#[test]
fn suppressed_notice_keeps_the_recommendation() {
    let first = advice(observed(json!(85)));
    let suppressed = advice(with(
        observed(json!(85)),
        json!({"previous": first["notice"]}),
    ));
    assert_eq!(suppressed["suggestion"], Value::Null);
    assert_eq!(suppressed["recommendation"]["action"], "compact");
    let failed = json!({"task": {"goalComplete": true, "verification": "failed"}});
    assert_eq!(action(&advice(failed)), "inspect-failure");
}

#[test]
fn healthy_reuse_alone_does_not_imply_post_compaction_degradation() {
    let input = with(
        observed(json!(91)),
        json!({"task": {"safeBoundary": true, "handoffSaved": true, "checkpointFresh": true, "compacted": true},
            "activity": {"repeatedLookups": 8, "reuseHits": 8}}),
    );
    assert_eq!(advice(input)["recommendation"]["action"], "compact");
}

fn budgeted(current: i64, task: Value) -> Value {
    let mut input = observed(json!(current));
    input["usage"]["capacityTokens"] = json!(1000);
    input["host"]["budgetTokens"] = json!(200);
    input["task"] = task;
    input
}

#[test]
fn a_declared_budget_advises_resuming_from_the_checkpoint() {
    let ready = json!({"safeBoundary": true, "handoffSaved": true, "checkpointFresh": true});
    let result = advice(budgeted(250, ready.clone()));
    assert_eq!(result["pressure"], "healthy");
    assert_eq!(action(&result), "start-new-session");
    assert!(
        result["suggestion"]["reason"]
            .as_str()
            .unwrap()
            .contains("working budget")
    );
    assert_eq!(
        result["budget"],
        json!({"budgetTokens": 200, "exceeded": true})
    );
    assert_eq!(result["host"]["budgetTokens"], 200);

    let unsaved = advice(budgeted(250, json!({"safeBoundary": true})));
    assert_eq!(action(&unsaved), "save-handoff");
    assert_eq!(
        unsaved["suggestion"]["afterCheckpoint"],
        "start-new-session"
    );
    let busy = advice(budgeted(250, json!({"safeBoundary": false})));
    assert_eq!(action(&busy), "continue-bounded-step");
    assert_eq!(busy["suggestion"]["atBoundary"], "resume-from-checkpoint");

    let under = advice(budgeted(150, ready.clone()));
    assert_eq!(under["suggestion"], Value::Null);
    assert_eq!(under["budget"]["exceeded"], false);
    // Window pressure keeps its own advice; the budget never hides it.
    assert_eq!(action(&advice(budgeted(800, ready))), "compact");
}

#[test]
fn no_budget_is_assumed_and_unknown_usage_cannot_pass_one() {
    let result = advice(observed(json!(50)));
    assert_eq!(result.get("budget"), None);
    let mut stale = budgeted(250, json!({"safeBoundary": true}));
    stale["usage"]["observedAt"] = json!("2026-10-17T12:00:00Z");
    let result = advice(stale);
    assert_eq!(result["budget"]["exceeded"], Value::Null);
    assert_eq!(result["suggestion"], Value::Null);
}

#[test]
fn a_model_window_fills_in_only_when_the_host_reports_none() {
    let mut input = observed(json!(400));
    input["usage"]
        .as_object_mut()
        .unwrap()
        .remove("capacityTokens");
    assert_eq!(advice(input.clone())["usage"]["status"], "unknown");
    input["host"]["windowTokens"] = json!(1000);
    let result = advice(input.clone());
    assert_eq!(result["usage"]["capacityTokens"], 1000);
    assert_eq!(result["usage"]["capacitySource"], "model-context-limit");
    // A host-reported window wins over the declared one.
    input["usage"]["capacityTokens"] = json!(500);
    let result = advice(input);
    assert_eq!(result["usage"]["capacityTokens"], 500);
    assert_eq!(result["usage"].get("capacitySource"), None);
}

#[test]
fn high_pressure_names_listed_models_with_a_larger_window() {
    let mut input = observed(json!(85));
    input["host"]["otherModels"] = json!([
        {"model": "small", "windowTokens": 50},
        {"model": "large", "windowTokens": 1000}
    ]);
    let result = advice(input.clone());
    assert_eq!(action(&result), "compact");
    assert_eq!(
        result["suggestion"]["largerWindowModels"],
        json!([{"model": "large", "windowTokens": 1000}])
    );
    input["usage"]["currentTokens"] = json!(20);
    assert_eq!(advice(input)["suggestion"], Value::Null);
}

#[test]
fn invalid_context_limit_facts_are_refused() {
    for host in [
        json!({"budgetTokens": 0}),
        json!({"windowTokens": -5}),
        json!({"windowTokens": "1m"}),
        json!({"otherModels": [{"model": "", "windowTokens": 10}]}),
        json!({"otherModels": [{"model": "m", "windowTokens": 10, "extra": 1}]}),
        json!({"otherModels": {}}),
    ] {
        let mut input = observed(json!(10));
        input["host"] = host.clone();
        assert!(!refused(input).is_empty(), "{host}");
    }
}

#[test]
fn project_limits_override_the_bundled_table_and_declared_facts_win() {
    use craft_engine::session::status::{Observations, Request, status};
    let project = json!({"formatVersion": 1, "execution": {"hosts": {"claude": {"contextLimits": {
        "m1": {"budgetTokens": 200}, "m3": {"windowTokens": 4000}}}}}});
    let observations = Observations {
        host_limits: Some(json!({"m1": {"windowTokens": 1000}, "m2": {"windowTokens": 2000}})),
        ..Observations::default()
    };
    let run = |host: Value| {
        let mut input = observed(json!(300));
        input["usage"]
            .as_object_mut()
            .unwrap()
            .remove("capacityTokens");
        input["host"] = host;
        status(Request {
            input: &input,
            settings: &settings(&project),
            session: None,
            task: None,
            now: NOW,
            observations: &observations,
        })
        .unwrap()
    };
    let result = run(json!({"name": "claude", "model": "m1"}));
    assert_eq!(result["usage"]["capacityTokens"], 1000);
    assert_eq!(result["budget"]["budgetTokens"], 200);
    assert_eq!(result["budget"]["exceeded"], true);
    let declared = run(json!({"name": "claude", "model": "m1", "budgetTokens": 500}));
    assert_eq!(declared["budget"]["exceeded"], false);
    let unlisted = run(json!({"name": "claude", "model": "other"}));
    assert_eq!(unlisted["usage"]["status"], "unknown");
    assert_eq!(unlisted.get("budget"), None);
    // High pressure on a listed model names the larger listed windows; an unlisted one, none.
    let high = |model: &str| {
        let mut input = observed(json!(900));
        // The listed model takes its window from the table; the unlisted one reports its own.
        input["usage"]["capacityTokens"] = if model == "m1" {
            Value::Null
        } else {
            json!(1000)
        };
        input["host"] = json!({"name": "claude", "model": model});
        status(Request {
            input: &input,
            settings: &settings(&project),
            session: None,
            task: None,
            now: NOW,
            observations: &observations,
        })
        .unwrap()["suggestion"]["largerWindowModels"]
            .clone()
    };
    assert_eq!(
        high("m1"),
        json!([{"model": "m2", "windowTokens": 2000}, {"model": "m3", "windowTokens": 4000}])
    );
    assert_eq!(high("other"), Value::Null);
}

#[test]
fn merged_limits_keep_the_largest_other_windows_within_the_advice_bound() {
    use craft_engine::session::status::{Observations, Request, status};
    // Sixteen project models plus two bundled ones exceed the sixteen others advice accepts.
    let limits: serde_json::Map<String, Value> = (1..=16)
        .map(|i| (format!("custom-{i:02}"), json!({"windowTokens": 1000 + i})))
        .collect();
    let project =
        json!({"formatVersion": 1, "execution": {"hosts": {"claude": {"contextLimits": limits}}}});
    let observations = Observations {
        host_limits: Some(json!({"m1": {"windowTokens": 1000}, "m2": {"windowTokens": 2000}})),
        ..Observations::default()
    };
    let mut input = observed(json!(900));
    input["usage"]["capacityTokens"] = Value::Null;
    input["host"] = json!({"name": "claude", "model": "m1"});
    let result = status(Request {
        input: &input,
        settings: &settings(&project),
        session: None,
        task: None,
        now: NOW,
        observations: &observations,
    })
    .unwrap();
    let larger = result["suggestion"]["largerWindowModels"]
        .as_array()
        .unwrap();
    assert_eq!(larger.len(), 16);
    assert!(larger.contains(&json!({"model": "m2", "windowTokens": 2000})));
    assert!(!larger.contains(&json!({"model": "custom-01", "windowTokens": 1001})));
}
