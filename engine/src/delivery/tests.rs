use super::*;

const CORPUS: &str = include_str!("../../../tests/fixtures/runtime/delivery-contracts.json");

fn corpus() -> Value {
    serde_json::from_str(CORPUS).unwrap()
}

fn cases<'a>(corpus: &'a Value, name: &str) -> &'a [Value] {
    corpus[name].as_array().unwrap()
}

fn flag(facts: &Value, name: &str) -> bool {
    facts[name] == true
}

#[test]
fn plans_replay_the_frozen_corpus() {
    let corpus = corpus();
    for case in cases(&corpus, "plans") {
        let facts = &case["facts"];
        let facts = Facts {
            session: flag(facts, "session"),
            context_supported: flag(facts, "contextSupported"),
            edit: flag(facts, "edit"),
            established: None,
            file_routed: false,
            prompt: facts["prompt"].as_str(),
            started: flag(facts, "started"),
            context_changing: flag(facts, "contextChanging"),
            context_restored: flag(facts, "contextRestored"),
            turn_finished: flag(facts, "turnFinished"),
            restore_pending: false,
        };
        let plan = plan(&facts, &case["settings"]);
        let expected = &case["result"];
        let observed = json!({
            "intent": plan.intent,
            "invalidateHint": plan.invalidate_hint,
            "services": plan.services,
            "saveCheckpoint": plan.save_checkpoint,
            "recoverCheckpoint": plan.recover_checkpoint,
            "readCheckpoint": plan.read_checkpoint,
        });
        assert_eq!(&observed, expected, "{}", case["id"]);
    }
}

#[test]
fn decisions_replay_the_frozen_corpus() {
    let corpus = corpus();
    for case in cases(&corpus, "decisions") {
        let present = |name: &str| Some(&case[name]).filter(|v| !v.is_null());
        let observed = decision(
            present("routing"),
            present("checkpoint"),
            present("services"),
            case["preserve"] == true,
        );
        assert_eq!(observed, case["result"], "{}", case["id"]);
    }
    for case in cases(&corpus, "failures") {
        let observed = failure(
            case["enforcement"] == true,
            case["message"].as_str().unwrap(),
        );
        assert_eq!(observed, case["result"], "{case}");
    }
}

#[test]
fn hint_records_replay_the_frozen_corpus() {
    let corpus = corpus();
    for case in cases(&corpus, "validations") {
        let observed = validate_hint(&case["state"]).err();
        assert_eq!(observed.as_deref(), case["error"].as_str(), "{case}");
    }
    for case in cases(&corpus, "reuse") {
        let key = case["key"].as_str().unwrap();
        let observed = hint_valid(&case["previous"], key, case["now"].as_f64().unwrap());
        assert_eq!(json!(observed), case["result"], "{case}");
    }
}

#[test]
fn an_edit_is_a_side_effect_of_the_intent_a_request_established() {
    let settings = json!({"routing": {"editHints": true}});
    let edit = |established, file_routed| {
        let facts = Facts {
            session: true,
            context_supported: true,
            edit: true,
            established,
            file_routed,
            ..Facts::default()
        };
        let plan = plan(&facts, &settings);
        (plan.intent, plan.establish)
    };
    assert_eq!(edit(None, false), (Some("implement"), Some("implement")));
    assert_eq!(
        edit(Some("implement"), false),
        (Some("implement"), Some("implement"))
    );
    assert_eq!(edit(Some("implement"), true), (None, None));
    for intent in ["plan", "locate", "review", "verify"] {
        assert_eq!(edit(Some(intent), false), (None, None), "{intent}");
    }
}

#[test]
fn a_request_establishes_its_intent_even_where_its_hint_cannot_be_shown() {
    let settings = json!({"routing": {"editHints": true}});
    let prompt = |text, context_supported, session| {
        let facts = Facts {
            session,
            context_supported,
            prompt: Some(text),
            established: Some("implement"),
            ..Facts::default()
        };
        let plan = plan(&facts, &settings);
        (plan.intent, plan.establish)
    };
    let question = "Which methods call applyDiscount?";
    assert_eq!(
        prompt(question, true, true),
        (Some("locate"), Some("locate"))
    );
    assert_eq!(prompt(question, false, true), (None, Some("locate")));
    assert_eq!(prompt(question, true, false), (Some("locate"), None));
    assert_eq!(prompt("thanks", true, true), (None, None));
}

