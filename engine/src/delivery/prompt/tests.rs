use super::request;

fn intent(prompt: &str) -> Option<&'static str> {
    request(prompt).map(|request| request.intent)
}

fn artifact(prompt: &str) -> Option<&'static str> {
    request(prompt).and_then(|request| request.artifact)
}

#[test]
fn questions_that_need_no_project_work_get_no_hint() {
    for prompt in [
        "Which HTTP status code means \"Too Many Requests\"?\n\nWrite only the number to `ANSWER.md` at the repository root. Do not change any other file.",
        "What status code does the API return for a missing order?",
        "Write only the number to `ANSWER.md` at the repository root.",
    ] {
        assert_eq!(intent(prompt), None, "{prompt}");
    }
    assert_eq!(
        intent("What is the status of the invoice export?"),
        Some("status")
    );
}

#[test]
fn where_the_result_goes_never_decides_the_request() {
    for (prompt, expected) in [
        (
            "Find every place we read SMTP_HOST and save the list to notes/smtp.md",
            Some("locate"),
        ),
        (
            "Which services call the billing API? Put the answer in docs/callers.md.",
            Some("locate"),
        ),
        (
            "Plan the migration and write the plan to PLAN.md.",
            Some("plan"),
        ),
        (
            "The 3.0 notes must list every mention of `LEGACY_TAX_RATE`. Write them to `ANSWER.md`.",
            None,
        ),
        ("Write them to ANSWER.md", None),
        // Writing a file is the request when what is written is new work.
        (
            "Write a regression test for the rounding bug",
            Some("verify"),
        ),
        (
            "Update ANSWER.md with the corrected totals",
            Some("implement"),
        ),
        (
            "Implement the discount cap described in PLAN.md",
            Some("implement"),
        ),
    ] {
        assert_eq!(intent(prompt), expected, "{prompt}");
    }
}

#[test]
fn someone_elses_plans_are_context_and_their_question_is_the_request() {
    assert_eq!(
        intent(
            "A teammate wants to change the signature of `DiscountPolicy.applyDiscount` and asked which methods call it directly. Write the answer to `ANSWER.md`."
        ),
        Some("locate")
    );
    // The task text as written: hard-wrapped mid-sentence.
    assert_eq!(
        intent(
            "A teammate wants to change the signature of `DiscountPolicy.applyDiscount` in this\nrepository and asked which methods call it directly.\n\nWrite the answer to `ANSWER.md` at the repository root, one caller per line, as\n`<path from the repository root>#<method name>`, for example\n`src/main/java/com/acme/billing/Example.java#run`. Do not change any other file.\n"
        ),
        Some("locate")
    );
    assert_eq!(intent("The team wants to migrate the orders table"), None);
    assert_eq!(
        intent("My colleague is going to rename Order. Where is it used?"),
        Some("locate")
    );
}

#[test]
fn pasted_material_after_a_question_does_not_decide() {
    assert_eq!(
        intent("so we paln this ?\n\nFix routing contamination from output artifacts."),
        None
    );
    assert_eq!(
        intent("can we plan this?\n\nAdd a retry to the webhook sender."),
        Some("plan")
    );
    assert_eq!(
        intent("Fix the retry.\n\nThe sender drops webhooks?"),
        Some("implement")
    );
}

#[test]
fn text_the_host_relays_is_not_the_users_request() {
    for prompt in [
        "<task-notification>\n<task-id>b1</task-id>\n<status>completed</status>\n<summary>Background command \"cargo test\" completed</summary>\n<result>Review the PRD, then fix the rounding bug.</result>\n</task-notification>",
        "Another Claude session sent a message:\n<agent-message from=\"a83f08e999a615e4d\">\n[Subagent hand-back] The text below is the final report of a subagent.\n\nWrite user stories for each persona. Review the acceptance criteria.\n</agent-message>",
        "<system-reminder>Write tests for the refund flow.</system-reminder>",
        "<agent-message from=\"a1\">\nFix the parser",
    ] {
        assert_eq!(intent(prompt), None, "{prompt}");
    }
    // The user's own words around relayed text still decide.
    assert_eq!(
        intent("<system-reminder>Write tests for x.</system-reminder>\nFix the rounding bug"),
        Some("implement")
    );
    assert_eq!(
        intent("Plan the migration\n<agent-message from=\"a1\">\nFix the parser\n</agent-message>"),
        Some("plan")
    );
    // A tag the host does not relay is the user's text.
    assert_eq!(
        intent("<notes>\nFix the rounding bug\n</notes>"),
        Some("implement")
    );
}

