//! The intent a submitted prompt asks for, from its leading words only. A request is read
//! clause by clause: courtesy and modal openers are skipped, the first clause that leads
//! with a known request names the intent, and a change request anywhere turns a lookup,
//! diagnosis or test request into implementation: a test-only route for a requested change
//! would exclude the change itself. For verbs that create or assess, what the request is
//! about (code, product artifacts, tests, decisions) names the intent. Statements, chat and
//! conversation questions name none: a missing hint costs less than a misleading one.
use crate::values::{js_blank, js_space};

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
    if third_party(words) || output_instruction(words) {
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
        "run" if about == Subject::Exploration => Some("explore"),
        "run" => names(CHECKS).then_some("run-check"),
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
    requested(own(prompt))
}

/// The user's own words: "so we plan this?" followed by pasted text is the question alone.
/// When the question names no request, the material after it does not decide one either.
pub(super) fn own(prompt: &str) -> &str {
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
mod tests {
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
}
