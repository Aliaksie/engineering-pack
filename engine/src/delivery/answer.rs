//! The structural lookup a prompt asks for in so many words, and the cited answer a prompt
//! hook may deliver with its routing hint. Both abstain unless they are sure: a wrong answer
//! in the prompt costs more than none, and the runnable lookup stays in the hint either way.
use super::prompt::own;
use crate::values::js_space;
use serde_json::{Value, json};

/// Words that ask who calls something.
const CALLERS: &[&str] = &[
    "call", "calls", "called", "calling", "caller", "callers", "invoke", "invokes", "invoked",
    "invoking",
];
/// Words that ask where something is declared.
const DECLARATION: &[&str] = &[
    "declared",
    "declares",
    "declaration",
    "declarations",
    "defined",
    "defines",
    "definition",
    "definitions",
];

/// Words that ask where something is used: a prompt that asks them is not asking for a
/// declaration, even when it mentions one ("which files use it, apart from the one that
/// declares it?").
const USAGE: &[&str] = &[
    "use",
    "uses",
    "used",
    "using",
    "usage",
    "usages",
    "reference",
    "references",
    "referenced",
    "import",
    "imports",
    "imported",
    "importers",
    "depend",
    "depends",
    "dependents",
];

/// Extensions that make a dotted name a file rather than `Type.member`: `Policy.apply` is
/// code, `config.json` a file. A method named like an extension reads as a file and asks
/// nothing.
const EXTENSIONS: &[&str] = &[
    "c",
    "cc",
    "cfg",
    "cjs",
    "conf",
    "cpp",
    "cs",
    "css",
    "csv",
    "env",
    "go",
    "gradle",
    "h",
    "hpp",
    "htm",
    "html",
    "ini",
    "java",
    "js",
    "json",
    "jsx",
    "kt",
    "kts",
    "lock",
    "log",
    "md",
    "mjs",
    "php",
    "properties",
    "py",
    "rb",
    "rs",
    "scala",
    "scss",
    "sh",
    "sql",
    "swift",
    "toml",
    "ts",
    "tsx",
    "txt",
    "xml",
    "yaml",
    "yml",
];

/// Words that ask where a configuration key is set.
const SETTING: &[&str] = &[
    "set",
    "sets",
    "configure",
    "configured",
    "configures",
    "configuration",
    "config",
    "setting",
    "settings",
];
/// Words that ask which documents mention something.
const MENTIONS: &[&str] = &[
    "mention",
    "mentions",
    "mentioned",
    "mentioning",
    "doc",
    "docs",
    "document",
    "documents",
    "documented",
    "documentation",
    "link",
    "links",
    "linked",
];
/// Words that ask about architecture decision records.
const RECORDS: &[&str] = &["adr", "adrs"];
const DECISIONS: &[&str] = &["decision", "decisions"];
const RECORD_WORDS: &[&str] = &["record", "records", "architecture", "architectural"];
/// Words that make "ADR" a question about decisions rather than about code that models them.
const DECIDING: &[&str] = &[
    "decision",
    "decisions",
    "decide",
    "decides",
    "decided",
    "govern",
    "governs",
    "current",
    "supersede",
    "supersedes",
    "superseded",
];

fn file(name: &str) -> bool {
    !name.contains("::")
        && !name.contains('#')
        && name.rsplit_once('.').is_some_and(|(_, extension)| {
            EXTENSIONS.contains(&extension.to_ascii_lowercase().as_str())
        })
}

/// Most rows an answer delivers; a longer answer stays with the runnable lookup.
pub const ROWS: usize = 10;
/// Most bytes of one serialized answer.
pub const ANSWER_BYTES: usize = 1600;
/// A decision record index is one row per record: titles and statuses together answer which
/// record is current, so it may run longer than a symbol answer.
const DECISION_ROWS: usize = 24;
const DECISION_BYTES: usize = 3200;
/// Records a decision lookup reads: fixtures are left out before the answer's bound applies.
const DECISION_LOOKUP: usize = 200;

/// Most rows a lookup for `ask` may return and still answer.
pub fn rows(ask: &str) -> usize {
    if ask == "decisions" {
        DECISION_LOOKUP
    } else {
        ROWS
    }
}

/// Directories that hold tests or their fixtures: a decision record there is test data, not a
/// decision of the project.
const TEST_DIRECTORIES: &[&str] = &[
    "test",
    "tests",
    "__tests__",
    "spec",
    "specs",
    "fixture",
    "fixtures",
    "__fixtures__",
    "testdata",
    "e2e",
];

/// Whether `path` lies under a test or fixture directory: one named in `TEST_DIRECTORIES`,
/// or ending in `-tests`, `_tests`, `-fixtures` or `_fixtures` (`integration-tests`).
fn under_tests(path: &str) -> bool {
    let mut directories = path.split('/').rev().skip(1);
    directories.any(|directory| {
        let directory = directory.to_ascii_lowercase();
        TEST_DIRECTORIES.contains(&directory.as_str())
            || ["-tests", "_tests", "-fixtures", "_fixtures"]
                .iter()
                .any(|suffix| directory.ends_with(suffix))
    })
}
const EXCERPT_CHARS: usize = 120;