#[test]
fn a_prohibition_bounds_a_request_and_is_never_one() {
    assert_eq!(
        intent(
            "Your claude.ai usage limit has reset. Continue the task you were working on when the limit was reached; do not repeat work that is already complete."
        ),
        None
    );
    assert_eq!(
        intent("Don't fix it yet, explain why it fails"),
        Some("investigate")
    );
    assert_eq!(
        intent("Never mind, fix the rounding bug"),
        Some("implement")
    );
    assert_eq!(
        intent("Fix the parser and do not touch the tests"),
        Some("implement")
    );
    assert_eq!(
        intent("Does checkout already send an email?"),
        Some("investigate")
    );
}

#[test]
fn a_line_break_inside_a_sentence_does_not_end_it() {
    assert_eq!(
        intent("Where is the discount\ncalculated for promo codes?"),
        Some("locate")
    );
    assert_eq!(
        intent("Plan the discount cap for each\nstorefront, but do not implement it yet."),
        Some("plan")
    );
    // A new line that starts a sentence or a list item still ends the clause.
    assert_eq!(
        intent("The team wants a cap\nFix the rounding bug"),
        Some("implement")
    );
    assert_eq!(
        intent("Notes from the call\n- fix the rounding bug"),
        Some("implement")
    );
}

#[test]
fn dotted_names_stay_one_word() {
    assert_eq!(
        intent("Explain `Order.total` in src/order.js"),
        Some("investigate")
    );
    assert_eq!(intent("Where is v2.3 configured?"), Some("locate"));
}

#[test]
fn assessing_a_spec_and_recording_an_adr_name_their_artifact() {
    for prompt in [
        "Review the PRD for bulk cancellation",
        "What is missing from this spec?",
        "Can you check the requirements for contradictions?",
        "Is the requirement clear enough?",
    ] {
        assert_eq!(artifact(prompt), Some("spec-audit"), "{prompt}");
    }
    for prompt in [
        "Write user stories for bulk cancellation",
        "Split the epic into smaller stories",
        "Review the ADR for the queue",
        "Plan the migration",
    ] {
        assert_eq!(artifact(prompt), None, "{prompt}");
    }
    assert_eq!(artifact("Write an ADR for event sourcing"), Some("adr"));
    assert_eq!(
        artifact("Should we split orders out? Write an ADR"),
        Some("adr")
    );
}

#[test]
fn openers_do_not_change_the_request() {
    for prompt in [
        "Please review PR 42",
        "can you review this?",
        "I'd like you to review it",
    ] {
        assert_eq!(intent(prompt), Some("review"), "{prompt}");
    }
    assert_eq!(intent("Go ahead and fix it"), Some("implement"));
    assert_eq!(intent("yes, go ahead"), None);
}

#[test]
fn statements_and_conversation_questions_name_no_intent() {
    for prompt in [
        "Review comments are addressed, push the fix",
        "Fix is merged",
        "Where were we?",
        "where did we leave off",
        "thanks!",
    ] {
        assert_eq!(intent(prompt), None, "{prompt}");
    }
    assert_eq!(
        intent("Where do we configure the pool size?"),
        Some("locate")
    );
}

#[test]
fn a_later_change_request_turns_a_lookup_into_implementation() {
    assert_eq!(intent("Find the bug and fix it"), Some("implement"));
    assert_eq!(intent("Why is it slow? Fix it"), Some("implement"));
    assert_eq!(
        intent("Review the change and fix any issues"),
        Some("review")
    );
    assert_eq!(intent("Plan it, then implement it"), Some("plan"));
}

