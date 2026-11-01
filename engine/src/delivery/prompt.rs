//! The intent a submitted prompt asks for, from its leading words only. A request is read
//! clause by clause: courtesy and modal openers are skipped, the first clause that leads
//! with a known request names the intent, and a later change request turns a lookup or
//! diagnosis into implementation. For verbs that create or assess, what the request is
//! about (code, product artifacts, tests, decisions) names the intent. Statements, chat and
//! conversation questions name none: a missing hint costs less than a misleading one.
use crate::values::js_space;

/// Openers that do not change what is asked: "please", "can you", "I'd like you to".
const OPENERS: &[&[&str]] = &[
    &["please"],
    &["pls"],
    &["kindly"],
    &["ok"],
    &["okay"],
    &["now"],
    &["so"],
    &["also"],
    &["just"],
    &["then"],
    &["go", "ahead"],
    &["can", "you"],
    &["could", "you"],
    &["would", "you"],
    &["will", "you"],
    &["can", "we"],
    &["could", "we"],
    &["let", "s"],
    &["lets"],
    &["help", "me"],
    &["tell", "me"],
    &["show", "me"],
    &["i", "want", "you", "to"],
    &["i", "need", "you", "to"],
    &["i", "d", "like", "you", "to"],
    &["i", "would", "like", "you", "to"],
    &["i", "want", "to"],
    &["i", "need", "to"],
    &["we", "need", "to"],
    &["try", "to"],
];

/// Leading words per intent, longest phrases first within an intent.
const LEADS: &[(&[&str], &str)] = &[
    (&["find", "out"], "investigate"),
    (&["figure", "out"], "investigate"),
    (&["look", "into"], "investigate"),
    (&["why"], "investigate"),
    (&["explain"], "investigate"),
    (&["describe"], "investigate"),
    (&["walk", "me", "through"], "investigate"),
    (&["investigate"], "investigate"),
    (&["debug"], "investigate"),
    (&["diagnose"], "investigate"),
    (&["troubleshoot"], "investigate"),
    (&["look", "over"], "review"),
    (&["should", "we"], "plan"),
    (&["plan"], "plan"),
    (&["design"], "plan"),
    (&["propose"], "plan"),
    (&["check", "that"], "verify"),
    (&["check", "whether"], "verify"),
    (&["check", "if"], "verify"),
    (&["verify"], "verify"),
    (&["validate"], "verify"),
    (&["confirm"], "verify"),
    (&["retest"], "verify"),
    (&["test"], "verify"),
    (&["find"], "locate"),
    (&["locate"], "locate"),
    (&["search"], "locate"),
    (&["where"], "locate"),
    (&["research"], "research"),
    (&["explore"], "explore"),
    (&["reproduce"], "explore"),
    (&["implement"], "implement"),
    (&["fix"], "implement"),
    (&["address"], "implement"),
    (&["apply"], "implement"),
    (&["resolve"], "implement"),
    (&["clean", "up"], "implement"),
    (&["tidy"], "implement"),
    (&["simplify"], "implement"),
    (&["refactor"], "implement"),
    (&["rename"], "implement"),
    (&["build"], "implement"),
    (&["make"], "implement"),
    (&["replace"], "implement"),
    (&["move"], "implement"),
    (&["extract"], "implement"),
    (&["introduce"], "implement"),
    (&["migrate"], "implement"),
    (&["convert"], "implement"),
    (&["upgrade"], "implement"),
    (&["port"], "implement"),
    (&["stabilize"], "implement"),
];