const USE: &str = "Verified Project Intelligence rows from the current files with complete coverage of this scope: they answer this question without another search";
/// What a row matched by name claims, added to `USE` when an answer has one.
const BY_NAME: &str = "name rows spell the asked name in syntax, not a resolved target";

fn identifier(part: &str) -> bool {
    part.chars()
        .next()
        .is_some_and(|c| c.is_alphabetic() || c == '_' || c == '$')
        && part
            .chars()
            .all(|c| c.is_alphanumeric() || c == '_' || c == '$')
}

/// A token's code name: identifiers joined by `::`, `.` or `#`, optionally called as `()`.
/// A bare word counts only when it reads as code on its own (quoted, `snake_case` or
/// `camelCase`); "Checkout" in a sentence is a word, not a symbol.
fn code_name(token: &str) -> Option<String> {
    let token = token.trim_start_matches(['(', '[', '{', '"', '\'', '<']);
    let quoted = token.starts_with('`');
    let mut core = token.trim_start_matches('`');
    // Closing punctuation and brackets, but a call's own `()` stays.
    let core = loop {
        let trimmed =
            core.trim_end_matches(['`', '"', '\'', ']', '}', '>', '.', ',', ';', ':', '!', '?']);
        match trimmed.strip_suffix(')') {
            Some(rest) if !trimmed.ends_with("()") => core = rest,
            _ => break trimmed,
        }
    };
    let called = core.ends_with("()");
    let name = core.strip_suffix("()").unwrap_or(core);
    let qualified = name.contains("::") || name.contains(['.', '#']);
    let mut parts = name.split("::").flat_map(|part| part.split(['.', '#']));
    if name.is_empty() || file(name) || !parts.all(identifier) {
        return None;
    }
    let chars: Vec<char> = name.chars().collect();
    let camel = chars
        .windows(2)
        .any(|pair| pair[0].is_lowercase() && pair[1].is_uppercase());
    (quoted || called || qualified || camel || name.contains('_')).then(|| name.to_string())
}

/// A dotted configuration key spelled as code: `compilerOptions.strict`, `jobs.build.runs-on`.
/// Segments are letters, digits, `_` and `-`; a file name is not a key.
fn key_name(token: &str) -> Option<String> {
    let token = token.trim_start_matches(['(', '[', '{', '"', '\'', '<', '`']);
    let core = token.trim_end_matches([
        '`', '"', '\'', ']', '}', ')', '>', '.', ',', ';', ':', '!', '?',
    ]);
    let segment = |part: &str| {
        part.chars().next().is_some_and(char::is_alphabetic)
            && part
                .chars()
                .all(|c| c.is_alphanumeric() || c == '_' || c == '-')
    };
    (core.contains('.') && !file(core) && core.split('.').all(segment)).then(|| core.to_string())
}

/// A root-relative file path spelled as one token: `src/hono.ts`, `docs/adr/0001-x.md`.
fn path_name(token: &str) -> Option<String> {
    let token = token.trim_start_matches(['(', '[', '{', '"', '\'', '<', '`']);
    let core = token.trim_end_matches([
        '`', '"', '\'', ']', '}', ')', '>', '.', ',', ';', ':', '!', '?',
    ]);
    let core = core.strip_prefix("./").unwrap_or(core);
    (file(core)
        && core
            .chars()
            .all(|c| c.is_alphanumeric() || matches!(c, '_' | '-' | '.' | '/'))
        && core.split('/').all(|part| !part.is_empty() && part != ".."))
    .then(|| core.to_string())
}

/// The one distinct name `name` reads in `text`, or `None` for none or several.
fn one(text: &str, name: impl Fn(&str) -> Option<String>) -> Option<String> {
    let mut names = Vec::new();
    for found in text.split(js_space).filter_map(name) {
        if !names.contains(&found) {
            names.push(found);
        }
    }
    <[String; 1]>::try_from(names).ok().map(|[name]| name)
}