#[test]
fn a_requested_change_with_a_test_is_implementation_in_either_order() {
    for prompt in [
        "Fix the rounding bug and write a regression test",
        "Write a failing test for the rounding bug, then fix it",
        "tooling/native-install.mjs: when --current-target is given on an unsupported platform, the error says only \"unsupported current native platform\". Include the detected platform and architecture in the message, and add a test for it. Keep the change minimal.",
        "Include the order id in the refund log line and add a test",
        "Handle an empty cart in the discount calculation and cover it with tests",
    ] {
        assert_eq!(intent(prompt), Some("implement"), "{prompt}");
    }
    // Test work alone keeps its testing-only route.
    assert_eq!(intent("Add a test for the refund rounding"), Some("verify"));
    assert_eq!(
        intent("Write tests for the refund flow, don't change product code"),
        Some("verify")
    );
}

#[test]
fn a_test_run_is_talked_about_not_tested() {
    for prompt in [
        "okey add it all in plan ... what with test run ?",
        "what with the test run?",
        "what about the test results from CI?",
        "are the test logs uploaded?",
    ] {
        assert_ne!(intent(prompt), Some("verify"), "{prompt}");
    }
    assert_eq!(intent("what tests cover the refund flow?"), Some("verify"));
    assert_eq!(intent("Add a test for the refund rounding"), Some("verify"));
    assert_eq!(intent("Run the tests"), Some("run-check"));
    assert_eq!(
        intent("check whether the test run passes after the fix"),
        Some("verify")
    );
}

#[test]
fn the_object_decides_what_writing_or_running_asks_for() {
    assert_eq!(intent("Write tests for the refund flow"), Some("verify"));
    assert_eq!(intent("Write an ADR"), Some("plan"));
    assert_eq!(intent("Write a parser for the config"), Some("implement"));
    assert_eq!(intent("Run the order module checks"), Some("run-check"));
    assert_eq!(intent("Run the app"), None);
}

#[test]
fn what_a_request_is_about_decides_the_intent_across_actors() {
    assert_eq!(
        intent("Review the change in services/orders"),
        Some("review")
    );
    assert_eq!(
        intent("Review the PRD for bulk cancellation"),
        Some("story")
    );
    assert_eq!(intent("Review the ADR for the queue"), Some("plan"));
    assert_eq!(
        intent("Draft acceptance criteria for refunds"),
        Some("story")
    );
    assert_eq!(
        intent("Prepare an exploratory test charter"),
        Some("explore")
    );
    assert_eq!(intent("Write a bug report for the coupon field"), None);
    assert_eq!(
        intent("Is the cancellation flow covered by tests?"),
        Some("verify")
    );
    assert_eq!(intent("What are the trade-offs here?"), Some("plan"));
    assert_eq!(intent("Which team owns pricing?"), Some("locate"));
}

#[test]
fn changing_verbs_follow_what_they_change() {
    assert_eq!(intent("Create stories for loyalty points"), Some("story"));
    assert_eq!(intent("Split the epic into smaller stories"), Some("story"));
    assert_eq!(
        intent("Create a REST endpoint for refunds"),
        Some("implement")
    );
    assert_eq!(intent("Add a quantity limit to Order"), Some("implement"));
    assert_eq!(intent("update the plan with the new findings"), None);
    assert_eq!(intent("Write a commit message for these changes"), None);
}

#[test]
fn reading_and_feedback_requests_about_a_change_are_reviews() {
    for prompt in [
        "What do you think of these changes?",
        "Any feedback on my changes?",
        "Look at the diff and point out naming problems",
        "Go through the diff and flag anything risky",
        "Check whether the migration in PR 88 is backwards compatible",
        "Are the tests in this PR enough?",
    ] {
        assert_eq!(intent(prompt), Some("review"), "{prompt}");
    }
    assert_eq!(
        intent("Fix the feedback form validation"),
        Some("implement")
    );
    assert_eq!(intent("Look at the weather"), None);
    assert_eq!(
        intent("Check whether the password change works"),
        Some("verify")
    );
}

