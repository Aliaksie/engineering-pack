//! The read-only review guardrail. It recognizes direct publishing commands, literal
//! scripts passed to supported shells with `-c` and active `$()`/backtick substitutions.
//! The text of a here-document is data unless a supported shell is named on the line that
//! opens it; substitutions in it are still read when its delimiter is unquoted.
//! Aliases, variable expansion and arbitrary interpreters stay outside the recognizer;
//! tool-side authorization and an isolated runner remain the enforcement boundary.
use crate::values::js_space;
use serde_json::{Value, json};

pub mod artifact;
pub mod followup;

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
    let mut too_deep = false;
    inspect(
        &command.chars().collect::<Vec<_>>(),
        0,
        &mut publishing,
        &mut too_deep,
    ) || too_deep
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

/// Visit the words of every command `command` runs: each segment, the script a supported
/// shell is given with `-c` and each `$()`/backtick substitution, to any depth the recognizer
/// reads. Stops at the first segment `visit` accepts. Returns whether something was nested
/// too deep to read.
pub(crate) fn walk(command: &str, visit: &mut dyn FnMut(&[String]) -> bool) -> bool {
    let mut too_deep = false;
    inspect(
        &command.chars().collect::<Vec<_>>(),
        0,
        &mut |words| visit(words),
        &mut too_deep,
    );
    too_deep
}