/// The structural lookup a prompt asks for and the one name it names: `callers` or
/// `declaration` of a code name, `decisions` for a question about decision records (answered
/// by the index of every record, `*`), `key` for where a dotted configuration key is set, or
/// `mentions` for the documents naming a code name or file. Competing questions or more than
/// one name ask nothing.
pub fn question(prompt: &str) -> Option<(&'static str, String)> {
    let text = own(prompt);
    let words: Vec<String> = text
        .split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|word| !word.is_empty())
        .map(str::to_ascii_lowercase)
        .collect();
    let asks = |set: &[&str]| words.iter().any(|word| set.contains(&word.as_str()));
    let symbol = match (asks(CALLERS), asks(DECLARATION)) {
        (true, false) => Some("callers"),
        (false, true) if !asks(USAGE) => Some("declaration"),
        (true, true) => return None,
        _ => None,
    };
    if let Some(ask) = symbol {
        return one(text, code_name).map(|name| (ask, name));
    }
    if words.iter().any(|w| w == "adrs")
        || asks(RECORDS) && asks(DECIDING)
        || asks(DECISIONS) && asks(RECORD_WORDS)
    {
        return Some(("decisions", "*".into()));
    }
    if asks(SETTING)
        && let Some(key) = one(text, key_name)
    {
        return Some(("key", key));
    }
    if asks(MENTIONS) && !asks(USAGE) {
        let names = one(text, code_name).into_iter().chain(one(text, path_name));
        return match names.collect::<Vec<_>>().as_slice() {
            [name] => Some(("mentions", name.clone())),
            _ => None,
        };
    }
    None
}

/// The rows of a lookup result that answers for its whole scope: from verified evidence,
/// complete coverage, no literal gap match left to interpret and no further page.
fn complete_rows(result: &Value) -> Option<&[Value]> {
    complete_within(result, ROWS)
}

fn complete_within(result: &Value, most: usize) -> Option<&[Value]> {
    let route = &result["route"];
    let indexed = matches!(
        route["answeredBy"].as_str(),
        Some("intelligence" | "intelligence+locate")
    );
    let gaps = result["gapSearch"]["matches"]
        .as_array()
        .is_some_and(|matches| !matches.is_empty());
    let rows = result["results"].as_array()?;
    (indexed
        && route["complete"] == true
        && !gaps
        && result["continuation"].is_null()
        && (1..=most).contains(&rows.len()))
    .then_some(rows.as_slice())
}

/// Whether a lookup `result` can answer at all, before any further lookup is spent on it:
/// a callers result that is incomplete, empty or too large answers nothing whatever its
/// declaration says.
pub fn answerable(result: &Value) -> bool {
    complete_rows(result).is_some()
}

/// Whether the source at `path` is named by the qualifier's last segment: `Type` in
/// `Type.method` names `Type.java`, `module` in `module::fn` names `module.rs` or
/// `module/mod.rs`.
fn named_by(path: &str, qualifier: &str) -> bool {
    let last = ["::", ".", "#"]
        .iter()
        .filter_map(|separator| qualifier.rfind(separator).map(|at| at + separator.len()))
        .max()
        .map_or(qualifier, |start| &qualifier[start..]);
    let mut parts = path.rsplit('/');
    let file = parts.next().unwrap_or_default();
    let stem = file.split_once('.').map_or(file, |(stem, _)| stem);
    let stem = match stem {
        "mod" | "index" | "__init__" => parts.next().unwrap_or_default(),
        stem => stem,
    };
    stem == last
}

fn line(row: &Value) -> Option<String> {
    let path = row["path"].as_str()?;
    let number = row["line"].as_u64()?;
    let within = row["container"]["name"]
        .as_str()
        .map_or(String::new(), |name| format!(" in {name}"));
    let excerpt = row["excerpt"].as_str().unwrap_or_default().trim();
    let excerpt = match excerpt.char_indices().nth(EXCERPT_CHARS) {
        Some((end, _)) => format!("{}…", &excerpt[..end]),
        None => excerpt.to_string(),
    };
    // A decision record reads as its title and status, a key as its path and value.
    match (row["kind"].as_str(), row["name"].as_str()) {
        (Some("md:decision"), Some(title)) => {
            let status = row["target"].as_str().unwrap_or("no status");
            Some(format!("{path}:{number}: {title} [{status}]"))
        }
        (Some("config:key"), Some(key)) => {
            Some(format!("{path}:{number}{within}: {key}: {excerpt}"))
        }
        _ => Some(format!("{path}:{number}{within}: {excerpt}")),
    }
}