#[test]
fn intent_records_keep_the_files_routed_under_one_intent() {
    let request = established(&Value::Null, "plan", None, 1000).unwrap();
    assert_eq!(
        request,
        json!({"formatVersion": 1, "intent": "plan", "files": [], "since": 1000})
    );
    assert_eq!(validate_intent(&request), Ok(()));
    // An edit under the same intent keeps when it started; a new request starts again.
    let planned = established(&request, "plan", Some("PLAN.md"), 2000).unwrap();
    assert_eq!(planned["since"], 1000);
    assert_eq!(
        established(&planned, "plan", None, 3000).unwrap()["since"],
        3000
    );
    assert_eq!(established(&request, "plan", None, 1000), None);
    let edited = established(&Value::Null, "implement", Some("src/a.rs"), 1000).unwrap();
    assert!(file_routed(&edited, "src/a.rs"));
    assert!(!file_routed(&edited, "src/b.rs"));
    assert_eq!(
        established(&edited, "implement", Some("src/a.rs"), 1000),
        None
    );
    // A new request starts afresh: its edits route again.
    let renewed = established(&edited, "implement", None, 1000).unwrap();
    assert!(!file_routed(&renewed, "src/a.rs"));
    let mut record = edited;
    for n in 0..=INTENT_FILES {
        record = established(&record, "implement", Some(&format!("f{n}")), 1000).unwrap();
    }
    assert_eq!(validate_intent(&record), Ok(()));
    assert!(!file_routed(&record, "src/a.rs"));
    for invalid in [
        json!({"formatVersion": 2, "intent": "plan", "files": []}),
        json!({"formatVersion": 1, "files": []}),
        json!({"formatVersion": 1, "intent": "plan", "files": [1]}),
        json!({"formatVersion": 1, "intent": "plan", "files": [], "since": "now"}),
    ] {
        assert!(validate_intent(&invalid).is_err(), "{invalid}");
    }
}

#[test]
fn a_planning_session_records_its_artifacts_as_evidence_without_reading_them() {
    let hash = |c: char| c.to_string().repeat(64);
    let plan = established(&Value::Null, "plan", None, 1000).unwrap();
    let first = planning_recorded(&plan, "PLAN.md", &hash('a')).unwrap();
    assert_eq!(
        first["planning"],
        json!([{"path": "PLAN.md", "contentHash": hash('a'), "revision": 1, "intent": "plan"}])
    );
    assert_eq!(validate_intent(&first), Ok(()));
    assert_eq!(planning_recorded(&first, "PLAN.md", &hash('a')), None);
    let second = planning_recorded(&first, "PLAN.md", &hash('b')).unwrap();
    assert_eq!(second["planning"][0]["revision"], 2);
    assert_eq!(second["planning"][0]["contentHash"], hash('b'));
    // Outside a planning session a write is no evidence; a later request keeps the evidence.
    let implement = established(&second, "implement", None, 1000).unwrap();
    assert_eq!(planning_recorded(&implement, "PLAN.md", &hash('c')), None);
    assert_eq!(implement["planning"], second["planning"]);
    let mut record = plan;
    for n in 0..=PLANNING_ARTIFACTS {
        record = planning_recorded(&record, &format!("plan-{n}.md"), &hash('a')).unwrap();
    }
    assert_eq!(validate_intent(&record), Ok(()));
    assert_eq!(record["planning"][0]["path"], "plan-1.md");
    let invalid = json!({"formatVersion": 1, "intent": "plan", "files": [],
        "planning": [{"path": "PLAN.md", "contentHash": "x", "revision": 1, "intent": "plan"}]});
    assert!(validate_intent(&invalid).is_err());
}

#[test]
fn a_status_names_the_files_a_turn_may_have_written() {
    let status = json!([
        "?? PLAN.md",
        " M src/cart.js",
        "A  notes/new.md",
        " D gone.md",
        "D  staged-gone.md",
        "R  docs/renamed.md",
        "docs/original.md",
        "MM both.md"
    ]);
    assert_eq!(
        written_paths(status.as_array().unwrap()),
        [
            "PLAN.md",
            "src/cart.js",
            "notes/new.md",
            "docs/renamed.md",
            "both.md"
        ]
    );
    let many: Vec<Value> = (0..40).map(|n| json!(format!("?? f{n}"))).collect();
    assert_eq!(written_paths(&many).len(), INTENT_FILES);
    assert!(written_paths(&[json!(""), json!("??"), json!(1)]).is_empty());
}

