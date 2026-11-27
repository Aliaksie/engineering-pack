use super::*;

#[test]
fn a_prompt_names_its_ticket_by_url_or_by_a_key_it_calls_a_ticket() {
    for (prompt, expected) in [
        (
            "let's implement https://github.com/acme/shop/issues/145",
            Some("https://github.com/acme/shop/issues/145"),
        ),
        (
            "fix (https://gitlab.com/acme/shop/-/issues/7).",
            Some("https://gitlab.com/acme/shop/-/issues/7"),
        ),
        (
            "work on https://acme.atlassian.net/browse/SHOP-12 please",
            Some("https://acme.atlassian.net/browse/SHOP-12"),
        ),
        (
            "see https://linear.app/acme/issue/ENG-3/refunds",
            Some("https://linear.app/acme/issue/ENG-3/refunds"),
        ),
        ("start ticket SHOP-12", Some("SHOP-12")),
        ("Issue #42: the parser drops quotes", Some("#42")),
        ("please do story 7.", Some("7")),
        // Named by the phrase that starts work on it, or as `#12`.
        ("go ahead with 176", Some("176")),
        ("start work on 164", Some("164")),
        ("yes, close 132 and start on 133", Some("133")),
        ("yes go ahead with 132 and clean the board", Some("132")),
        ("pick up SHOP-12 next", Some("SHOP-12")),
        ("do 176 next", Some("176")),
        ("let's look at #176 today", Some("#176")),
        ("work on #9: the parser", Some("#9")),
        // Not a ticket: a pull request, a commit, an encoding name, a bare number.
        ("review https://github.com/acme/shop/pull/9", None),
        ("convert the files to UTF-8 and SHA-256", None),
        ("bump the limit to 145", None),
        ("the issues page is slow", None),
        // A number that counts things, one no start-work phrase names, or a request's `#12`.
        ("fix 3 failing tests", None),
        ("fix UTF-8 handling in the parser", None),
        ("start 2 workers and retry", None),
        ("177 merged, what next?", None),
        ("close 132", None),
        ("check PR #57 for security issues", None),
        ("review merge request #8", None),
        // A numbered list item after a start-work phrase is not a ticket.
        (
            "yes go ahead with\n1. Do the dogfood sessions\n2. Cut 0.0.8",
            None,
        ),
        ("go ahead with:\n  2) move the items", None),
        ("go ahead with\n1. pick up #176", Some("#176")),
    ] {
        assert_eq!(reference(prompt).as_deref(), expected, "{prompt}");
    }
}

#[test]
fn a_path_holds_a_contract_by_its_kind() {
    for (path, expected) in [
        ("schemas/project.schema.json", Some("schema")),
        ("api/openapi.yaml", Some("schema")),
        ("proto/order.proto", Some("schema")),
        ("db/migrations/0004_refunds.sql", Some("migration")),
        ("hosts/hooks.json", Some("hooks")),
        (".claude/settings.json", Some("hooks")),
        (".github/workflows/ci.yml", Some("ci")),
        ("src/schema.rs", None),
        ("engine/src/contracts.rs", None),
        ("docs/ROUTING.md", None),
    ] {
        assert_eq!(contract(path), expected, "{path}");
    }
}

fn decided(criteria: &str, questions: usize, modules: &[&str], contracts: &[&str]) -> Value {
    let owned = |items: &[&str]| items.iter().map(|s| s.to_string()).collect::<Vec<_>>();
    decide(&Facts {
        criteria,
        questions,
        labels: &[],
        modules: &owned(modules),
        contracts: &owned(contracts),
        status: None,
    })
}

/// One module, no contract and clear criteria is direct work; several modules, a contract or
/// unclear criteria plan first; open questions are asked before either.
#[test]
fn the_mode_follows_the_tickets_facts() {
    let direct = decided("clear", 0, &["application"], &[]);
    assert_eq!(direct["mode"], "direct");
    assert_eq!(
        line(&direct),
        "Craft: direct delivery (1 module, no contract change, clear acceptance criteria)"
    );

    let plan = decided("clear", 0, &["engine", "application", "docs"], &["schema"]);
    assert_eq!(plan["mode"], "plan");
    assert_eq!(
        line(&plan),
        "Craft suggests: plan → implement → review (3 modules, schema change)"
    );
    assert_eq!(decided("unclear", 0, &["engine"], &[])["mode"], "plan");
    assert_eq!(
        decided("missing", 0, &[], &["hooks", "hooks"])["reasons"],
        json!(["hooks change", "no acceptance criteria"])
    );

    let ask = decided("clear", 2, &["engine"], &[]);
    assert_eq!(ask["mode"], "ask");
    assert_eq!(
        line(&ask),
        "Craft suggests: answer the ticket's questions first (2 open questions in the ticket)"
    );
}