/// The cited answer a hook may deliver for `ask` about `value`, from the lookup `result` and,
/// for callers, the `declarations` lookup of the same name. Callers answer only for a name
/// declared exactly once, so every row calls that declaration; a qualified name must also
/// name the declaring source. Anything less certain, larger or incomplete answers nothing.
pub fn answered(
    ask: &str,
    value: &str,
    result: &Value,
    declarations: Option<&Value>,
) -> Option<Value> {
    let found = complete_within(result, rows(ask))?;
    let qualifier = result["route"]["qualifier"].as_str();
    let owned = |row: &Value| {
        qualifier.is_none_or(|qualifier| {
            row["path"]
                .as_str()
                .is_some_and(|path| named_by(path, qualifier))
        })
    };
    let rows: Vec<&Value> = match ask {
        "callers" => match complete_rows(declarations?)? {
            [declaration] if owned(declaration) => found.iter().collect(),
            _ => return None,
        },
        "declaration" => found.iter().filter(|row| owned(row)).collect(),
        "mentions" | "key" => found.iter().collect(),
        "decisions" => found
            .iter()
            .filter(|row| !row["path"].as_str().is_some_and(under_tests))
            .collect(),
        _ => return None,
    };
    if rows.is_empty() || ask == "decisions" && rows.len() > DECISION_ROWS {
        return None;
    }
    let left_out = found.len() - rows.len();
    // A row of unknown kind claims no more than its name.
    let basis = |row: &Value| match (row["basis"].as_str(), row["kind"].as_str()) {
        (Some("parsed"), _) => "parsed",
        (Some(_), _) | (None, None) => "name",
        (None, Some(kind)) => crate::basis(kind),
    };
    let mixed = rows.iter().any(|row| basis(row) != basis(rows[0]));
    let lines = rows
        .iter()
        .map(|row| {
            let line = line(row)?;
            Some(if mixed && basis(row) == "name" {
                format!("{line} [by name]")
            } else {
                line
            })
        })
        .collect::<Option<Vec<_>>>()?;
    let by_name = rows.iter().any(|row| basis(row) == "name");
    let mut answer = json!({
        "question": format!("{ask} {value}"),
        "scopes": result["route"]["scopes"],
        "complete": true,
        "rows": lines,
        "basis": if mixed { "mixed" } else { basis(rows[0]) },
        "use": if by_name { format!("{USE}; {BY_NAME}") } else { USE.to_string() },
    });
    if ask == "decisions" && left_out > 0 {
        answer["leftOut"] = json!(format!(
            "{left_out} records under test or fixture directories"
        ));
    }
    let bound = if ask == "decisions" {
        DECISION_BYTES
    } else {
        ANSWER_BYTES
    };
    (answer.to_string().len() <= bound).then_some(answer)
}

/// The one code name a search the model ran looks for: a Grep tool `pattern`, or the first
/// `rg`, `grep` or `git grep` of a shell `command`. Only a literal identifier (optionally
/// qualified with `.` or `::`, word-bounded or followed by `(`) names something; a regex,
/// several patterns, an inverted match or a pattern file name nothing.
pub fn searched(pattern: Option<&str>, command: Option<&str>) -> Option<String> {
    match pattern.filter(|p| !p.is_empty()) {
        Some(pattern) => literal_name(pattern),
        None => shell_search(command?),
    }
}

fn literal_name(pattern: &str) -> Option<String> {
    let core = pattern.strip_prefix("\\b").unwrap_or(pattern);
    let core = core.strip_suffix("\\b").unwrap_or(core);
    let core = ["\\(", "("]
        .iter()
        .find_map(|call| core.strip_suffix(call))
        .unwrap_or(core);
    let core = core.strip_suffix("\\b").unwrap_or(core);
    let mut parts = core.split("::").flat_map(|part| part.split('.'));
    (core.chars().count() >= 3 && !file(core) && parts.all(identifier)).then(|| core.to_string())
}

/// Shell words with plain quoting; `None` for an unterminated quote.
fn words(command: &str) -> Option<Vec<String>> {
    let mut words = Vec::new();
    let mut word: Option<String> = None;
    let mut quote: Option<char> = None;
    let mut chars = command.chars();
    while let Some(c) = chars.next() {
        match (quote, c) {
            (Some(open), c) if c == open => quote = None,
            (Some('"'), '\\') => word.get_or_insert_default().extend(chars.next()),
            (Some(_), c) => word.get_or_insert_default().push(c),
            (None, '\'' | '"') => {
                quote = Some(c);
                word.get_or_insert_default();
            }
            (None, '\\') => word.get_or_insert_default().extend(chars.next()),
            (None, c) if c.is_whitespace() => words.extend(word.take()),
            (None, '|' | ';' | '&') => {
                words.extend(word.take());
                words.push(c.to_string());
            }
            (None, c) => word.get_or_insert_default().push(c),
        }
    }
    words.extend(word);
    quote.is_none().then_some(words)
}

/// Options whose value is the next word, for `rg` and for `grep`/`git grep`.
const RG_VALUED: &[&str] = &[
    "-A",
    "-B",
    "-C",
    "-g",
    "-t",
    "-T",
    "-m",
    "-M",
    "-j",
    "-r",
    "--glob",
    "--iglob",
    "--type",
    "--type-not",
    "--max-count",
    "--context",
    "--after-context",
    "--before-context",
    "--replace",
    "--threads",
    "--sort",
    "--color",
    "--colors",
    "--max-columns",
];
const GREP_VALUED: &[&str] = &[
    "-A",
    "-B",
    "-C",
    "-m",
    "-d",
    "-D",
    "--max-count",
    "--context",
    "--after-context",
    "--before-context",
    "--max-depth",
    "--color",
    "--colour",
];