/// Words that make a leading verb the subject of a statement: "review comments are addressed",
/// "test fails on CI".
const AUXILIARIES: &[&str] = &[
    "is", "are", "was", "were", "has", "have", "had", "looks", "seems",
];
const STATEMENTS: &[&str] = &[
    "fails",
    "failed",
    "passes",
    "passed",
    "works",
    "worked",
    "broke",
    "succeeded",
];
/// What a leading "run" runs when it asks for project checks.
const CHECKS: &[&str] = &[
    "check", "checks", "test", "tests", "lint", "linter", "suite",
];
/// What names a code change under review: "check the PR", "look at the diff".
const CHANGES: &[&str] = &["change", "changes"];
/// Code change artifacts, which also name a change where "change" alone may not: "check
/// whether the password change works" verifies behavior, "check whether the PR…" reviews.
const ARTIFACTS: &[&str] = &[
    "pr", "prs", "pull", "diff", "patch", "commit", "commits", "branch",
];
/// A clause naming a failure without a request: "I get NullPointerException at …".
fn names_failure(words: &[String]) -> bool {
    words.iter().any(|w| {
        matches!(
            w.as_str(),
            "traceback" | "panic" | "panicked" | "segfault" | "stacktrace"
        ) || ["exception", "error"]
            .iter()
            .any(|end| w.len() > end.len() && w.ends_with(end))
    })
}
/// Who a conversation question asks about: "where were we?", "where did we leave off?".
const SPEAKERS: &[&str] = &["we", "i"];

/// One word: a run of characters that are neither white space nor ASCII punctuation other
/// than `_`. Its key is the leading ASCII alphanumeric or `_` part, lowercased, so a
/// non-ASCII letter ends the key but stays in the word: `reviewé` reads as `review` while
/// `λ review` leads with `λ`.
fn key(word: &str) -> String {
    word.chars()
        .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
        .map(|c| c.to_ascii_lowercase())
        .collect()
}

fn separator(c: char) -> bool {
    js_space(c) || (c.is_ascii_punctuation() && c != '_')
}

/// The clauses of a prompt as word keys, split at sentence and clause punctuation.
fn clauses(prompt: &str) -> Vec<Vec<String>> {
    prompt
        .split(['.', '?', '!', ';', ',', '\n'])
        .map(|clause| {
            clause
                .split(separator)
                .filter(|w| !w.is_empty())
                .map(key)
                .collect()
        })
        .filter(|words: &Vec<String>| !words.is_empty())
        .collect()
}

fn starts(words: &[String], phrase: &[&str]) -> bool {
    words.len() >= phrase.len() && words.iter().zip(phrase).all(|(word, part)| word == part)
}

fn skip_openers(mut words: &[String]) -> &[String] {
    while let Some(opener) = OPENERS.iter().find(|opener| starts(words, opener)) {
        words = &words[opener.len()..];
    }
    words
}

/// What a request is about, from the artifacts it names: the small taxonomy that lets one
/// verb serve several actors. "Review the PRD" is product work, "review the change" is code
/// review, "write tests" is verification and "write an ADR" is a decision.
#[derive(Clone, Copy, PartialEq)]
enum Subject {
    Exploration,
    Report,
    Tests,
    Product,
    Decision,
    Code,
}

const SUBJECTS: &[(Subject, &[&str])] = &[
    (
        Subject::Exploration,
        &["charter", "charters", "exploratory", "reproduce"],
    ),
    // Text for the conversation or a tool, which no workflow owns: "write a commit message",
    // "update the plan with the new findings".
    (
        Subject::Report,
        &[
            "report", "reports", "message", "summary", "notes", "email", "findings",
        ],
    ),
    (
        Subject::Tests,
        &[
            "test",
            "tests",
            "scenario",
            "scenarios",
            "coverage",
            "covered",
        ],
    ),
    (
        Subject::Product,
        &[
            "spec",
            "specs",
            "specification",
            "prd",
            "requirement",
            "requirements",
            "story",
            "stories",
            "criteria",
            "epic",
        ],
    ),
    (
        Subject::Decision,
        &[
            "adr",
            "adrs",
            "decision",
            "decisions",
            "options",
            "tradeoffs",
            "offs",
            "proposal",
            "proposed",
            "rfc",
        ],
    ),
];

fn subject(words: &[String]) -> Subject {
    SUBJECTS
        .iter()
        .find(|(_, names)| words.iter().any(|w| names.contains(&w.as_str())))
        .map_or(Subject::Code, |(subject, _)| *subject)
}

