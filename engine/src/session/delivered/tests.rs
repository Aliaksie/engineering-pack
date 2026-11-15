use super::*;

#[test]
fn a_refused_command_is_counted_and_summarized() {
    let once = refusal(&Value::Null);
    validate(&once).expect("valid record");
    let twice = refusal(&once);
    assert_eq!(counts(&twice)["commandsRefused"], 2);
    assert_eq!(
        summary(&twice).as_deref(),
        Some("Craft this session: 2 commands refused by the safety policy")
    );
}

#[test]
fn a_failed_hook_is_counted_and_summarized() {
    let once = hook_failure(&Value::Null);
    validate(&once).expect("valid record");
    let twice = hook_failure(&once);
    assert_eq!(counts(&twice)["hookFailures"], 2);
    assert_eq!(
        summary(&twice).as_deref(),
        Some("Craft this session: 2 hook failures")
    );
}

fn after(record: &Value, delivery: Delivery) -> Value {
    let next = recorded(record, &delivery).expect("record changes");
    validate(&next).expect("valid record");
    next
}

#[test]
fn an_edit_hears_only_findings_it_was_not_told_and_a_fixed_one_again() {
    let standard = json!("docs");
    let verdicts = |messages: &[&str]| {
        vec![json!({"standard": standard, "findings": messages
            .iter()
            .map(|m| json!({"rule": "r", "message": m}))
            .collect::<Vec<_>>()})]
    };
    let key = |m: &str| finding_key('a', &standard, &json!("r"), &json!(m));
    let checked = |keys: Vec<String>| Checked {
        scope: "a.md".into(),
        kinds: "a".into(),
        keys,
    };
    let told = after(
        &Value::Null,
        Delivery {
            context_bytes: 100,
            checked: &[checked(vec![key("one"), key("two")])],
            ..Delivery::default()
        },
    );
    // The same two again: nothing new to say.
    assert!(unheard(&told, "a.md", 'a', verdicts(&["one", "two"])).is_empty());
    // A third appears: only it is said.
    assert_eq!(
        unheard(&told, "a.md", 'a', verdicts(&["one", "two", "three"])),
        verdicts(&["three"])
    );
    // Another file, kind or no record: said in full.
    assert_eq!(
        unheard(&told, "b.md", 'a', verdicts(&["one"])),
        verdicts(&["one"])
    );
    assert_eq!(
        unheard(&told, "a.md", 'd', verdicts(&["one"])),
        verdicts(&["one"])
    );
    assert_eq!(
        unheard(&Value::Null, "a.md", 'a', verdicts(&["one"])),
        verdicts(&["one"])
    );
    // Fixed, then back: told again.
    let fixed = after(
        &told,
        Delivery {
            checked: &[checked(vec![key("two")])],
            ..Delivery::default()
        },
    );
    assert_eq!(
        unheard(&fixed, "a.md", 'a', verdicts(&["one", "two"])),
        verdicts(&["one"])
    );
}

#[test]
fn an_answer_is_delivered_once_per_window_and_within_its_budget() {
    let answered = |record: &Value, key: &str, bytes| {
        after(
            record,
            Delivery {
                context_bytes: bytes + 80,
                answer: Some((key, bytes)),
                ..Delivery::default()
            },
        )
    };
    let mut record = Value::Null;
    assert_eq!(answer(&record, "a", 1600), Answer::Deliver);
    record = answered(&record, "a", 1600);
    assert_eq!(answer(&record, "a", 1600), Answer::Repeated);
    // Ten answers of 1600 bytes fit 16 KiB; the eleventh does not.
    for index in 1..10 {
        let key = format!("k{index}");
        assert_eq!(answer(&record, &key, 1600), Answer::Deliver, "{index}");
        record = answered(&record, &key, 1600);
    }
    assert_eq!(answer(&record, "new", 1600), Answer::OverBudget);
    assert_eq!(answer(&record, "small", 300), Answer::Deliver);
    assert!(!window_spent(&record));
    record = answered(&record, "small", 384);
    assert!(window_spent(&record));
    // Held answers are counted, not delivered.
    let held = after(
        &record,
        Delivery {
            held: Some(Answer::OverBudget),
            ..Delivery::default()
        },
    );
    assert_eq!(held["answers"]["overBudget"], 1);
    assert_eq!(held["answers"]["delivered"], 11);
    // A compaction starts a new window; session totals stay.
    let compacted = new_window(&held).expect("window reset");
    validate(&compacted).unwrap();
    assert_eq!(answer(&compacted, "a", 1600), Answer::Deliver);
    assert!(!window_spent(&compacted));
    assert_eq!(compacted["answers"]["delivered"], 11);
    assert_eq!(compacted["answers"]["bytes"], 16_384);
    assert_eq!(new_window(&compacted), None);
}

#[test]
fn a_window_remembers_a_bounded_number_of_answers() {
    let mut record = Value::Null;
    for index in 0..ANSWER_KEYS + 3 {
        let key = format!("k{index}");
        let delivery = Delivery {
            answer: Some((&key, 1)),
            ..Delivery::default()
        };
        record = after(&record, delivery);
    }
    let kept = keys(&record["answers"]["window"]["keys"]);
    assert_eq!(kept.len(), ANSWER_KEYS);
    assert_eq!(kept[0], "k3");
}

