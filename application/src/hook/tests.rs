use super::*;

/// Codex `apply_patch` passes the patch text (openai/codex
/// `core/src/tools/handlers/apply_patch.rs`); the files come from its headers.
#[test]
fn a_patch_edit_names_every_file_it_writes() {
    let patch = "*** Begin Patch\n*** Update File: src/a.rs\n@@\n-x\n+y\n*** Add File: docs/b.md\n+b\n*** Delete File: old.rs\n*** Update File: src/c.rs\n*** Move to: src/d.rs\n@@\n-x\n+y\n*** Update File: src/a.rs\n*** End Patch";
    let payload = json!({"session_id": "S", "cwd": "/r", "tool_name": "apply_patch",
        "tool_input": {"command": patch}});
    let event = normalize(&payload, "codex", "after-tool");
    assert_eq!(event["tool"], "edit");
    assert_eq!(event["file"], "src/a.rs");
    assert_eq!(
        event["files"],
        json!(["src/a.rs", "docs/b.md", "old.rs", "src/c.rs", "src/d.rs"])
    );
    // A path-carrying edit keeps its one file; text that is not a patch names none.
    let path = json!({"tool_name": "Edit", "tool_input": {"file_path": "x.rs", "command": patch}});
    let event = normalize(&path, "claude", "after-tool");
    assert_eq!(
        (event["file"].as_str(), event.get("files")),
        (Some("x.rs"), None)
    );
    let prose =
        json!({"tool_name": "apply_patch", "tool_input": {"command": "*** Update File: a.rs"}});
    assert!(
        normalize(&prose, "codex", "after-tool")
            .get("file")
            .is_none()
    );
    let many: String = (0..100)
        .map(|i| format!("*** Add File: f{i}.rs\n"))
        .collect();
    let many = json!({"tool_name": "apply_patch", "tool_input": {"command": format!("*** Begin Patch\n{many}")}});
    assert_eq!(
        normalize(&many, "codex", "after-tool")["files"]
            .as_array()
            .unwrap()
            .len(),
        PATCH_FILES
    );
}

/// Payloads a released Codex sent to Craft's hooks, recorded live: the tool calls a code
/// mode `exec` script makes reach hooks as `apply_patch` and `Bash`, with absolute paths.
#[test]
fn observed_codex_payloads_name_their_tool_and_files() {
    let observed: Value = serde_json::from_str(include_str!(
        "../../../tests/fixtures/hosts/codex-observed.json"
    ))
    .unwrap();
    for case in observed["cases"].as_array().unwrap() {
        let lifecycle = case["lifecycle"].as_str().unwrap();
        let event = normalize(&case["payload"], "codex", lifecycle);
        let expect = &case["expect"];
        assert_eq!(event["tool"], expect["tool"], "{}", case["id"]);
        for key in ["file", "files"] {
            if let Some(value) = expect.get(key) {
                assert_eq!(&event[key], value, "{} {key}", case["id"]);
            }
        }
        assert_eq!(
            event["session"], case["payload"]["session_id"],
            "{}",
            case["id"]
        );
    }
}

/// The frozen host corpus: payload normalization and wire serialization per host.
#[test]
fn frozen_host_contracts() {
    let corpus: Value = serde_json::from_str(include_str!(
        "../../../tests/fixtures/runtime/host-contracts.json"
    ))
    .unwrap();
    for case in corpus["cases"].as_array().unwrap() {
        let host = case["host"].as_str().unwrap();
        let lifecycle = case["definition"]["event"].as_str().unwrap();
        let actual = if case["operation"] == "normalize" {
            normalize(&case["input"], host, lifecycle)
        } else {
            let wire = serialize(&case["input"], host, lifecycle);
            json!({"output": wire.stdout, "error": wire.stderr, "exitCode": wire.exit_code})
        };
        assert_eq!(
            actual.to_string(),
            case["response"].to_string(),
            "{}",
            case["id"]
        );
    }
}