#[test]
fn labels_that_ask_for_discussion_or_design_decide_too() {
    let labelled = |label: &str| {
        decide(&Facts {
            criteria: "clear",
            questions: 0,
            labels: &[label.to_owned()],
            modules: &["engine".to_owned()],
            contracts: &[],
            status: None,
        })
    };
    assert_eq!(labelled("Needs-Info")["mode"], "ask");
    assert_eq!(labelled("epic")["reasons"], json!(["labelled epic"]));
    assert_eq!(labelled("bug")["mode"], "direct");
}

#[test]
fn the_offer_is_on_until_the_project_turns_it_off() {
    assert!(enabled(&json!({"routing": {}})));
    assert!(!enabled(&json!({"routing": {"ticketOffer": false}})));
}

#[test]
fn work_on_a_finished_issue_is_confirmed_before_anything_else() {
    let modules = ["engine".to_string(), "application".to_string()];
    let with = |status: Option<&str>| {
        decide(&Facts {
            criteria: "missing",
            questions: 2,
            labels: &[],
            modules: &modules,
            contracts: &[],
            status,
        })
    };
    for status in [
        "CLOSED",
        "closed",
        "Done",
        "Resolved",
        "Cancelled",
        "Won’t Do",
        " done ",
    ] {
        let decision = with(Some(status));
        assert_eq!(decision["mode"], "ask", "{status}");
        assert_eq!(decision["finished"], true, "{status}");
    }
    assert_eq!(
        line(&with(Some("Done"))),
        "Craft suggests: confirm the ticket is still to be done (issue Done)"
    );
    // An open issue, or none read, decides from the ticket's facts as before.
    for status in [
        Some("OPEN"),
        Some("opened"),
        Some("In Progress"),
        Some("Ready"),
        None,
    ] {
        let decision = with(status);
        assert_eq!(decision["mode"], "ask", "{status:?}");
        assert!(decision.get("finished").is_none(), "{status:?}");
        assert_eq!(
            decision["reasons"],
            json!(["2 open questions in the ticket"])
        );
    }
}

#[test]
fn only_work_that_may_change_code_is_delivered_directly_or_by_a_plan() {
    for intent in ["story", "plan", "explore", "verify", "review", "locate"] {
        assert!(!delivers_change(Some(intent)), "{intent}");
    }
    for intent in ["implement", "debug", "investigate", "run-check"] {
        assert!(delivers_change(Some(intent)), "{intent}");
    }
    assert!(delivers_change(None));
}

#[test]
fn a_ticket_names_its_plan_and_the_reference_linking_it() {
    for (reference, id, link) in [
        (
            "https://github.com/acme/shop/issues/145",
            "issue-145",
            "link:https://github.com/acme/shop/issues/145",
        ),
        (
            "https://gitlab.example/acme/shop/-/issues/12/?tab=x",
            "issue-12",
            "link:https://gitlab.example/acme/shop/-/issues/12/?tab=x",
        ),
        (
            "https://acme.atlassian.net/browse/SHOP-12",
            "shop-12",
            "link:https://acme.atlassian.net/browse/SHOP-12",
        ),
        ("SHOP-12", "shop-12", "tracker:SHOP-12"),
        ("#145", "issue-145", "tracker:#145"),
        ("145", "issue-145", "tracker:145"),
    ] {
        assert_eq!(plan_id(reference), id, "{reference}");
        assert!(crate::plan::slug(&plan_id(reference)));
        assert_eq!(plan_reference(reference).as_deref(), Some(link));
    }
}

#[test]
fn an_agent_shell_command_records_an_approval_however_it_is_wrapped() {
    for (command, expected) in [
        ("craft ticket --ticket '#7' --approve --session s1", true),
        (
            "/opt/bin/craft ticket --approve --ticket 7 --session s",
            true,
        ),
        (
            "cd repo && npm run ticket -- --ticket 7 --approve --session s",
            true,
        ),
        ("sh -c \"craft ticket --ticket 7 --approve=yes\"", true),
        ("craft ticket --ticket 7 --accept plan --session s", false),
        ("craft plan --plan ticket-7", false),
        ("echo --approve", false),
    ] {
        assert_eq!(approval_command(command), expected, "{command}");
    }
}

#[test]
fn only_an_approval_no_agent_command_recorded_under_running_hooks_is_the_users() {
    assert_eq!(command_source(false, true), "user");
    assert_eq!(command_source(true, true), "agent");
    // Without hooks seen in the session nothing shows the user ran it.
    assert_eq!(command_source(false, false), "agent");
    assert_eq!(command_source(true, false), "agent");
}

#[test]
fn an_approval_saved_without_a_source_reads_as_unknown_never_the_users() {
    let legacy = json!({"revision": 2, "digest": "d1", "at": 1});
    assert_eq!(approval_source(&legacy), "unknown");
    assert_eq!(
        approval_source(&json!({"digest": "d1", "source": "bogus"})),
        "unknown"
    );
    assert!(!approved_by("unknown").contains("user approved"));
    assert!(!approved_by("agent").starts_with("the user"));
    assert_eq!(
        approval_view(&legacy, Some("d1")),
        Some(json!({"revision": 2, "source": "unknown", "current": true}))
    );
    let reported = json!({"revision": 3, "digest": "d1", "source": "agent"});
    assert_eq!(
        approval_view(&reported, Some("d2")),
        Some(json!({"revision": 3, "source": "agent", "current": false}))
    );
    assert_eq!(approval_view(&json!(null), Some("d1")), None);
}

