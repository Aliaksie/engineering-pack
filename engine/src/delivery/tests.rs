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
    let request = established(&Value::Null, "plan", None).unwrap();
    assert_eq!(
        request,
        json!({"formatVersion": 1, "intent": "plan", "files": []})
    );
    assert_eq!(validate_intent(&request), Ok(()));
    assert_eq!(established(&request, "plan", None), None);
    let edited = established(&Value::Null, "implement", Some("src/a.rs")).unwrap();
    assert!(file_routed(&edited, "src/a.rs"));
    assert!(!file_routed(&edited, "src/b.rs"));
    assert_eq!(established(&edited, "implement", Some("src/a.rs")), None);
    // A new request starts afresh: its edits route again.
    let renewed = established(&edited, "implement", None).unwrap();
    assert!(!file_routed(&renewed, "src/a.rs"));
    let mut record = edited;
    for n in 0..=INTENT_FILES {
        record = established(&record, "implement", Some(&format!("f{n}"))).unwrap();
    }
    assert_eq!(validate_intent(&record), Ok(()));
    assert!(!file_routed(&record, "src/a.rs"));
    for invalid in [
        json!({"formatVersion": 2, "intent": "plan", "files": []}),
        json!({"formatVersion": 1, "files": []}),
        json!({"formatVersion": 1, "intent": "plan", "files": [1]}),
    ] {
        assert!(validate_intent(&invalid).is_err(), "{invalid}");
    }
}