#[test]
fn a_locate_route_names_the_lookup_command_it_can_run() {
    let found = lookup("locate", Some("/plugins/craft/bin/craft"), "/app").unwrap();
    assert_eq!(
        found["run"],
        json!(["/plugins/craft/bin/craft", "locate", "--root", "/app"])
    );
    assert!(found["ask"].as_str().unwrap().contains("--callers <name>"));
    assert_eq!(lookup("implement", Some("/bin/craft"), "/app"), None);
    assert_eq!(lookup("locate", None, "/app"), None);
    assert_eq!(lookup("locate", Some(""), "/app"), None);
}

#[test]
fn a_lookup_route_speaks_only_with_a_verified_answer() {
    let answered = json!({"intent": "locate", "answer": {"rows": ["a.rs:1"]}});
    assert!(speaks(&answered, true));
    assert!(!speaks(
        &json!({"intent": "locate", "lookup": {"run": ["craft"]}}),
        true
    ));
    assert!(!speaks(&json!({"intent": "locate", "answer": null}), true));
    assert!(speaks(
        &json!({"intent": "implement", "skills": ["x"]}),
        false
    ));
}

#[test]
fn a_change_request_speaks_at_the_prompt_only_with_a_task_or_a_missing_input() {
    let unbound = json!({"intent": "implement", "skills": ["x"], "missing": [],
        "taskIdentity": {"session": "s", "task": null}});
    assert!(!speaks(&unbound, true));
    // The first edit's hint carries the same guidance where the change happens.
    assert!(speaks(&unbound, false));
    let bound = json!({"intent": "implement", "skills": ["x"],
        "taskIdentity": {"session": "s", "task": "t1"}});
    assert!(speaks(&bound, true));
    let missing = json!({"intent": "implement", "missing": ["services.codehost"]});
    assert!(speaks(&missing, true));
    let answered = json!({"intent": "implement", "answer": {"rows": ["a.rs:1"]}});
    assert!(speaks(&answered, true));
    // Other workflow requests keep their skills.
    assert!(speaks(&json!({"intent": "plan", "skills": ["x"]}), true));
}

#[test]
fn a_compacted_task_resumes_at_the_next_event_that_can_show_it() {
    let settings = json!({"checkpoint": {"enabled": true}});
    let changing = Facts {
        session: true,
        context_changing: true,
        ..Facts::default()
    };
    assert!(plan(&changing, &settings).mark_restore);
    let prompt = |restore_pending, context_supported| {
        let facts = Facts {
            session: true,
            context_supported,
            prompt: Some("continue"),
            restore_pending,
            ..Facts::default()
        };
        plan(&facts, &settings).recover_checkpoint
    };
    assert!(prompt(true, true));
    assert!(!prompt(false, true));
    // An event whose context the host cannot show keeps the restore pending.
    assert!(!prompt(true, false));
    let off = json!({"checkpoint": {"enabled": false}});
    assert!(!plan(&changing, &off).mark_restore);
}

#[test]
fn the_session_identity_follows_the_checkpoint_switch() {
    let on = json!({"checkpoint": {"enabled": true}});
    let off = json!({"checkpoint": {"enabled": false}});
    for event in [
        Facts {
            session: true,
            started: true,
            ..Facts::default()
        },
        Facts {
            session: true,
            context_restored: true,
            ..Facts::default()
        },
    ] {
        assert!(plan(&event, &on).identify_session);
        // A fully manual policy says nothing at session start or after compaction.
        assert!(!plan(&event, &off).identify_session);
    }
    let prompt = Facts {
        session: true,
        prompt: Some("fix it"),
        ..Facts::default()
    };
    assert!(!plan(&prompt, &on).identify_session);
}

#[test]
fn a_resume_comes_before_routing_and_raw_checkpoint_data() {
    let resume = json!({"task": "fix", "goal": "Fix it"});
    let checkpoint = json!({"saved": {"taskId": "fix"}, "resume": resume});
    let routing = json!({"intent": "implement"});
    let decided = decision(Some(&routing), Some(&checkpoint), None, false);
    assert_eq!(
        decided["context"],
        json!({"kind": "resume", "data": resume})
    );
    let raw = json!({"saved": {"taskId": "fix"}});
    assert_eq!(
        decision(None, Some(&raw), None, false)["context"]["kind"],
        "checkpoint"
    );
}