#[test]
fn questions_name_impact_effort_progress_and_behavior() {
    assert_eq!(
        intent("What breaks if we change the ID format?"),
        Some("investigate")
    );
    assert_eq!(
        intent("Who is affected if we drop it?"),
        Some("investigate")
    );
    assert_eq!(
        intent("What would it take to split billing out?"),
        Some("plan")
    );
    assert_eq!(intent("How far along is the refund work?"), Some("status"));
    assert_eq!(
        intent("What does the ReconcileJob do?"),
        Some("investigate")
    );
    assert_eq!(
        intent("How do orders and inventory talk?"),
        Some("investigate")
    );
    assert_eq!(intent("How do I run it?"), None);
    assert_eq!(
        intent("Does checkout already send an email?"),
        Some("investigate")
    );
    assert_eq!(intent("Which modules depend on pricing?"), Some("locate"));
    assert_eq!(intent("Is the requirement clear enough?"), Some("story"));
}

#[test]
fn a_named_failure_asks_for_diagnosis() {
    assert_eq!(
        intent("I get NullPointerException at Checkout.java:42"),
        Some("investigate")
    );
    assert_eq!(intent("The error message is confusing"), None);
}

#[test]
fn a_reported_failing_check_asks_for_diagnosis() {
    for prompt in [
        "The test is failing again",
        "Test fails on CI",
        "The checkout e2e tests keep failing",
        "CI is red on main",
        "the nightly build is broken again",
        "Our integration suite crashed overnight",
    ] {
        assert_eq!(intent(prompt), Some("investigate"), "{prompt}");
    }
    // A request, question or condition about a failing check keeps its own reading, and
    // an observation that names no check names no intent.
    for (prompt, expected) in [
        ("Write a failing test for the rounding bug", Some("verify")),
        ("The tests are failing, fix them", Some("implement")),
        ("Run the suite and tell me what fails", Some("run-check")),
        ("How do I make the test fail?", None),
        ("When the build fails, who gets notified?", None),
        ("The save button stays disabled after editing", None),
        ("Fix is merged", None),
    ] {
        assert_eq!(intent(prompt), expected, "{prompt}");
    }
}

#[test]
fn progress_on_ongoing_work_is_a_status_question() {
    for prompt in [
        "Where are we with the invoice export?",
        "where are we on the refund migration",
        "Where do we stand on bulk cancellation?",
        "How is the invoice export going?",
        "How's the payments migration coming along?",
    ] {
        assert_eq!(intent(prompt), Some("status"), "{prompt}");
    }
    assert_eq!(intent("Where are we?"), None);
    assert_eq!(intent("How is it going?"), None);
    assert_eq!(
        intent("Where is the invoice export implemented?"),
        Some("locate")
    );
    assert_eq!(
        intent("How is the session token validated?"),
        Some("investigate")
    );
    assert_eq!(intent("How are you?"), None);
    assert_eq!(
        intent("don't change anything yet, just tell me where the tax is applied"),
        Some("locate")
    );
}

#[test]
fn words_end_at_ascii_punctuation_but_not_at_other_letters() {
    assert_eq!(intent("/review"), Some("review"));
    assert_eq!(intent("review-x"), Some("review"));
    assert_eq!(intent("reviewé"), Some("review"));
    assert_eq!(intent("λ review"), None);
    assert_eq!(intent("review_x"), None);
    assert_eq!(intent("planning"), None);
}

#[test]
fn other_wordings_of_a_known_request_name_the_same_intent() {
    for (prompt, expected) in [
        (
            "Execute the regression suite for payments",
            Some("run-check"),
        ),
        ("Execute the plan we agreed on", None),
        ("Does our test suite cover expired coupons?", Some("verify")),
        ("Do the tests pass on your machine?", None),
        (
            "Give me a charter for testing the new search filters",
            Some("explore"),
        ),
        ("Give me test scenarios for the refund flow", Some("verify")),
        ("Give me the list of changed files", None),
        ("Give me a summary of the session", None),
        ("What depends on the legacy customer API?", Some("locate")),
        ("What calls applyDiscount?", Some("locate")),
        ("What should I use for logging?", None),
        ("Document the decision to use Kafka in an ADR", Some("plan")),
        ("Document the retry helper", Some("implement")),
    ] {
        assert_eq!(intent(prompt), expected, "{prompt}");
    }
    assert_eq!(
        artifact("Document the decision to use Kafka in an ADR"),
        Some("adr")
    );
}