#[test]
fn only_an_approve_answer_to_this_plans_question_is_an_observed_approval() {
    let question = approval_question("issue-7", 3);
    assert_eq!(question, "Approve plan issue-7 revision 3?");
    let seen = |question: &str, answer: &str| {
        observed_approval(
            &json!([{"question": question, "answer": answer}]),
            "issue-7",
        )
    };
    assert_eq!(seen(&question, "Approve"), Some(3));
    assert_eq!(seen(&question, "Approve (Recommended)"), Some(3));
    assert_eq!(
        seen("Approve plan issue-7 revision 12? Tasks: a, b", "Approve"),
        Some(12)
    );
    for (question, answer) in [
        (question.as_str(), "Change the plan"),
        (question.as_str(), "Approve, but drop task b"),
        (question.as_str(), "Approve,Change the plan"),
        ("Approve plan issue-70 revision 3?", "Approve"),
        ("Approve plan issue-7 revision ?", "Approve"),
        ("Approve plan issue-7 revision 3x?", "Approve"),
        ("Plan first?", "Approve"),
    ] {
        assert_eq!(seen(question, answer), None, "{question} / {answer}");
    }
    // Another pair in the same question tool call may carry the approval.
    let pairs = json!([{"question": "Scope?", "answer": "All"},
        {"question": question, "answer": "Approve"}]);
    assert_eq!(observed_approval(&pairs, "issue-7"), Some(3));
    assert_eq!(observed_approval(&json!(null), "issue-7"), None);
}

#[test]
fn a_host_observed_approval_is_the_users_answer_never_unknown() {
    let seen = json!({"revision": 1, "digest": "d", "source": HOST_OBSERVED});
    assert_eq!(approval_source(&seen), HOST_OBSERVED);
    assert!(approved_by(HOST_OBSERVED).starts_with("the user approved"));
}

#[test]
fn an_agent_shell_command_records_a_delivery_choice_however_it_is_wrapped() {
    for (command, expected) in [
        ("craft ticket --ticket 7 --accept plan --session s", true),
        (
            "npm run ticket -- --ticket 7 --accept=direct --session s",
            true,
        ),
        ("craft ticket --ticket 7 --approve --session s", false),
        ("craft ticket --ticket 7 --accepted plan", false),
        ("echo --accept plan", false),
    ] {
        assert_eq!(accept_command(command), expected, "{command}");
    }
}

#[test]
fn work_with_no_ticket_has_a_plan_and_is_named_again_by_its_option() {
    let reference = work_reference("flaky-stop-review").unwrap();
    assert_eq!(plan_id(&reference), "work-flaky-stop-review");
    assert_eq!(plan_reference(&reference), None);
    assert_eq!(selector(&reference), "--work flaky-stop-review");
    assert_eq!(selector("#42"), "--ticket #42");
    assert_eq!(
        plan_reference("SHOP-12").as_deref(),
        Some("tracker:SHOP-12")
    );
    for name in ["", "two words", "a/b", "$(x)"] {
        assert_eq!(work_reference(name), None, "{name}");
    }
}

#[test]
fn a_ticket_reference_gives_the_id_its_tracker_uses() {
    for (reference, id) in [
        ("135", Some("135")),
        ("#135", Some("135")),
        ("https://github.com/acme/shop/issues/135", Some("135")),
        ("https://gitlab.com/acme/shop/-/issues/7?x=1", Some("7")),
        ("SHOP-12", Some("SHOP-12")),
        ("https://acme.atlassian.net/browse/SHOP-12", Some("SHOP-12")),
        ("work:cleanup", None),
        ("shop", None),
    ] {
        assert_eq!(issue_id(reference).as_deref(), id, "{reference}");
    }
}

#[test]
fn a_planned_delivery_is_reviewed_at_its_commit_by_a_separate_agent_where_the_host_has_one() {
    assert_eq!(reviewer(Some("Agent")), Reviewer::Separate("Agent".into()));
    assert_eq!(reviewer(None), Reviewer::IfAvailable);
    let separate = review_offer("SHOP-12", &reviewer(Some("Agent")));
    let maybe = review_offer("SHOP-12", &reviewer(None));
    for offer in [&separate, &maybe] {
        assert!(offer.contains(REVIEW_SKILL), "{offer}");
        assert!(offer.contains("SHOP-12"), "{offer}");
        assert!(!offer.contains("craft-delivery:review-code"), "{offer}");
    }
    // The host's tool is named only where Craft knows it; elsewhere the offer differs.
    assert!(separate.contains("Agent"), "{separate}");
    assert_ne!(separate, maybe);
}