/// Structured contexts rendered as hosts receive them, incl. truncation.
#[test]
fn frozen_context_rendering() {
    let corpus: Value = serde_json::from_str(include_str!(
        "../../../tests/fixtures/runtime/render-contracts.json"
    ))
    .unwrap();
    let mut different = Vec::new();
    // JavaScript renders an absent `data` as "undefined"; JSON input cannot omit it
    // into anything but null, so those captured cases are unreachable.
    let reachable = |case: &&Value| {
        !case["id"]
            .as_str()
            .unwrap()
            .starts_with("services-missing:")
    };
    for case in corpus["render"]
        .as_array()
        .unwrap()
        .iter()
        .filter(reachable)
    {
        let actual = json!(render(&case["context"], case["host"].as_str().unwrap()));
        if actual != case["expected"] {
            different.push(format!("{} {actual} {}", case["id"], case["expected"]));
        }
    }
    for case in corpus["wire"].as_array().unwrap().iter().filter(reachable) {
        let wire = serialize(
            &case["result"],
            case["host"].as_str().unwrap(),
            case["event"].as_str().unwrap(),
        );
        let actual =
            json!({"output": wire.stdout, "error": wire.stderr, "exitCode": wire.exit_code});
        if actual != case["expected"] {
            different.push(format!("{} {actual} {}", case["id"], case["expected"]));
        }
    }
    assert!(different.is_empty(), "{}", different.join("\n"));
}

/// A session start that would pass the host's ceiling keeps who the session is and the
/// task it resumes, leaves out whole lists of what earlier sessions left and says which.
#[test]
fn a_context_over_the_ceiling_loses_whole_low_priority_pieces() {
    let list = |name: &str| json!([{"name": name, "notes": "n".repeat(3_500)}]);
    let result = json!({"decision": "allow",
        "session": {"id": "S1", "task": "T1", "handoff": list("handoff"),
            "planning": list("planning"), "plans": list("plans")},
        "context": {"kind": "resume", "data": {"task": "T1", "next": "r".repeat(2_000)}}});
    for host in ["claude", "codex", "copilot", "cursor", "gemini"] {
        let shaped = shaped(&result, host, "session-start");
        assert!(shaped.context.len() <= ceiling(host), "{host}");
        assert_eq!(
            shaped.dropped,
            [
                "unfinished tasks of earlier sessions",
                "planning evidence of earlier sessions"
            ],
            "{host}"
        );
        assert_eq!(
            shaped.kept,
            ["session identity", "open work plans", RESUME],
            "{host}"
        );
        assert!(
            shaped
                .context
                .starts_with("Craft session (pass as --session")
        );
        assert!(shaped.context.ends_with(
            "[Craft left out, to stay within the host's hook output limit: unfinished tasks of earlier sessions, planning evidence of earlier sessions]"
        ));
        // The wire carries exactly this text, and the same bytes every time.
        let wire = serialize(&result, host, "session-start").stdout;
        assert!(wire.contains("Craft left out") && !wire.contains("[truncated"));
        assert_eq!(wire, serialize(&result, host, "session-start").stdout);
    }
    // Within the ceiling nothing is left out or added.
    let small = json!({"decision": "allow", "session": {"id": "S1", "task": null,
        "plans": [{"name": "p"}]}});
    let shaped = shaped(&small, "claude", "session-start");
    assert!(shaped.dropped.is_empty() && !shaped.context.contains("left out"));
}

/// The summary line reaches the user as the host's own message field, only at an event
/// where the host shows one, and never beside a held stop.
#[test]
fn the_session_summary_is_a_user_message_where_the_host_shows_one() {
    let result = json!({"decision": "allow", "notice": "Craft this session: 1 lookup"});
    for host in ["claude", "codex", "gemini"] {
        let wire = serialize(&result, host, "turn-end").stdout;
        assert_eq!(
            wire, "{\"systemMessage\":\"Craft this session: 1 lookup\"}\n",
            "{host}"
        );
    }
    for host in ["cursor", "copilot", "jetbrains"] {
        assert_eq!(serialize(&result, host, "turn-end").stdout, "", "{host}");
    }
    assert_eq!(serialize(&result, "claude", "after-tool").stdout, "");
    // Beside a held stop the user is shown the stop's list, not the summary.
    let held = json!({"decision": "allow", "notice": "n", "stop": {"findings": [{"rule": "r"}]}});
    let shown: Value =
        serde_json::from_str(&serialize(&held, "claude", "turn-end").stdout).unwrap();
    assert!(
        shown["systemMessage"]
            .as_str()
            .unwrap()
            .starts_with("Craft stop review"),
        "{shown}"
    );
}