/// Verbs that produce or change an artifact; the subject names the intent: "add a test" is
/// verification, "split the epic" product work, "update the module" implementation.
const CREATE: &[&str] = &[
    "write", "draft", "prepare", "turn", "record", "produce", "create", "add", "update", "change",
    "split", "remove", "delete", "edit", "rewrite", "refine", "improve", "extend",
];
/// Verbs that assess something; the subject names the intent.
const ASSESS: &[&str] = &[
    "review",
    "audit",
    "assess",
    "evaluate",
    "compare",
    "recommend",
    "estimate",
];

fn created(subject: Subject) -> Option<&'static str> {
    Some(match subject {
        Subject::Exploration => "explore",
        Subject::Report => return None,
        Subject::Tests => "verify",
        Subject::Product => "story",
        Subject::Decision => "plan",
        Subject::Code => "implement",
    })
}

/// What a question about an artifact asks for, when the artifact alone decides it.
fn asked(subject: Subject) -> Option<&'static str> {
    match subject {
        Subject::Exploration => Some("explore"),
        Subject::Product => Some("story"),
        Subject::Decision => Some("plan"),
        Subject::Tests => Some("verify"),
        Subject::Report | Subject::Code => None,
    }
}

/// Feedback on something: code review for a change, otherwise what the artifact names.
fn feedback(subject: Subject, change: bool) -> Option<&'static str> {
    match subject {
        Subject::Code if change => Some("review"),
        _ => asked(subject),
    }
}

fn assessed(verb: &str, subject: Subject) -> Option<&'static str> {
    Some(match (verb, subject) {
        (_, Subject::Report) => return None,
        (_, Subject::Product) => "story",
        (_, Subject::Decision) => "plan",
        ("review" | "audit", _) => "review",
        _ => "plan",
    })
}

/// The intent one sub-clause leads with.
fn lead(words: &[String]) -> Option<&'static str> {
    let words = skip_openers(words);
    let first = words.first()?.as_str();
    let next = |at: usize| words.get(at).map(String::as_str);
    let names = |names: &[&str]| words.iter().any(|w| names.contains(&w.as_str()));
    let about = subject(words);
    let statement = words
        .iter()
        .skip(1)
        .take(2)
        .any(|w| AUXILIARIES.contains(&w.as_str()) || STATEMENTS.contains(&w.as_str()));
    let artifact = names(ARTIFACTS);
    let change = artifact || names(CHANGES);
    let asks_feedback = starts(words, &["what", "do", "you", "think"])
        || (matches!(first, "any" | "give" | "share" | "your")
            && names(&["feedback", "thoughts", "opinion"]));
    match first {
        _ if statement && (CREATE.contains(&first) || ASSESS.contains(&first)) => None,
        _ if asks_feedback => feedback(about, change),
        _ if CREATE.contains(&first) => created(about),
        _ if ASSESS.contains(&first) => assessed(first, about),
        "do" if names(&["review"]) => assessed("review", about),
        "check" if matches!(next(1), Some("that" | "whether" | "if")) => {
            Some(if artifact { "review" } else { "verify" })
        }
        "check" => match about {
            Subject::Product => Some("story"),
            _ if change => Some("review"),
            _ => None,
        },
        // "Look at the diff", "go through the PR": a reading request is a review of a change.
        _ if (starts(words, &["look", "at"])
            || starts(words, &["take", "a", "look", "at"])
            || starts(words, &["go", "through"])
            || starts(words, &["go", "over"]))
            && !statement =>
        {
            feedback(about, change)
        }
        "list" => asked(about),
        "run" if about == Subject::Exploration => Some("explore"),
        "run" => names(CHECKS).then_some("run-check"),
        "what" | "which" if names(&["status", "progress"]) => Some("status"),
        "what" | "who" if names(&["affected", "impacted", "breaks", "break"]) => {
            Some("investigate")
        }
        "what" if starts(words, &["what", "would", "it", "take"]) => Some("plan"),
        "what"
            if matches!(next(1), Some("does" | "do"))
                && words.last().is_some_and(|w| w == "do" || w == "does")
                && words.len() > 3 =>
        {
            Some("investigate")
        }
        "who" | "which"
            if names(&[
                "owns",
                "own",
                "maintains",
                "depends",
                "depend",
                "use",
                "uses",
                "call",
                "calls",
                "imports",
            ]) =>
        {
            Some("locate")
        }
        "what" | "which" => match about {
            Subject::Code | Subject::Report => names(&["file", "files"]).then_some("locate"),
            _ => asked(about),
        },
        // "Are the tests in this PR enough?" assesses the change.
        "is" | "are" if about == Subject::Tests && change => Some("review"),
        "is" | "are" => asked(about).filter(|_| about != Subject::Exploration),
        "does" | "do" => {
            names(&["support", "supports", "handle", "handles", "already"]).then_some("investigate")
        }
        "how"
            if matches!(
                next(1),
                Some("big" | "much" | "long" | "hard" | "difficult")
            ) =>
        {
            Some("plan")
        }
        "how" if matches!(next(1), Some("far")) => Some("status"),
        // "How does the retry work?", not "how do I run it?".
        "how"
            if matches!(next(1), Some("does" | "do"))
                && !next(2).is_some_and(|w| SPEAKERS.contains(&w) || w == "you") =>
        {
            Some("investigate")
        }
        "where"
            if next(1).is_some_and(|w| AUXILIARIES.contains(&w) || w == "did" || w == "do")
                && next(2).is_some_and(|w| SPEAKERS.contains(&w))
                && words.len() <= 5 =>
        {
            None
        }
        _ => {
            let Some((phrase, intent)) = LEADS.iter().find(|(phrase, _)| starts(words, phrase))
            else {
                return names_failure(words).then_some("investigate");
            };
            // A verb led by a statement is its subject; question words take auxiliaries.
            let question = matches!(first, "why" | "where" | "should");
            let statement = words
                .iter()
                .skip(phrase.len())
                .take(2)
                .any(|w| AUXILIARIES.contains(&w.as_str()) || STATEMENTS.contains(&w.as_str()));
            (question || !statement).then_some(*intent)
        }
    }
}

