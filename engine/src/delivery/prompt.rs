//! The intent a submitted prompt asks for, from its leading words only. A request is read
//! clause by clause: courtesy and modal openers are skipped, the first clause that leads
//! with a known request names the intent, and a later change request turns a lookup or
//! diagnosis into implementation. Statements, chat and conversation questions name none:
//! a missing hint costs less than a misleading one.
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
    &["i", "want", "you", "to"],
    &["i", "need", "you", "to"],
    &["i", "d", "like", "you", "to"],
    &["i", "would", "like", "you", "to"],
    &["i", "want", "to"],
    &["i", "need", "to"],
    &["we", "need", "to"],
];

/// Leading words per intent, longest phrases first within an intent.
const LEADS: &[(&[&str], &str)] = &[
    (&["find", "out"], "investigate"),
    (&["figure", "out"], "investigate"),
    (&["look", "into"], "investigate"),
    (&["why"], "investigate"),
    (&["explain"], "investigate"),
    (&["investigate"], "investigate"),
    (&["debug"], "investigate"),
    (&["diagnose"], "investigate"),
    (&["troubleshoot"], "investigate"),
    (&["look", "over"], "review"),
    (&["review"], "review"),
    (&["should", "we"], "plan"),
    (&["plan"], "plan"),
    (&["design"], "plan"),
    (&["propose"], "plan"),
    (&["check", "that"], "verify"),
    (&["check", "whether"], "verify"),
    (&["check", "if"], "verify"),
    (&["verify"], "verify"),
    (&["test"], "verify"),
    (&["who", "owns"], "locate"),
    (&["who", "maintains"], "locate"),
    (&["which", "file"], "locate"),
    (&["which", "files"], "locate"),
    (&["find"], "locate"),
    (&["locate"], "locate"),
    (&["search"], "locate"),
    (&["where"], "locate"),
    (&["research"], "research"),
    (&["explore"], "explore"),
    (&["implement"], "implement"),
    (&["fix"], "implement"),
    (&["add"], "implement"),
    (&["refactor"], "implement"),
    (&["rename"], "implement"),
    (&["change"], "implement"),
    (&["update"], "implement"),
    (&["remove"], "implement"),
    (&["delete"], "implement"),
    (&["create"], "implement"),
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

/// The intent one sub-clause leads with.
fn lead(words: &[String]) -> Option<&'static str> {
    let words = skip_openers(words);
    let first = words.first()?.as_str();
    let next = |at: usize| words.get(at).map(String::as_str);
    let object = |names: &[&str]| words.iter().any(|w| names.contains(&w.as_str()));
    match first {
        "add" if object(&["test", "tests"]) => return Some("verify"),
        "check" if object(&["pr", "diff", "change", "changes", "commit", "branch"]) => {
            return Some("review");
        }
        "how" if matches!(next(1), Some("does" | "do")) && object(&["work", "works"]) => {
            return Some("investigate");
        }
        "write" | "draft" => {
            return Some(if object(&["test", "tests"]) {
                "verify"
            } else if object(&["adr", "adrs"]) {
                "plan"
            } else if object(&["story", "stories"]) {
                "story"
            } else {
                "implement"
            });
        }
        "run" => {
            let checks = words.iter().skip(1).any(|w| CHECKS.contains(&w.as_str()));
            return checks.then_some("run-check");
        }
        "what" if words.iter().any(|w| w == "status") => return Some("status"),
        "where"
            if next(1).is_some_and(|w| AUXILIARIES.contains(&w) || w == "did" || w == "do")
                && next(2).is_some_and(|w| SPEAKERS.contains(&w))
                && words.len() <= 5 =>
        {
            return None;
        }
        _ => {}
    }
    let (phrase, intent) = LEADS.iter().find(|(phrase, _)| starts(words, phrase))?;
    // A verb led by a statement is its subject; question words take auxiliaries.
    let question = matches!(first, "why" | "where" | "who" | "which" | "should");
    let statement = words
        .iter()
        .skip(phrase.len())
        .take(2)
        .any(|w| AUXILIARIES.contains(&w.as_str()) || STATEMENTS.contains(&w.as_str()));
    (question || !statement).then_some(*intent)
}

/// Sub-clauses joined by "and" or "then": "find the bug and fix it".
fn parts(words: &[String]) -> impl Iterator<Item = &[String]> {
    words
        .split(|w| w == "and" || w == "then")
        .filter(|part| !part.is_empty())
}

pub(super) fn intent(prompt: &str) -> Option<&'static str> {
    let mut found = None;
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
            match found {
                None => found = Some(intent),
                Some("locate" | "investigate") if intent == "implement" => {
                    return Some(intent);
                }
                Some(_) => {}
            }
        }
    }
    found
}

#[cfg(test)]
mod tests {
    use super::intent;

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
    fn words_end_at_ascii_punctuation_but_not_at_other_letters() {
        assert_eq!(intent("/review"), Some("review"));
        assert_eq!(intent("review-x"), Some("review"));
        assert_eq!(intent("reviewé"), Some("review"));
        assert_eq!(intent("λ review"), None);
        assert_eq!(intent("review_x"), None);
        assert_eq!(intent("planning"), None);
    }
}