/// Delegated standards reach a host-started subagent only where its start event takes
/// context; the answer never blocks the start.
#[test]
fn delegated_standards_answer_the_subagent_start_in_each_host_wire() {
    let result = json!({"decision": "allow", "delegation": {"intent": "story", "standards": [{"id": "stories"}]}});
    let output = |host: &str| -> Value {
        let wire = serialize(&result, host, "agent-start");
        serde_json::from_str(&wire.stdout).unwrap_or(Value::Null)
    };
    for host in ["claude", "codex"] {
        let answer = &output(host)["hookSpecificOutput"];
        assert_eq!(answer["hookEventName"], "SubagentStart");
        let context = answer["additionalContext"].as_str().unwrap();
        assert!(context.contains("\"stories\"") && context.contains("delegated from"));
        assert!(answer.get("permissionDecision").is_none());
    }
    assert!(
        output("copilot")["additionalContext"]
            .as_str()
            .unwrap()
            .contains("\"stories\"")
    );
    for host in ["cursor", "gemini", "jetbrains"] {
        assert_eq!(serialize(&result, host, "agent-start").stdout, "");
    }
}

/// A stop summary holds the stop in each host's own words, only where it can continue.
#[test]
fn a_stop_summary_holds_the_stop_where_the_host_continues() {
    let result = json!({"decision": "allow", "stop": {"findings": [{"rule": "todo-marker"}]}});
    let reason = |output: &Value, field: &str| {
        output[field]
            .as_str()
            .unwrap()
            .starts_with("Craft stop review")
    };
    for host in ["codex", "copilot"] {
        let wire = serialize(&result, host, "turn-end");
        let output: Value = serde_json::from_str(&wire.stdout).unwrap();
        assert_eq!(output["decision"], "block", "{host}");
        assert!(reason(&output, "reason"), "{host}");
    }
    // Claude continues the turn from feedback without labelling it a hook error.
    let claude: Value =
        serde_json::from_str(&serialize(&result, "claude", "turn-end").stdout).unwrap();
    assert!(claude.get("decision").is_none(), "{claude}");
    assert_eq!(claude["hookSpecificOutput"]["hookEventName"], "Stop");
    assert!(reason(&claude["hookSpecificOutput"], "additionalContext"));
    // Context reaches only the model: the user sees the same list as the hook's message.
    assert_eq!(
        claude["systemMessage"],
        claude["hookSpecificOutput"]["additionalContext"]
    );
    let gemini: Value =
        serde_json::from_str(&serialize(&result, "gemini", "turn-end").stdout).unwrap();
    assert_eq!(gemini["decision"], "deny");
    assert!(reason(&gemini, "reason"));
    let cursor: Value =
        serde_json::from_str(&serialize(&result, "cursor", "turn-end").stdout).unwrap();
    assert!(reason(&cursor, "followup_message"));
    // No stop event, or another event: nothing to hold.
    assert_eq!(serialize(&result, "jetbrains", "turn-end").stdout, "");
    assert_eq!(serialize(&result, "claude", "after-tool").stdout, "");
    assert_eq!(
        serialize(&json!({"decision": "allow"}), "claude", "turn-end").stdout,
        ""
    );
}

/// The held stop reads as a short list: one line per finding and per review item, its
/// place, rule and what to check; no JSON.
#[test]
fn a_stop_summary_reads_as_one_line_per_item() {
    let summary = json!({
        "findings": [
            {"file": "src/a.rs", "line": 4, "rule": "todo-marker", "message": "TODO added"},
            {"file": "CHANGELOG.md", "rule": "unreleased-section", "standard": "changelog",
             "message": "no Unreleased section"}],
        "more": 2,
        "review": [
            {"id": "changelog-line", "level": "review", "summary": "Add a line.",
             "checklist": ["Observable change has a line", "Merged facts only"]},
            {"id": "module-map", "level": "review", "file": "docs/adr/0002.md",
             "checklist": ["Map and ADR change together"],
             "run": {"command": "npm", "args": ["run", "architecture"]}}]
    });
    let text = stop_line(&summary);
    let lines: Vec<&str> = text.lines().collect();
    assert!(lines[0].starts_with("Craft stop review, 6 items"), "{text}");
    assert_eq!(
        &lines[1..6],
        [
            "- src/a.rs:4 todo-marker: TODO added",
            "- CHANGELOG.md changelog unreleased-section: no Unreleased section",
            "- 2 more findings not listed",
            "- changelog-line: Add a line. — Observable change has a line; Merged facts only",
            "- module-map: Map and ADR change together; run `npm run architecture` (see docs/adr/0002.md)",
        ]
    );
    assert!(lines[6].starts_with("Address each"));
    assert!(!text.contains('{'), "{text}");
    let one = stop_line(&json!({"review": [{"id": "r", "summary": "s"}]}));
    assert!(one.starts_with("Craft stop review, 1 item "), "{one}");
}