/// Shell segments, each with whether a single `|` feeds it another command's output.
fn segments(words: &[String]) -> Vec<(bool, &[String])> {
    let separator = |word: &String| matches!(word.as_str(), "|" | ";" | "&");
    let mut segments = Vec::new();
    let mut start = 0;
    for end in (0..=words.len()).filter(|&i| i == words.len() || separator(&words[i])) {
        let piped = start >= 1 && words[start - 1] == "|" && (start < 2 || words[start - 2] != "|");
        if end > start {
            segments.push((piped, &words[start..end]));
        }
        start = end + 1;
    }
    segments
}

fn shell_search(command: &str) -> Option<String> {
    let words = words(command)?;
    let mut searches = segments(&words)
        .into_iter()
        .filter_map(|(piped, segment)| match segment {
            [program, rest @ ..] if program == "rg" => Some((piped, RG_VALUED, rest)),
            [program, rest @ ..] if program == "grep" => Some((piped, GREP_VALUED, rest)),
            [git, grep, rest @ ..] if git == "git" && grep == "grep" => {
                Some((false, GREP_VALUED, rest))
            }
            _ => None,
        });
    let (piped, valued, arguments) = searches.next()?;
    if searches.next().is_some() {
        return None;
    }
    let mut patterns = Vec::new();
    let mut positional = Vec::new();
    let mut rest = arguments.iter();
    let mut options = true;
    while let Some(word) = rest.next() {
        match word.as_str() {
            "--" if options => options = false,
            "-e" | "--regexp" if options => patterns.push(rest.next()?),
            "-v" | "--invert-match" | "-f" | "--file" if options => return None,
            flag if options && valued.contains(&flag) => {
                rest.next()?;
            }
            flag if options && flag.starts_with('-') && flag.len() > 1 => {
                // Combined short flags: an inverted match or a pattern file names nothing.
                if !flag.starts_with("--") && flag[1..].contains(['v', 'f', 'e']) {
                    return None;
                }
            }
            _ => positional.push(word),
        }
    }
    let (pattern, operands) = match (patterns.as_slice(), positional.split_first()) {
        ([pattern], _) => (*pattern, positional.len()),
        ([], Some((pattern, files))) => (*pattern, files.len()),
        _ => return None,
    };
    // A piped search without a file operand reads the other command's output, not the
    // repository.
    if piped && operands == 0 {
        return None;
    }
    literal_name(pattern)
}

