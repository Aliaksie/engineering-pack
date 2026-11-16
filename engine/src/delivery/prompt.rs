//! The intent a submitted prompt asks for, from its leading words only. A request is read
//! clause by clause: courtesy and modal openers are skipped, the first clause that leads
//! with a known request names the intent, and a change request anywhere turns a lookup,
//! diagnosis or test request into implementation: a test-only route for a requested change
//! would exclude the change itself. For verbs that create or assess, what the request is
//! about (code, product artifacts, tests, decisions) names the intent. Statements, chat and
//! conversation questions name none: a missing hint costs less than a misleading one.
use crate::values::{js_blank, js_space};
use std::borrow::Cow;

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
    // Not a creating verb: what it puts somewhere is code, even "…in the message".
    (&["include"], "implement"),
    (&["handle"], "implement"),
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
/// What fails when a project check fails: "the test is failing again", "CI is red".
const FAILING_CHECKS: &[&str] = &[
    "test", "tests", "build", "builds", "pipeline", "ci", "suite", "check", "checks", "lint",
    "job", "jobs", "e2e",
];
/// How a statement reports a failure; the base form "fail" is left out, as it reads as a
/// request or a question ("make the test fail").
const FAILURES: &[&str] = &[
    "fails", "failed", "failing", "broke", "broken", "breaking", "red", "crashes", "crashed",
    "crashing", "flaky", "flakes",
];
/// Words that open a question or condition rather than a report: "when the build fails…".
const CONDITIONS: &[&str] = &[
    "when", "if", "whenever", "once", "after", "before", "until", "unless", "how", "what", "which",
    "who", "why", "where", "do", "does", "can", "could", "should", "would", "will",
];
/// A report that a project check fails, without a request: "the checkout test keeps
/// failing", "the build is broken again". It asks for diagnosis; other observations ("the
/// save button stays disabled") name no intent.
fn reports_failing_check(words: &[String]) -> bool {
    let is = |w: &String, set: &[&str]| set.contains(&w.as_str());
    !words.first().is_some_and(|w| is(w, CONDITIONS))
        && words
            .iter()
            .position(|w| is(w, FAILURES))
            .is_some_and(|at| words[..at].iter().any(|w| is(w, FAILING_CHECKS)))
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

/// A file name with an extension (`ANSWER.md`, `src/order.js`): a short extension that
/// starts with a letter after a stem of at least two characters or a path.
fn file_name(token: &str) -> bool {
    let Some((stem, extension)) = token.rsplit_once('.') else {
        return false;
    };
    (1..=5).contains(&extension.len())
        && extension.starts_with(|c: char| c.is_ascii_alphabetic())
        && extension.chars().all(|c| c.is_ascii_alphanumeric())
        && (stem.len() >= 2 || stem.contains('/'))
        && stem.ends_with(|c: char| c.is_ascii_alphanumeric() || matches!(c, '_' | '-'))
}

/// The word a file name reads as, so a request can tell where its output goes.
const FILE: &str = "_file_";

/// Keeps dotted names whole before clauses split at `.`: a file name reads as [`FILE`], and
/// `3.0` or `Policy.apply` as one word. Surrounding quotes, brackets and the trailing
/// sentence punctuation stay where they are.
fn protect(prompt: &str) -> String {
    let mut out = String::with_capacity(prompt.len());
    let mut rest = prompt;
    while let Some(start) = rest.find(|c: char| !js_space(c)) {
        out.push_str(&rest[..start]);
        rest = &rest[start..];
        let end = rest.find(js_space).unwrap_or(rest.len());
        let token = &rest[..end];
        rest = &rest[end..];
        let core = token.trim_start_matches(['(', '[', '{', '"', '\'', '`', '<']);
        let lead = &token[..token.len() - core.len()];
        let core = core.trim_end_matches([
            ')', ']', '}', '"', '\'', '`', '>', '.', ',', ';', ':', '!', '?',
        ]);
        let trail = &token[lead.len() + core.len()..];
        out.push_str(lead);
        if file_name(core) {
            out.push_str(FILE);
        } else {
            let chars: Vec<char> = core.chars().collect();
            for (i, c) in chars.iter().enumerate() {
                let inner = i > 0
                    && i + 1 < chars.len()
                    && chars[i - 1].is_ascii_alphanumeric()
                    && chars[i + 1].is_ascii_alphanumeric();
                out.push(if *c == '.' && inner { '_' } else { *c });
            }
        }
        out.push_str(trail);
    }
    out.push_str(rest);
    out
}

/// A hard-wrapped sentence as one line: a line break before a lowercase letter continues the
/// sentence. Any other line break (a list item, heading, new sentence) still ends a clause.
fn unwrap_lines(prompt: &str) -> String {
    let mut out = String::with_capacity(prompt.len());
    let mut lines = prompt.split('\n').peekable();
    while let Some(line) = lines.next() {
        out.push_str(line);
        if let Some(next) = lines.peek() {
            let continues = next
                .trim_start()
                .chars()
                .next()
                .is_some_and(char::is_lowercase);
            out.push(if continues { ' ' } else { '\n' });
        }
    }
    out
}

/// The clauses of a prompt as word keys, split at sentence and clause punctuation.
fn clauses(prompt: &str) -> Vec<Vec<String>> {
    protect(&unwrap_lines(prompt))
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

/// What a test produces: "the test run", "test results" talk about a run, not about tests.
const RUN_NOUNS: &[&str] = &[
    "run", "runs", "result", "results", "output", "outputs", "log", "logs",
];

fn subject(words: &[String]) -> Subject {
    let found = SUBJECTS
        .iter()
        .find(|(_, names)| words.iter().any(|w| names.contains(&w.as_str())))
        .map_or(Subject::Code, |(subject, _)| *subject);
    let about_run = words.windows(2).any(|pair| {
        matches!(pair[0].as_str(), "test" | "tests") && RUN_NOUNS.contains(&pair[1].as_str())
    });
    if found == Subject::Tests && about_run {
        Subject::Report
    } else {
        found
    }
}

/// Verbs that produce or change an artifact; the subject names the intent: "add a test" is
/// verification, "split the epic" product work, "update the module" implementation.
const CREATE: &[&str] = &[
    "write", "draft", "prepare", "turn", "record", "document", "produce", "create", "add",
    "update", "change", "split", "remove", "delete", "edit", "rewrite", "refine", "improve",
    "extend",
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

/// Verbs that put a result somewhere: "write the answer to ANSWER.md".
const OUTPUTS: &[&str] = &[
    "write", "save", "put", "store", "dump", "output", "add", "append", "record", "copy",
];
/// What an output instruction delivers: the result of the request, not new work.
const RESULTS: &[&str] = &[
    "it",
    "them",
    "this",
    "that",
    "these",
    "those",
    "everything",
    "answer",
    "answers",
    "result",
    "results",
    "list",
    "output",
    "findings",
    "callers",
    "mentions",
    "matches",
    "occurrences",
    "usages",
    "references",
    "locations",
    "paths",
    "names",
    "lines",
    "number",
    "count",
    "value",
    "total",
];
const DETERMINERS: &[&str] = &[
    "the", "a", "an", "your", "my", "our", "all", "every", "each", "of", "final", "full", "only",
    "just",
];

/// "Write them to `ANSWER.md`", "save the list to notes/smtp.md": where the result of the
/// request goes. A file written while doing the work is a side effect, never the request.
fn output_instruction(words: &[String]) -> bool {
    let is = |w: &String, set: &[&str]| set.contains(&w.as_str());
    let Some(to) = words
        .iter()
        .position(|w| matches!(w.as_str(), "to" | "in" | "into" | "as" | "under"))
    else {
        return false;
    };
    words.first().is_some_and(|w| is(w, OUTPUTS))
        && to > 1
        && words[1..to].iter().any(|w| is(w, RESULTS))
        && words[1..to]
            .iter()
            .all(|w| is(w, RESULTS) || is(w, DETERMINERS))
        && words[to..].iter().any(|w| w == FILE)
}

/// Who else a clause can be about: "a teammate wants to change…", "the team asked…".
const OTHERS: &[&str] = &[
    "teammate",
    "teammates",
    "colleague",
    "colleagues",
    "coworker",
    "coworkers",
    "team",
    "manager",
    "lead",
    "reviewer",
    "reviewers",
    "customer",
    "customers",
    "client",
    "clients",
    "someone",
    "somebody",
    "designer",
    "architect",
    "stakeholder",
    "stakeholders",
    "boss",
    "he",
    "she",
    "they",
];
/// What a third party does in a statement about them.
const REPORTS: &[&str] = &[
    "wants", "want", "asked", "asks", "needs", "plans", "is", "are", "will", "would", "said",
    "says", "wonders", "wondered", "reported", "reports",
];

/// A statement about what someone else wants or did: context, never the user's own request.
/// "A teammate wants to change the signature" does not ask to change it.
fn third_party(words: &[String]) -> bool {
    let start = words
        .iter()
        .position(|w| {
            !matches!(
                w.as_str(),
                "a" | "an" | "the" | "my" | "our" | "one" | "another"
            )
        })
        .unwrap_or(words.len());
    words
        .get(start)
        .is_some_and(|w| OTHERS.contains(&w.as_str()))
        && words[start + 1..]
            .iter()
            .take(2)
            .any(|w| REPORTS.contains(&w.as_str()))
}

/// The intent one sub-clause leads with.
fn lead(words: &[String]) -> Option<&'static str> {
    let words = skip_openers(words);
    // "Do not repeat work that is already complete": a prohibition bounds a request, it is
    // never one.
    let prohibition = [&["do", "not"][..], &["don", "t"], &["never"]]
        .iter()
        .any(|phrase| starts(words, phrase));
    if prohibition || third_party(words) || output_instruction(words) {
        return None;
    }
    // "…and asked which methods call it": the question someone asked is the request.
    if words
        .first()
        .is_some_and(|w| matches!(w.as_str(), "asked" | "asks" | "wonders" | "wondered"))
    {
        return lead(&words[1..]);
    }
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
        // "Give me a charter for testing the filters": the artifact asked for names the work;
        // "give me the file list" names none.
        "give" if next(1) == Some("me") => asked(about),
        "run" | "execute" if about == Subject::Exploration => Some("explore"),
        "run" | "execute" => names(CHECKS).then_some("run-check"),
        // "What is the status of the export?"; an HTTP or exit "status code" is no progress.
        "what" | "which"
            if names(&["progress"])
                || words.iter().enumerate().any(|(at, w)| {
                    w == "status" && !matches!(next(at + 1), Some("code" | "codes" | "line"))
                }) =>
        {
            Some("status")
        }
        "what" | "who" if names(&["affected", "impacted", "breaks", "break"]) => {
            Some("investigate")
        }
        "what" if starts(words, &["what", "would", "it", "take"]) => Some("plan"),
        // "What depends on the legacy API?", "what calls this?"; not "what should I use?".
        "what" if names(&["depends", "depend", "calls", "imports", "uses"]) => Some("locate"),
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
        // "Does our test suite cover expired coupons?"
        "does" | "do" if about == Subject::Tests && names(&["cover", "covers"]) => Some("verify"),
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
        // "How is the invoice export going?", "how's the migration coming along?"; a bare
        // "how is it going?" is conversation.
        "how" if words.last().is_some_and(|w| w == "going" || w == "along") => {
            (words.len() > 4).then_some("status")
        }
        // "How is the session token validated?": how something is done in the code.
        "how"
            if next(1).is_some_and(|w| matches!(w, "is" | "are" | "was" | "were"))
                && words.len() > 3
                && words
                    .last()
                    .is_some_and(|w| w.len() > 4 && (w.ends_with("ed") || w.ends_with("en"))) =>
        {
            Some("investigate")
        }
        // Progress on ongoing work: "where are we with the invoice export?", "where do we
        // stand on refunds?". A bare "where are we?" stays conversation.
        "where"
            if names(&["stand", "stands"])
                || (next(1).is_some_and(|w| AUXILIARIES.contains(&w))
                    && next(2).is_some_and(|w| SPEAKERS.contains(&w) || w == "things")
                    && matches!(next(3), Some("with" | "on"))) =>
        {
            Some("status")
        }
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
                return (names_failure(words) || reports_failing_check(words))
                    .then_some("investigate");
            };
            // A verb led by a statement is its subject; question words take auxiliaries.
            let question = matches!(first, "why" | "where" | "should");
            let statement = words
                .iter()
                .skip(phrase.len())
                .take(2)
                .any(|w| AUXILIARIES.contains(&w.as_str()) || STATEMENTS.contains(&w.as_str()));
            // "Test fails on CI": the check is the subject of a reported failure.
            if !question && statement {
                return reports_failing_check(words).then_some("investigate");
            }
            Some(*intent)
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
    requested(&own(prompt))
}

/// Text a host relays into the prompt on its own: a background task's completion, another
/// session's or a subagent's report, a reminder. What it says was not asked by the user.
const RELAYED: &[&str] = &["task-notification", "agent-message", "system-reminder"];

/// The prompt without relayed blocks: from a line that opens a [`RELAYED`] tag to its closing
/// tag, or to the end when it is not closed.
fn unrelayed(prompt: &str) -> Cow<'_, str> {
    let opens = |line: &str| {
        let line = line.trim_start();
        RELAYED.iter().find(|name| {
            line.strip_prefix('<')
                .and_then(|rest| rest.strip_prefix(**name))
                .is_some_and(|rest| rest.starts_with(['>', ' ', '\t']))
        })
    };
    if !prompt.lines().any(|line| opens(line).is_some()) {
        return Cow::Borrowed(prompt);
    }
    let mut out = String::with_capacity(prompt.len());
    let mut inside: Option<String> = None;
    for line in prompt.split_inclusive('\n') {
        match &inside {
            Some(close) => {
                if let Some(at) = line.find(close.as_str()) {
                    out.push_str(&line[at + close.len()..]);
                    inside = None;
                }
            }
            None => match opens(line) {
                Some(name) => {
                    let close = format!("</{name}>");
                    match line.find(&close) {
                        Some(at) => out.push_str(&line[at + close.len()..]),
                        None => inside = Some(close),
                    }
                }
                None => out.push_str(line),
            },
        }
    }
    Cow::Owned(out)
}