/// Every answer shape, at every event a host maps, uses only the output fields that
/// host documents for that event: a field it ignores is lost, one it rejects breaks the
/// hook (Cursor refuses an answer outside its schema).
#[test]
fn every_answer_uses_only_fields_its_host_documents() {
    let documented: Value = serde_json::from_str(include_str!(
        "../../../tests/fixtures/runtime/host-response-fields.json"
    ))
    .unwrap();
    let hosts: Value = serde_json::from_str(HOSTS).unwrap();
    let results = [
        json!({"decision": "allow"}),
        json!({"decision": "allow", "context": {"kind": "routing", "skills": ["s"]}}),
        json!({"decision": "deny", "reason": "R", "context": {"kind": "answer"}}),
        json!({"decision": "deny", "rule": "read-secret-file", "reason": "R", "ask": "A"}),
        json!({"decision": "allow", "notice": "N", "standards": {"findings": []}}),
        json!({"decision": "allow", "notice": "N"}),
        json!({"decision": "allow", "notice": "N", "stop": {"findings": [{"rule": "r"}]}}),
        json!({"decision": "allow", "delegation": {"intent": "story", "standards": []}}),
        json!({"decision": "allow", "session": {"id": "s", "task": null}}),
    ];
    let mut checked = 0;
    for (host, table) in hosts.as_object().unwrap() {
        let Some(events) = table["events"].as_object().filter(|e| !e.is_empty()) else {
            continue;
        };
        for (lifecycle, mapping) in events {
            let name = mapping["name"].as_str().unwrap();
            let allowed = &documented["hosts"][host][name];
            assert!(allowed.is_object(), "{host} {name}: no documented fields");
            let listed = |field: &str| -> Vec<&str> {
                allowed[field]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|f| f.as_str().unwrap())
                    .collect()
            };
            for result in &results {
                let wire = serialize(result, host, lifecycle);
                if wire.stdout.is_empty() {
                    continue;
                }
                let output: Value = serde_json::from_str(&wire.stdout).unwrap();
                let label = format!("{host} {lifecycle} {result}: {output}");
                for field in output.as_object().unwrap().keys() {
                    assert!(listed("fields").contains(&field.as_str()), "{label}");
                }
                if let Some(nested) = output.get("hookSpecificOutput") {
                    assert_eq!(nested["hookEventName"], name, "{label}");
                    for field in nested.as_object().unwrap().keys() {
                        assert!(
                            listed("hookSpecificOutput").contains(&field.as_str()),
                            "{label}"
                        );
                    }
                }
                checked += 1;
            }
        }
    }
    assert!(checked > 40, "{checked}");
}

#[test]
fn a_continued_stop_is_marked_active() {
    let active = |payload: Value| normalize(&payload, "claude", "turn-end")["stopActive"] == true;
    assert!(active(json!({"stop_hook_active": true})));
    assert!(active(json!({"loop_count": 1})));
    assert!(!active(json!({"stop_hook_active": false, "loop_count": 0})));
    assert!(!active(json!({})));
}

#[test]
fn lone_surrogate_escapes_become_replacement_characters() {
    let parse = |text: &str| serde_json::from_str::<Value>(&repair_surrogates(text)).unwrap();
    assert_eq!(parse(r#""a\ud800b""#), json!("a\u{fffd}b"));
    assert_eq!(parse(r#""\udc00""#), json!("\u{fffd}"));
    assert_eq!(parse(r#""\ud83d\ude00""#), json!("\u{1f600}"));
    // An escaped backslash starts no escape sequence.
    assert_eq!(parse(r#""\\ud800""#), json!("\\ud800"));
    assert_eq!(
        parse(r#"{"k":"\ud800\ud800"}"#),
        json!({"k": "\u{fffd}\u{fffd}"})
    );
}