/// The answer a search for `value` gets: its one declaration and its callers, from complete
/// `callers` and `declarations` lookups. Anything `answered` would not certify answers nothing.
pub fn usages(value: &str, callers: &Value, declarations: &Value) -> Option<Value> {
    let mut answer = answered("callers", value, callers, Some(declarations))?;
    let declaration = line(complete_rows(declarations)?.first()?)?;
    answer["declaration"] = json!(declaration);
    (answer.to_string().len() <= ANSWER_BYTES).then_some(answer)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_complete_bounded_result_is_worth_a_further_lookup() {
        let row = json!({"path": "src/a.rs", "line": 3, "excerpt": "a()"});
        let result = |complete: bool, rows: Vec<Value>| json!({"route": {"answeredBy": "intelligence", "complete": complete}, "results": rows, "continuation": null});
        assert!(answerable(&result(true, vec![row.clone()])));
        assert!(!answerable(&result(false, vec![row.clone()])));
        assert!(!answerable(&result(true, vec![])));
        assert!(!answerable(&result(true, vec![row; ROWS + 1])));
    }

    #[test]
    fn a_lookup_prompt_names_one_question_and_one_code_name() {
        let eval = "A teammate wants to change the signature of `DiscountPolicy.applyDiscount` in this\nrepository and asked which methods call it directly.\n\nWrite the answer to `ANSWER.md` at the repository root, one caller per line, as\n`<path from the repository root>#<method name>`, for example\n`src/main/java/com/acme/billing/Example.java#run`. Do not change any other file.";
        assert_eq!(
            question(eval),
            Some(("callers", "DiscountPolicy.applyDiscount".into()))
        );
        for (prompt, expected) in [
            (
                "Who calls prompt::request?",
                Some(("callers", "prompt::request")),
            ),
            (
                "who calls `apply_discount()`",
                Some(("callers", "apply_discount")),
            ),
            (
                "Where is applyDiscount defined?",
                Some(("declaration", "applyDiscount")),
            ),
            (
                "Which files use `AdrSlug`, apart from the file that declares it?",
                None,
            ),
            ("Where is `Checkout` defined and where is it used?", None),
            (
                "Where is `Checkout` declared?",
                Some(("declaration", "Checkout")),
            ),
            (
                "Which methods call `Policy.apply`?",
                Some(("callers", "Policy.apply")),
            ),
            ("Who calls Order#total?", Some(("callers", "Order#total"))),
            ("Who calls the code in `orders.py`?", None),
            // A word in a sentence is not a symbol; neither is a file or a version.
            ("Who calls Checkout?", None),
            ("Who calls the code in src/order.js?", None),
            ("Where is v2.3 defined?", None),
            // Both questions, neither, or two names ask nothing.
            ("Where is `run` defined and who calls it?", None),
            ("Where is `run` used?", None),
            ("Who calls `run` or `start`?", None),
        ] {
            assert_eq!(
                question(prompt),
                expected.map(|(ask, name)| (ask, name.to_string())),
                "{prompt}"
            );
        }
    }

    #[test]
    fn decision_key_and_mention_prompts_name_their_question() {
        for (prompt, expected) in [
            (
                "Which architecture decision record is the current decision on how an ADR is uniquely identified?",
                Some(("decisions", "*")),
            ),
            (
                "Which ADRs govern the React code?",
                Some(("decisions", "*")),
            ),
            ("What did we decide about caching?", None),
            (
                "Which ADR decides the cache policy?",
                Some(("decisions", "*")),
            ),
            (
                "Which file models an ADR's status in the core package?",
                None,
            ),
            (
                "Where is `compilerOptions.strict` set?",
                Some(("key", "compilerOptions.strict")),
            ),
            (
                "Which file configures jobs.build.runs-on?",
                Some(("key", "jobs.build.runs-on")),
            ),
            ("Where is config.json set?", None),
            (
                "Which docs mention `AdrSlug`?",
                Some(("mentions", "AdrSlug")),
            ),
            (
                "Which documents link to src/hono.ts?",
                Some(("mentions", "src/hono.ts")),
            ),
            // A document question without a name, or with two, asks nothing.
            (
                "Which documentation page explains how to configure connection pool limits?",
                None,
            ),
            ("Which docs mention `a_b` and `c_d`?", None),
            // A symbol question keeps its meaning beside document words.
            (
                "Where is `parseArgs` defined in the docs?",
                Some(("declaration", "parseArgs")),
            ),
        ] {
            assert_eq!(
                question(prompt),
                expected.map(|(ask, name)| (ask, name.to_string())),
                "{prompt}"
            );
        }
    }

    #[test]
    fn decisions_and_keys_answer_with_status_and_value() {
        let decisions: Vec<Value> = (0..12)
            .map(|n| json!({"path": format!("docs/adr/{n}.md"), "line": 1, "kind": "md:decision", "name": format!("Use {n}"), "target": "accepted", "excerpt": "# Use"}))
            .collect();
        let answer = answered("decisions", "*", &result(decisions.clone(), None), None).unwrap();
        assert_eq!(answer["rows"][0], "docs/adr/0.md:1: Use 0 [accepted]");
        assert_eq!(answer["rows"].as_array().unwrap().len(), 12);
        let many: Vec<Value> = decisions
            .iter()
            .cycle()
            .take(DECISION_ROWS + 1)
            .cloned()
            .collect();
        assert!(answered("decisions", "*", &result(many, None), None).is_none());
        // Records under test or fixture directories are test data: left out, and counted.
        let fixtures: Vec<Value> = (0..DECISION_ROWS)
            .map(|n| {
                let directory = ["packages/core/integration-tests/p/docs/adr", "tests/fixtures/adr", "src/__tests__/adr"][n % 3];
                json!({"path": format!("{directory}/{n}.md"), "line": 1, "kind": "md:decision", "name": format!("Fixture {n}"), "target": "accepted"})
            })
            .chain(decisions.iter().cloned())
            .collect();
        let answer = answered("decisions", "*", &result(fixtures, None), None).unwrap();
        assert_eq!(answer["rows"].as_array().unwrap().len(), 12);
        assert_eq!(
            answer["leftOut"],
            "24 records under test or fixture directories"
        );
        assert!(
            answered("decisions", "*", &result(decisions.clone(), None), None)
                .unwrap()
                .get("leftOut")
                .is_none()
        );
        assert!(!under_tests("docs/adr/testing-strategy.md"));
        assert!(!under_tests("tests.md"));
        assert!(under_tests("Tests/adr/x.md"));
        let key = json!({"path": "tsconfig.json", "line": 8, "kind": "config:key", "name": "compilerOptions.strict", "target": "true", "excerpt": "\"strict\": true,", "container": {"name": "compilerOptions"}});
        assert_eq!(
            answered("key", "strict", &result(vec![key], None), None).unwrap()["rows"][0],
            "tsconfig.json:8 in compilerOptions: compilerOptions.strict: \"strict\": true,"
        );
        // Symbol answers keep their row bound.
        let rows = vec![row("docs/a.md", 1, Some("Use")); ROWS + 1];
        assert!(answered("mentions", "x", &result(rows, None), None).is_none());
    }

    #[test]
    fn pasted_material_does_not_name_the_lookup() {
        let prompt = "Who calls `apply`?\n\nlog: `retry_once` failed, see `Worker.run`";
        assert_eq!(question(prompt), Some(("callers", "apply".into())));
    }

    fn row(path: &str, line: u64, container: Option<&str>) -> Value {
        let mut row = json!({"path": path, "line": line, "excerpt": "  x = policy.apply(1);  "});
        if let Some(name) = container {
            row["container"] = json!({"name": name});
        }
        row
    }

    fn result(rows: Vec<Value>, qualifier: Option<&str>) -> Value {
        let mut route = json!({"answeredBy": "intelligence", "complete": true, "scopes": ["."]});
        if let Some(qualifier) = qualifier {
            route["qualifier"] = json!(qualifier);
        }
        json!({"results": rows, "continuation": null, "route": route})
    }

    #[test]
    fn callers_answer_only_for_one_declaration_named_by_the_qualifier() {
        let callers = result(
            vec![row("src/Checkout.java", 14, Some("finish"))],
            Some("Policy"),
        );
        let one = result(vec![row("src/Policy.java", 5, None)], Some("Policy"));
        let answer = answered("callers", "Policy.apply", &callers, Some(&one)).unwrap();
        assert_eq!(
            answer["rows"],
            json!(["src/Checkout.java:14 in finish: x = policy.apply(1);"])
        );
        assert_eq!(answer["question"], "callers Policy.apply");
        // Two declarations: the rows may call either one.
        let two = result(
            vec![
                row("src/Policy.java", 5, None),
                row("src/Other.java", 3, None),
            ],
            Some("Policy"),
        );
        assert_eq!(
            answered("callers", "Policy.apply", &callers, Some(&two)),
            None
        );
        // The one declaration lives elsewhere: the qualifier may name an outside type.
        let elsewhere = result(vec![row("src/Other.java", 3, None)], Some("Policy"));
        assert_eq!(
            answered("callers", "Policy.apply", &callers, Some(&elsewhere)),
            None
        );
        assert_eq!(answered("callers", "Policy.apply", &callers, None), None);
        // A Rust module path names its file or its folder's mod.rs.
        let unqualified = result(
            vec![row("a/b.rs", 1, None)],
            Some("engine::delivery::prompt"),
        );
        let module = result(vec![row("src/prompt/mod.rs", 9, None)], None);
        assert!(answered("callers", "prompt::request", &unqualified, Some(&module)).is_some());
    }

    #[test]
    fn answers_state_what_their_rows_rest_on() {
        let kind = |mut row: Value, kind: &str| {
            row["kind"] = json!(kind);
            row
        };
        let declaration = result(
            vec![kind(
                row("src/Policy.java", 5, None),
                "java:declares-method",
            )],
            None,
        );
        let callers = result(
            vec![kind(
                row("src/A.java", 1, Some("run")),
                "java:invokes-syntax",
            )],
            None,
        );
        let parsed = answered("declaration", "apply", &declaration, None).unwrap();
        assert_eq!(
            (&parsed["basis"], &parsed["use"]),
            (&json!("parsed"), &json!(USE))
        );
        let named = usages("apply", &callers, &declaration).unwrap();
        assert_eq!(named["basis"], "name");
        assert!(named["use"].as_str().unwrap().ends_with(BY_NAME));
        assert_eq!(
            named["rows"],
            json!(["src/A.java:1 in run: x = policy.apply(1);"])
        );
        // Only a mixed answer marks its name rows; a row of unknown kind is one of them.
        let mentions = result(
            vec![
                kind(row("docs/a.md", 2, None), "md:link"),
                kind(row("docs/a.md", 4, None), "md:mention"),
                row("docs/b.md", 1, None),
            ],
            None,
        );
        let mixed = answered("mentions", "apply", &mentions, None).unwrap();
        assert_eq!(mixed["basis"], "mixed");
        assert_eq!(
            mixed["rows"],
            json!([
                "docs/a.md:2: x = policy.apply(1);",
                "docs/a.md:4: x = policy.apply(1); [by name]",
                "docs/b.md:1: x = policy.apply(1); [by name]"
            ])
        );
    }

    #[test]
    fn incomplete_or_oversized_answers_abstain() {
        let one = result(vec![row("src/Policy.java", 5, None)], None);
        let callers = result(vec![row("src/A.java", 1, None)], None);
        assert!(answered("callers", "apply", &callers, Some(&one)).is_some());
        let mut partial = callers.clone();
        partial["route"]["complete"] = json!(false);
        let mut literal = callers.clone();
        literal["route"]["answeredBy"] = json!("locate");
        let mut gaps = callers.clone();
        gaps["route"]["answeredBy"] = json!("intelligence+locate");
        gaps["gapSearch"] = json!({"matches": [{"path": "web/a.js", "line": 2}]});
        let mut paged = callers.clone();
        paged["continuation"] = json!({"after": "0".repeat(64)});
        let empty = result(vec![], None);
        let many = result(
            (0..=ROWS as u64)
                .map(|n| row("src/A.java", n + 1, None))
                .collect(),
            None,
        );
        for (name, case) in [
            ("partial", &partial),
            ("literal", &literal),
            ("gaps", &gaps),
            ("paged", &paged),
            ("empty", &empty),
            ("many", &many),
        ] {
            assert_eq!(
                answered("callers", "apply", case, Some(&one)),
                None,
                "{name}"
            );
        }
        let mut clean_gaps = gaps.clone();
        clean_gaps["gapSearch"] = json!({"matches": []});
        assert!(answered("callers", "apply", &clean_gaps, Some(&one)).is_some());
        let mut long = row("src/A.java", 1, Some("run"));
        long["excerpt"] = json!("y".repeat(400));
        let rows = (0..ROWS).map(|_| long.clone()).collect();
        assert_eq!(
            answered("callers", "apply", &result(rows, None), Some(&one)),
            None
        );
    }

    #[test]
    fn declarations_keep_the_rows_the_qualifier_names() {
        let both = result(
            vec![
                row("src/Policy.java", 5, None),
                row("src/Other.java", 3, None),
            ],
            Some("Policy"),
        );
        let answer = answered("declaration", "Policy.apply", &both, None).unwrap();
        assert_eq!(answer["rows"].as_array().unwrap().len(), 1);
        let none = result(vec![row("src/Other.java", 3, None)], Some("Policy"));
        assert_eq!(answered("declaration", "Policy.apply", &none, None), None);
        assert!(
            answered(
                "declaration",
                "apply",
                &result(vec![row("a.rs", 1, None)], None),
                None
            )
            .is_some()
        );
    }

    #[test]
    fn a_search_names_one_literal_identifier() {
        let grep = |pattern| searched(Some(pattern), None);
        assert_eq!(grep("applyDiscount"), Some("applyDiscount".into()));
        assert_eq!(grep("\\bapply_discount\\b"), Some("apply_discount".into()));
        assert_eq!(grep("applyDiscount\\("), Some("applyDiscount".into()));
        assert_eq!(grep("Policy.apply"), Some("Policy.apply".into()));
        assert_eq!(grep("crate::route"), Some("crate::route".into()));
        for regex in [
            "apply.*Discount",
            "foo|bar",
            "ab",
            "config.json",
            "x y",
            "^main",
        ] {
            assert_eq!(grep(regex), None, "{regex}");
        }
        let shell = |command| searched(None, Some(command));
        assert_eq!(
            shell("rg -n applyDiscount src"),
            Some("applyDiscount".into())
        );
        assert_eq!(
            shell("cd app && rg -n 'applyDiscount' -g '*.java' | head -20"),
            Some("applyDiscount".into())
        );
        assert_eq!(
            shell("grep -rn --include=*.py apply_rate ."),
            Some("apply_rate".into())
        );
        assert_eq!(
            shell("git grep -n -e TaxRate -- src"),
            Some("TaxRate".into())
        );
        assert_eq!(
            shell("rg -A 3 -t java applyDiscount"),
            Some("applyDiscount".into())
        );
        assert_eq!(
            shell("grep -r applyDiscount src"),
            Some("applyDiscount".into()),
            "grep -r takes no value"
        );
        for none in [
            "rg -v applyDiscount",
            "grep -rv applyDiscount .",
            "rg -e one -e two",
            "rg -f patterns.txt",
            "rg 'apply.*'",
            "rg applyDiscount && grep total src",
            "cat a.txt",
            "rg 'unterminated",
            "ls | wc -l",
            "copilot --help | grep -iA3 \"model\"",
            "cargo tree | rg serde_json",
            "git log --oneline | grep -e applyDiscount",
        ] {
            assert_eq!(shell(none), None, "{none}");
        }
        assert_eq!(
            shell("git ls-files | rg applyDiscount src"),
            Some("applyDiscount".into()),
            "a piped search with a file operand still reads the repository"
        );
        assert_eq!(
            shell("false || rg applyDiscount"),
            Some("applyDiscount".into()),
            "`||` does not feed output"
        );
        assert_eq!(
            searched(Some(""), Some("rg -n applyDiscount")),
            Some("applyDiscount".into())
        );
        assert_eq!(searched(None, None), None);
    }

    #[test]
    fn a_search_answer_names_the_declaration_and_its_callers() {
        let callers = result(
            vec![row("src/Checkout.java", 14, Some("finish"))],
            Some("Policy"),
        );
        let one = result(vec![row("src/Policy.java", 5, None)], Some("Policy"));
        let answer = usages("Policy.apply", &callers, &one).unwrap();
        assert!(
            answer["declaration"]
                .as_str()
                .unwrap()
                .starts_with("src/Policy.java:5")
        );
        assert_eq!(answer["rows"].as_array().unwrap().len(), 1);
        assert_eq!(answer["complete"], true);
        let two = result(
            vec![
                row("src/Policy.java", 5, None),
                row("src/Rule.java", 2, None),
            ],
            None,
        );
        assert_eq!(usages("apply", &callers, &two), None);
        let open = json!({"results": [row("a.rs", 1, None)], "continuation": null, "route": {"answeredBy": "locate", "complete": true}});
        assert_eq!(usages("apply", &open, &one), None);
    }
}