/// The user's own words: relayed text is left out, and "so we plan this?" followed by pasted
/// text is the question alone. When the question names no request, the material after it
/// does not decide one either.
pub(super) fn own(prompt: &str) -> Cow<'_, str> {
    match unrelayed(prompt) {
        Cow::Borrowed(text) => Cow::Borrowed(question(text)),
        Cow::Owned(text) => Cow::Owned(question(&text).to_owned()),
    }
}

fn question(prompt: &str) -> &str {
    match prompt.trim_start().split_once("\n\n") {
        Some((question, material)) if question.trim_end().ends_with('?') && !js_blank(material) => {
            question
        }
        _ => prompt,
    }
}

fn requested(prompt: &str) -> Option<Request> {
    let mut found: Option<Request> = None;
    for clause in clauses(prompt) {
        for (index, part) in parts(&clause).enumerate() {
            // A later sub-clause counts only when it leads with a request of its own.
            let Some(intent) = lead(part) else {
                // Only an opener ("go ahead and …") or a statement about someone else
                // leaves the request to the next part.
                if index == 0 && !skip_openers(part).is_empty() && !third_party(skip_openers(part))
                {
                    break;
                }
                continue;
            };
            let artifact = artifact(intent, part);
            match &mut found {
                None => found = Some(Request { intent, artifact }),
                // "Write a failing test, then fix it": the change includes its tests, while a
                // testing-only route would exclude the change.
                Some(Request {
                    intent: "locate" | "investigate" | "verify",
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
mod tests;
