//! The read-only review guardrail. It recognizes direct publishing commands, literal
//! scripts passed to supported shells with `-c` and active `$()`/backtick substitutions.
//! Aliases, variable expansion and arbitrary interpreters stay outside the recognizer;
//! tool-side authorization and an isolated runner remain the enforcement boundary.
use crate::contracts::js_space;
use serde_json::{Value, json};

const SHELLS: [&str; 10] = [
    "sh",
    "bash",
    "dash",
    "zsh",
    "ksh",
    "/bin/sh",
    "/bin/bash",
    "/bin/dash",
    "/bin/zsh",
    "/bin/ksh",
];
const GIT_VALUE_OPTIONS: [&str; 7] = [
    "-C",
    "-c",
    "--git-dir",
    "--work-tree",
    "--namespace",
    "--config-env",
    "--super-prefix",
];
const REQUEST_ACTIONS: [&str; 9] = [
    "create", "comment", "note", "review", "approve", "merge", "close", "reopen", "edit",
];
/// Opaque scripts nested deeper than this are treated as publishing.
const MAX_DEPTH: usize = 16;

/// Whether `command` publishes: pushes, mutates a pull/merge request or issue, or writes
/// through the GitHub/GitLab API.
pub fn publishes(command: &str) -> bool {
    inspect(&command.chars().collect::<Vec<_>>(), 0)
}

/// The decision for one tool call in a project with the given effective settings.
pub fn read_only_decision(settings: &Value, command_execution: bool, command: &Value) -> Value {
    let read_only = settings["review"]["readOnly"] == Value::Bool(true);
    if read_only && command_execution && command.as_str().is_some_and(publishes) {
        json!({"decision": "deny", "reason": "This project is configured for read-only review. Return revision-bound findings; publication belongs to a separately authorized job."})
    } else {
        json!({"decision": "allow"})
    }
}

/// Words of each command segment. Quotes group words and may be empty; backslashes are
/// literal and unterminated quotes run to the end.
fn segments(command: &[char]) -> Vec<Vec<String>> {
    let mut result = Vec::new();
    let mut words = Vec::new();
    let mut word = String::new();
    let mut quote = None;
    let mut started = false;
    for &character in command {
        if let Some(open) = quote {
            if character == open {
                quote = None;
            } else {
                word.push(character);
                started = true;
            }
            continue;
        }
        if character == '\'' || character == '"' {
            quote = Some(character);
            started = true;
        } else if ";|&\n\r".contains(character) {
            if started {
                words.push(std::mem::take(&mut word));
                started = false;
            }
            if !words.is_empty() {
                result.push(std::mem::take(&mut words));
            }
        } else if js_space(character) {
            if started {
                words.push(std::mem::take(&mut word));
                started = false;
            }
        } else {
            word.push(character);
            started = true;
        }
    }
    if started {
        words.push(word);
    }
    if !words.is_empty() {
        result.push(words);
    }
    result
}

/// A short option group that includes `c` before any further `-`, as `-c`, `-lc` or `-ec`.
fn script_option(word: &str) -> bool {
    word.strip_prefix('-')
        .is_some_and(|rest| rest.split('-').next().is_some_and(|g| g.contains('c')))
}

/// Regular-expression `.` does not match these.
fn line_terminator(character: char) -> bool {
    matches!(character, '\n' | '\r' | '\u{2028}' | '\u{2029}')
}

fn mutating(method: &str) -> bool {
    ["POST", "PUT", "PATCH", "DELETE"]
        .iter()
        .any(|m| m.eq_ignore_ascii_case(method))
}

/// `mutation` as a whole word, where word characters are ASCII letters, digits and `_`.
fn names_mutation(argument: &str) -> bool {
    let word = |c: Option<char>| c.is_some_and(|c| c.is_ascii_alphanumeric() || c == '_');
    argument.match_indices("mutation").any(|(at, found)| {
        !word(argument[..at].chars().next_back())
            && !word(argument[at + found.len()..].chars().next())
    })
}

/// Field and input flags make an API request a POST unless a method is named.
fn field_option(argument: &str) -> bool {
    if ["--field", "--raw-field", "--input"]
        .iter()
        .any(|flag| argument == *flag || argument.starts_with(&format!("{flag}=")))
    {
        return true;
    }
    let short = argument.strip_prefix("-f").or(argument.strip_prefix("-F"));
    short.is_some_and(|rest| rest.chars().next().is_none_or(|c| !line_terminator(c)))
}