fn publishing(words: &[String]) -> bool {
    let Some((executable, args)) = words.split_first() else {
        return false;
    };
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

/// One segment: the script of a shell `-c` is read in its place, any other goes to `visit`.
fn segment(
    words: &[String],
    depth: usize,
    visit: &mut dyn FnMut(&[String]) -> bool,
    too_deep: &mut bool,
) -> bool {
    if let Some((executable, args)) = words.split_first()
        && SHELLS.contains(&executable.as_str())
        && let Some(index) = args.iter().position(|word| script_option(word))
    {
        let script = args.get(index + 1).map_or("", String::as_str);
        return inspect(
            &script.chars().collect::<Vec<_>>(),
            depth + 1,
            visit,
            too_deep,
        );
    }
    visit(words)
}

/// Whether `visit` accepted a segment of `command`. Nesting past [`MAX_DEPTH`] is not read
/// and sets `too_deep`.
fn inspect(
    command: &[char],
    depth: usize,
    visit: &mut dyn FnMut(&[String]) -> bool,
    too_deep: &mut bool,
) -> bool {
    if depth >= MAX_DEPTH {
        *too_deep = true;
        return false;
    }
    let (code, expanded) = here_documents(command);
    let command = code.as_deref().unwrap_or(command);
    segments(command)
        .iter()
        .any(|words| segment(words, depth, visit, too_deep))
        || substitutions(command, true, depth, visit, too_deep)
        || expanded
            .iter()
            .any(|body| substitutions(body, false, depth, visit, too_deep))
}

/// A here-document a line opens: its delimiter, whether `<<-` strips leading tabs from its
/// lines and whether the delimiter was quoted, which keeps the text from being expanded.
struct HereDocument {
    delimiter: Vec<char>,
    tabs: bool,
    quoted: bool,
}

/// The here-document operator at `at`, if one is there, and the index after its delimiter.
fn here_operator(command: &[char], at: usize) -> Option<(HereDocument, usize)> {
    let rest = command.get(at..)?;
    if !rest.starts_with(&['<', '<']) {
        return None;
    }
    let mut index = at + 2;
    let tabs = command.get(index) == Some(&'-');
    index += usize::from(tabs);
    while matches!(command.get(index), Some(' ' | '\t')) {
        index += 1;
    }
    let mut delimiter = Vec::new();
    let mut quoted = false;
    let mut quote = None;
    while let Some(&character) = command.get(index) {
        if let Some(open) = quote {
            if character == open {
                quote = None;
            } else {
                delimiter.push(character);
            }
        } else if character == '\'' || character == '"' {
            quote = Some(character);
            quoted = true;
        } else if character == '\\' {
            quoted = true;
            index += 1;
            delimiter.extend(command.get(index));
        } else if js_space(character) || ";|&<>()".contains(character) {
            break;
        } else {
            delimiter.push(character);
        }
        index += 1;
    }
    let document = HereDocument {
        delimiter,
        tabs,
        quoted,
    };
    (!document.delimiter.is_empty() && quote.is_none()).then_some((document, index))
}

/// Where the line starting at `start` ends: its newline, or the end of `command`.
fn line_end(command: &[char], start: usize) -> usize {
    command[start..]
        .iter()
        .position(|character| *character == '\n')
        .map_or(command.len(), |length| start + length)
}

/// `command` without the here-document text that is data, and the texts the shell expands.
/// `None` when nothing was removed. A document stays in place, read as commands, when a
/// supported shell is named on the line that opens it, and when its closing line is
/// missing: `<<` is then more likely arithmetic than a document that hides what follows.
fn here_documents(command: &[char]) -> (Option<Vec<char>>, Vec<Vec<char>>) {
    if !command.windows(2).any(|pair| pair == ['<', '<']) {
        return (None, Vec::new());
    }
    let mut code = Vec::with_capacity(command.len());
    let mut expanded = Vec::new();
    let mut removed = false;
    let mut opened: Vec<HereDocument> = Vec::new();
    let mut line = 0;
    let mut quote = None;
    let mut index = 0;
    while index < command.len() {
        let character = command[index];
        if let Some(open) = quote {
            if character == '\\' && open == '"' {
                code.extend(command.get(index..index + 2).unwrap_or(&command[index..]));
                index += 2;
                continue;
            }
            if character == open {
                quote = None;
            }
        } else if character == '\\' {
            code.extend(command.get(index..index + 2).unwrap_or(&command[index..]));
            index += 2;
            continue;
        } else if character == '\'' || character == '"' {
            quote = Some(character);
        } else if command[index..].starts_with(&['<', '<', '<']) {
            code.extend(&command[index..index + 3]);
            index += 3;
            continue;
        } else if let Some((document, after)) = here_operator(command, index) {
            code.extend(&command[index..after]);
            opened.push(document);
            index = after;
            continue;
        } else if character == '\n' {
            if !opened.is_empty() {
                let runs_it = code[line..]
                    .split(|c: &char| js_space(*c) || ";|&()$`<>".contains(*c))
                    .any(|word| {
                        let name = word.rsplit(|c| *c == '/').next().unwrap_or(word);
                        SHELLS
                            .iter()
                            .any(|shell| shell.chars().eq(name.iter().copied()))
                    });
                // The texts follow in the order their documents were opened.
                let mut start = index + 1;
                let mut closing = index;
                let mut texts = Vec::new();
                for document in &opened {
                    let from = start;
                    let mut closed = false;
                    while start < command.len() && !closed {
                        let end = line_end(command, start);
                        let mut text = &command[start..end];
                        while document.tabs && text.first() == Some(&'\t') {
                            text = &text[1..];
                        }
                        closed = text == document.delimiter;
                        if closed {
                            texts.push((document.quoted, &command[from..start]));
                            closing = end;
                        }
                        start = (end + 1).min(command.len());
                    }
                    if !closed {
                        break;
                    }
                }
                if texts.len() == opened.len() {
                    if runs_it {
                        // Kept as commands, as written: quotes in the text open nothing.
                        code.extend(&command[index..closing]);
                    } else {
                        expanded.extend(
                            texts
                                .iter()
                                .filter(|(quoted, _)| !quoted)
                                .map(|(_, text)| text.to_vec()),
                        );
                        removed = true;
                    }
                    // The newline that ends the last closing line, or the end.
                    index = closing;
                }
                opened.clear();
                if index >= command.len() {
                    break;
                }
            }
            line = code.len() + 1;
        }
        code.push(command[index]);
        index += 1;
    }
    (removed.then_some(code), expanded)
}

/// Whether `visit` accepted a segment of a `$()` or backtick substitution in `command`.
/// With `quotes` off the text is a here-document, where quotes group nothing.
fn substitutions(
    command: &[char],
    quotes: bool,
    depth: usize,
    visit: &mut dyn FnMut(&[String]) -> bool,
    too_deep: &mut bool,
) -> bool {
    let mut quote = None;
    let mut index = 0;
    while index < command.len() {
        let character = command[index];
        if character == '\\' && quote != Some('\'') {
            index += 2;
            continue;
        }
        if quotes && character == '\'' && quote != Some('"') {
            quote = if quote.is_some() { None } else { Some('\'') };
            index += 1;
            continue;
        }
        if quote == Some('\'') {
            index += 1;
            continue;
        }
        if quotes && character == '"' {
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
        if inspect(inner, depth + 1, visit, too_deep) {
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

    #[test]
    fn a_here_document_publishes_only_when_a_shell_runs_it() {
        for command in [
            "cat > review.md <<'EOF'\nRun git push after approval.\nEOF",
            "cat <<EOF\ngh pr create is the author's step\nEOF\ngit status",
        ] {
            assert!(!publishes(command), "{command}");
        }
        for command in [
            "sh <<'EOF'\ngit push\nEOF",
            "cat <<EOF\n$(gh pr merge 7)\nEOF",
            "cat <<EOF\nnotes\nEOF\ngit push",
        ] {
            assert!(publishes(command), "{command}");
        }
    }
}