fn checked(scope: &str, kinds: &str, keys: &[&str]) -> Checked {
    Checked {
        scope: scope.into(),
        kinds: kinds.into(),
        keys: keys.iter().map(|k| k.to_string()).collect(),
    }
}

#[test]
fn a_finding_is_raised_once_and_fixed_when_its_check_no_longer_finds_it() {
    let check = |record: &Value, scopes: &[Checked]| {
        recorded(
            record,
            &Delivery {
                checked: scopes,
                ..Delivery::default()
            },
        )
    };
    let first = [checked("src/a.rs", "ad", &["a:1", "a:2", "d:1"])];
    let record = check(&Value::Null, &first).unwrap();
    assert_eq!(record["findings"]["raised"], 3);
    // The same findings on the next edit are not raised again.
    assert_eq!(check(&record, &first), None);
    // The stop checks dependencies and markers only: anchor findings stay open.
    let stop = [checked("src/a.rs", "dm", &["m:1"])];
    let record = check(&record, &stop).unwrap();
    assert_eq!(
        (&record["findings"]["raised"], &record["findings"]["fixed"]),
        (&json!(4), &json!(1))
    );
    assert_eq!(
        record["findings"]["open"]["src/a.rs"],
        json!(["a:1", "a:2", "m:1"])
    );
    // A clean check of every kind closes the file.
    let clean = [checked("src/a.rs", "adm", &[])];
    let record = check(&record, &clean).unwrap();
    assert_eq!(record["findings"]["fixed"], 4);
    assert!(record["findings"].get("open").is_none());
    validate(&record).unwrap();
    // A clean check of a file with nothing open records nothing.
    assert_eq!(check(&Value::Null, &clean), None);
}

#[test]
fn the_summary_names_only_what_happened_and_is_due_once() {
    assert_eq!(summary(&Value::Null), None);
    assert_eq!(summary_due(&json!({"formatVersion": 1})), None);
    // Context bytes alone are nothing a user asked about.
    let quiet = after(
        &Value::Null,
        Delivery {
            context_bytes: 200,
            ..Delivery::default()
        },
    );
    assert_eq!(summary(&quiet), None);
    let scopes = [checked("a.rs", "a", &["a:1", "a:2"])];
    let record = after(
        &quiet,
        Delivery {
            checked: &scopes,
            empty_test: true,
            answer: Some(("k", 100)),
            resume: true,
            ..Delivery::default()
        },
    );
    let (line, shown) = summary_due(&record).unwrap();
    assert_eq!(
        line,
        "Craft this session: 2 standards findings raised, 0 fixed · 1 test run that ran no test caught · 1 lookup answered from the project index · 1 context restored after compaction"
    );
    validate(&shown).unwrap();
    assert_eq!(summary_due(&shown), None);
    // A change makes the line due again.
    let fixed = [checked("a.rs", "a", &["a:2"])];
    let record = after(
        &shown,
        Delivery {
            checked: &fixed,
            ..Delivery::default()
        },
    );
    assert!(summary_due(&record).unwrap().0.contains("raised, 1 fixed"));
}

#[test]
fn a_next_step_is_said_once_while_it_applies_and_again_when_it_returns() {
    let check = Some("craft check --stage commit");
    assert_eq!(notice_due(&Value::Null, true, None), None);
    let (line, record) = notice_due(&Value::Null, true, check).unwrap();
    assert_eq!(
        line.as_deref(),
        Some("Craft next: craft check --stage commit")
    );
    validate(&record).unwrap();
    assert_eq!(notice_due(&record, true, check), None);
    // A new summary is said alone while the same next step still applies.
    let refused = refusal(&record);
    let (line, record) = notice_due(&refused, true, check).unwrap();
    assert_eq!(
        line.as_deref(),
        Some("Craft this session: 1 command refused by the safety policy")
    );
    // Once it no longer applies it is forgotten quietly, and said when it returns.
    let (line, record) = notice_due(&record, true, None).unwrap();
    assert_eq!(line, None);
    assert!(record.get("next").is_none());
    assert_eq!(notice_due(&record, true, None), None);
    let (line, record) = notice_due(&refusal(&record), true, check).unwrap();
    assert_eq!(
        line.as_deref(),
        Some(
            "Craft this session: 2 commands refused by the safety policy · next: craft check --stage commit"
        )
    );
    validate(&record).unwrap();
    // Without the summary only the next step speaks.
    let other = Some("context 80 %: save a checkpoint before compacting");
    assert_eq!(
        notice_due(&refusal(&record), false, other)
            .unwrap()
            .0
            .as_deref(),
        Some("Craft next: context 80 %: save a checkpoint before compacting")
    );
}

#[test]
fn records_of_another_shape_are_refused() {
    validate(&Value::Null).unwrap();
    for record in [
        json!({}),
        json!({"formatVersion": 2}),
        json!({"formatVersion": 1, "resumes": -1}),
        json!({"formatVersion": 1, "answers": {"window": {"keys": [1]}}}),
        json!({"formatVersion": 1, "findings": {"open": []}}),
        json!({"formatVersion": 1, "context": {"text": "x"}}),
        json!({"formatVersion": 1, "shown": 1}),
        json!({"formatVersion": 1, "hooks": {"unread": 1}}),
        json!({"formatVersion": 1, "hooks": {"failed": "1"}}),
    ] {
        assert!(validate(&record).is_err(), "{record}");
    }
}
