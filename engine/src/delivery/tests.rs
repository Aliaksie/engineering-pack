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
            edit: flag(facts, "edit"),
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