fn api_writes(args: &[String]) -> bool {
    let mut methods = Vec::new();
    for (index, argument) in args.iter().enumerate() {
        if argument == "-X" || argument == "--method" {
            methods.push(args.get(index + 1).map_or("", String::as_str));
        } else if let Some(rest) = argument.strip_prefix("--method=").or_else(|| {
            argument
                .strip_prefix("-X")
                .map(|rest| rest.strip_prefix('=').unwrap_or(rest))
        }) && !rest.contains(line_terminator)
        {
            methods.push(rest);
        }
    }
    // GraphQL reads are POST requests too; only a named mutation writes.
    args.iter().any(|a| names_mutation(a))
        || methods.iter().any(|m| mutating(m))
        || (methods.is_empty()
            && !args.iter().any(|a| a == "graphql")
            && args.iter().any(|a| field_option(a)))
}

fn publishing(words: &[String], depth: usize) -> bool {
    let Some((executable, args)) = words.split_first() else {
        return false;
    };
    if SHELLS.contains(&executable.as_str())
        && let Some(index) = args.iter().position(|word| script_option(word))
    {
        let script = args.get(index + 1).map_or("", String::as_str);
        return inspect(&script.chars().collect::<Vec<_>>(), depth + 1);
    }
    if executable == "git" {
        let mut index = 0;
        while index < args.len() && args[index].starts_with('-') {
            if GIT_VALUE_OPTIONS.contains(&args[index].as_str()) {
                index += 1;
            }
            index += 1;
        }
        return args.get(index).is_some_and(|a| a == "push");
    }
    if executable != "gh" && executable != "glab" {
        return false;
    }
    let first = args.first().map(String::as_str);
    if matches!(first, Some("pr" | "mr" | "issue"))
        && args
            .get(1)
            .is_some_and(|a| REQUEST_ACTIONS.contains(&a.as_str()))
    {
        return true;
    }
    first == Some("api") && api_writes(args)
}

fn inspect(command: &[char], depth: usize) -> bool {
    if depth >= MAX_DEPTH {
        return true;
    }
    if segments(command)
        .iter()
        .any(|words| publishing(words, depth))
    {
        return true;
    }
    let mut quote = None;
    let mut index = 0;
    while index < command.len() {
        let character = command[index];
        if character == '\\' && quote != Some('\'') {
            index += 2;
            continue;
        }
        if character == '\'' && quote != Some('"') {
            quote = if quote.is_some() { None } else { Some('\'') };
            index += 1;
            continue;
        }
        if quote == Some('\'') {
            index += 1;
            continue;
        }
        if character == '"' {
            quote = if quote.is_some() { None } else { Some('"') };
            index += 1;
            continue;
        }
        let backtick = character == '`';
        if !backtick && !(character == '$' && command.get(index + 1) == Some(&'(')) {
            index += 1;
            continue;
        }
        let start = index + if backtick { 1 } else { 2 };
        let mut end = start;
        let mut nesting = 1;
        let mut inner = None;
        while end < command.len() {
            let current = command[end];
            if current == '\\' && inner != Some('\'') {
                end += 2;
                continue;
            }
            if backtick {
                if current == '`' {
                    break;
                }
            } else if let Some(open) = inner {
                if current == open {
                    inner = None;
                }
            } else if current == '\'' || current == '"' {
                inner = Some(current);
            } else if current == '(' {
                nesting += 1;
            } else if current == ')' {
                nesting -= 1;
                if nesting == 0 {
                    break;
                }
            }
            end += 1;
        }
        let inner = &command[start.min(command.len())..end.min(command.len())];
        if inspect(inner, depth + 1) {
            return true;
        }
        index = end + 1;
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn options_and_words_follow_the_recognizer_rules() {
        for word in ["-c", "-lc", "-ec", "-xec"] {
            assert!(script_option(word), "{word}");
        }
        for word in ["--command", "-e-c", "c", "-", "-x"] {
            assert!(!script_option(word), "{word}");
        }
        assert!(names_mutation("query=mutation X"));
        assert!(names_mutation("mutation"));
        assert!(!names_mutation("mutations"));
        assert!(!names_mutation("pre_mutation"));
        assert!(field_option("-fa=b"));
        assert!(field_option("-f"));
        assert!(!field_option("-f\n"));
        assert!(!field_option("--fields"));
        assert_eq!(
            segments(&"a \"\" b;c".chars().collect::<Vec<_>>()),
            [vec!["a", "", "b"], vec!["c"]]
        );
    }
}