/// Sub-clauses joined by "and" or "then": "find the bug and fix it".
fn parts(words: &[String]) -> impl Iterator<Item = &[String]> {
    words
        .split(|w| w == "and" || w == "then")
        .filter(|part| !part.is_empty())
}

/// What a prompt asks for: its intent and, where it decides the workflow, the artifact.
#[derive(Debug, PartialEq)]
pub(super) struct Request {
    pub intent: &'static str,
    pub artifact: Option<&'static str>,
}

/// The artifact a request with `intent` produces when it selects a workflow of its own:
/// assessing a specification is an audit, writing or recording an ADR is an ADR.
fn artifact(intent: &str, words: &[String]) -> Option<&'static str> {
    let words = skip_openers(words);
    let creates = words.first().is_some_and(|w| CREATE.contains(&w.as_str()));
    match intent {
        "story" if !creates => Some("spec-audit"),
        "plan" if creates && words.iter().any(|w| w == "adr" || w == "adrs") => Some("adr"),
        _ => None,
    }
}

pub(super) fn request(prompt: &str) -> Option<Request> {
    let mut found: Option<Request> = None;
    for clause in clauses(prompt) {
        for (index, part) in parts(&clause).enumerate() {
            // A later sub-clause counts only when it leads with a request of its own.
            let Some(intent) = lead(part) else {
                // Only an opener ("go ahead and …") leaves the request to the next part.
                if index == 0 && !skip_openers(part).is_empty() {
                    break;
                }
                continue;
            };
            let artifact = artifact(intent, part);
            match &mut found {
                None => found = Some(Request { intent, artifact }),
                Some(Request {
                    intent: "locate" | "investigate",
                    ..
                }) if intent == "implement" => {
                    return Some(Request { intent, artifact });
                }
                // "Should we split orders out? Write an ADR": the same intent names its artifact.
                Some(earlier) if earlier.intent == intent && earlier.artifact.is_none() => {
                    earlier.artifact = artifact;
                }
                Some(_) => {}
            }
        }
    }
    found
}

#[cfg(test)]
mod tests {
    use super::request;

    fn intent(prompt: &str) -> Option<&'static str> {
        request(prompt).map(|request| request.intent)
    }

    fn artifact(prompt: &str) -> Option<&'static str> {
        request(prompt).and_then(|request| request.artifact)
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
            "Test fails on CI",
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
}
