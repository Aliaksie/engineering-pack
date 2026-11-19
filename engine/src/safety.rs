//! The project's safety policy: rules that refuse destructive shell commands, commands that
//! reach outside the project, and reads of files that commonly hold secrets, through any
//! tool the hook sees. A rule only ever refuses or hands the call to the user; nothing here
//! approves one, so the host's own prompts, sandbox and deny rules keep applying.
//!
//! It is a guardrail, not a security boundary. The recognizer reads literal commands,
//! scripts passed to supported shells with `-c` and `$()`/backtick substitutions; aliases,
//! other interpreters, variable expansion and scripts on disk are outside it. Paths are
//! judged by name: the caller passes each spelling of the project root it knows.
use crate::contracts::kebab;
use crate::review::walk;
use crate::text::Template;
use crate::values::object;
use serde_json::{Value, json};

type Result<T = ()> = std::result::Result<T, String>;

pub mod secrets;

/// The rule that keeps secret files out of the model's context.
pub const SECRET_RULE: &str = "read-secret-file";

/// A built-in rule: its id and what it refuses, in the words shown to the user and model.
pub const RULES: [(&str, &str); 7] = [
    (
        "delete-outside-project",
        "a recursive delete whose target is outside the project and the temporary directories, in the home directory, an unexpanded variable or read from input",
    ),
    (
        "delete-project-root",
        "a recursive delete of the project root, everything in it or its Git directory",
    ),
    (
        "force-push",
        "a forced push, which rewrites the remote branch",
    ),
    (
        "delete-remote-branch",
        "a push that deletes a remote branch or tag",
    ),
    (
        "discard-changes",
        "a Git command that discards uncommitted work or stashes",
    ),
    (
        "system-administration",
        "a privileged or system-level command (sudo, shutdown, service, mount or disk changes)",
    ),
    (
        SECRET_RULE,
        "reading a file that commonly holds secrets (.env files, private keys, credential and token files) through any tool",
    ),
];
/// The rule reported for a command nested too deep to read.
const UNREADABLE: (&str, &str) = (
    "unreadable-command",
    "a command nested too deep for the policy to read",
);
const MAX_DENY: usize = 32;
const MAX_WORDS: usize = 8;
const MAX_REASON: usize = 200;

/// Words that run the command that follows them.
const WRAPPERS: [&str; 5] = ["env", "command", "nohup", "time", "exec"];
const GIT_VALUE_OPTIONS: [&str; 7] = [
    "-C",
    "-c",
    "--git-dir",
    "--work-tree",
    "--namespace",
    "--config-env",
    "--super-prefix",
];
const SYSTEM: [&str; 12] = [
    "sudo", "doas", "shutdown", "reboot", "halt", "poweroff", "umount", "fdisk", "parted",
    "mkswap", "swapoff", "chroot",
];
const SERVICE_ACTIONS: [&str; 4] = ["stop", "disable", "mask", "kill"];
/// `xargs` options whose value is the next word.
const XARGS_VALUE_OPTIONS: [&str; 15] = [
    "-a",
    "-d",
    "-E",
    "-I",
    "-L",
    "-n",
    "-P",
    "-s",
    "--arg-file",
    "--delimiter",
    "--max-args",
    "--max-chars",
    "--max-lines",
    "--max-procs",
    "--process-slot-var",
];
/// `find` tests that narrow what an expression matches by name, place, time, size or owner.
const FIND_TESTS: [&str; 30] = [
    "-name",
    "-iname",
    "-path",
    "-ipath",
    "-wholename",
    "-iwholename",
    "-regex",
    "-iregex",
    "-lname",
    "-ilname",
    "-newer",
    "-mtime",
    "-mmin",
    "-atime",
    "-amin",
    "-ctime",
    "-cmin",
    "-size",
    "-empty",
    "-user",
    "-group",
    "-nouser",
    "-nogroup",
    "-perm",
    "-links",
    "-inum",
    "-samefile",
    "-uid",
    "-gid",
    "-used",
];

/// Where a command runs, as absolute `/`-separated paths: the project root under each
/// spelling it may be written in (as given, as resolved on disk), the working directory, and
/// the temporary directories strictly below which a recursive delete is allowed.
pub struct Place<'a> {
    pub roots: &'a [String],
    pub cwd: &'a str,
    pub temp: &'a [String],
}

/// Whether the policy is on in resolved settings, where it is on unless a layer or the
/// project sets `safety.enabled` to false.
pub fn enabled(settings: &Value) -> bool {
    settings["safety"]["enabled"] == true
}

/// Whether the secret-file rule is in force: the policy is on and does not leave it out.
pub fn guards_secret_files(settings: &Value) -> bool {
    enabled(settings) && !disabled(settings, SECRET_RULE)
}

/// Validate `safety`: `enabled`, `disables` naming built-in rules, `deny` rules of the
/// project's own, each an id, the leading words of a command and an optional reason, and
/// `secretFiles`, each an id and a file pattern.
pub fn validate(safety: &Value) -> Result {
    let map = object(
        safety,
        &["enabled", "disables", "deny", "secretFiles"],
        "safety",
    )?;
    if let Some(patterns) = map.get("secretFiles") {
        validate_secret_files(patterns)?;
    }
    if !map.get("enabled").is_none_or(Value::is_boolean) {
        return Err("safety.enabled must be boolean".into());
    }
    let mut ids: Vec<&str> = RULES.iter().map(|(id, _)| *id).collect();
    if let Some(disables) = map.get("disables") {
        let names = disables
            .as_array()
            .ok_or("safety.disables must be an array")?;
        for name in names {
            if !name.as_str().is_some_and(|name| ids.contains(&name)) {
                return Err(format!("safety.disables: unknown rule {name}"));
            }
        }
    }
    if let Some(deny) = map.get("deny") {
        let rules = deny
            .as_array()
            .filter(|rules| rules.len() <= MAX_DENY)
            .ok_or_else(|| format!("safety.deny must be an array of at most {MAX_DENY}"))?;
        for rule in rules {
            let rule = object(
                rule,
                &["id", "command", "reason", "disabled"],
                "safety.deny rule",
            )?;
            let id = rule
                .get("id")
                .and_then(Value::as_str)
                .filter(|id| kebab(id) && id.len() <= 64)
                .ok_or("safety.deny rule id must be kebab-case")?;
            if ids.contains(&id) || id == UNREADABLE.0 {
                return Err(format!("duplicate safety rule id: {id}"));
            }
            ids.push(id);
            // An entry that only removes the rule of this id an earlier layer declared.
            match rule.get("disabled") {
                None | Some(Value::Bool(false)) => {}
                Some(Value::Bool(true)) if rule.len() == 2 => continue,
                Some(Value::Bool(true)) => {
                    return Err(format!(
                        "safety rule {id}: a disabled entry takes only its id"
                    ));
                }
                Some(_) => return Err(format!("safety rule {id}: disabled must be boolean")),
            }
            let words = rule.get("command").and_then(Value::as_array);
            let valid = words.is_some_and(|words| {
                (1..=MAX_WORDS).contains(&words.len())
                    && words.iter().all(|word| {
                        word.as_str().is_some_and(|word| {
                            !word.is_empty() && !word.contains(char::is_whitespace)
                        })
                    })
            });
            if !valid {
                return Err(format!(
                    "safety rule {id}: command must be 1..{MAX_WORDS} words without spaces"
                ));
            }
            if !rule.get("reason").is_none_or(|reason| {
                reason.as_str().is_some_and(|reason| {
                    !reason.trim().is_empty() && reason.chars().count() <= MAX_REASON
                })
            }) {
                return Err(format!(
                    "safety rule {id}: reason must be text of at most {MAX_REASON} characters"
                ));
            }
        }
    }
    Ok(())
}

/// `safety.secretFiles`: at most [`secrets::MAX_DECLARED`] entries of a unique kebab-case id
/// and a pattern, or an id and `disabled` to remove the entry an earlier layer declared.
fn validate_secret_files(patterns: &Value) -> Result {
    let max = secrets::MAX_DECLARED;
    let entries = patterns
        .as_array()
        .filter(|entries| entries.len() <= max)
        .ok_or_else(|| format!("safety.secretFiles must be an array of at most {max}"))?;
    let mut ids = Vec::new();
    for entry in entries {
        let entry = object(
            entry,
            &["id", "pattern", "disabled"],
            "safety.secretFiles entry",
        )?;
        let id = entry
            .get("id")
            .and_then(Value::as_str)
            .filter(|id| kebab(id) && id.len() <= 64)
            .ok_or("safety.secretFiles entry id must be kebab-case")?;
        if ids.contains(&id) {
            return Err(format!("duplicate safety.secretFiles id: {id}"));
        }
        ids.push(id);
        match entry.get("disabled") {
            None | Some(Value::Bool(false)) => {}
            Some(Value::Bool(true)) if entry.len() == 2 => continue,
            Some(Value::Bool(true)) => {
                return Err(format!(
                    "safety.secretFiles {id}: a disabled entry takes only its id"
                ));
            }
            Some(_) => {
                return Err(format!("safety.secretFiles {id}: disabled must be boolean"));
            }
        }
        let pattern = entry
            .get("pattern")
            .and_then(Value::as_str)
            .ok_or_else(|| format!("safety.secretFiles {id}: pattern must be text"))?;
        if let Some(error) = secrets::pattern_error(pattern) {
            return Err(format!("safety.secretFiles {id}: pattern {error}"));
        }
    }
    Ok(())
}

fn disabled(settings: &Value, id: &str) -> bool {
    settings["safety"]["disables"]
        .as_array()
        .is_some_and(|names| names.iter().any(|name| name == id))
}

fn project_rules(settings: &Value) -> &[Value] {
    settings["safety"]["deny"]
        .as_array()
        .map_or(&[], Vec::as_slice)
}

/// The policy as `craft settings` shows it: whether it is on, each rule in force with where
/// it comes from, the built-in rules the project turned off and the secret-file patterns a
/// layer added.
pub fn listing(settings: &Value) -> Value {
    let mut rules: Vec<Value> = RULES
        .iter()
        .filter(|(id, _)| !disabled(settings, id))
        .map(|(id, refuses)| json!({"id": id, "source": "craft", "refuses": refuses}))
        .collect();
    rules.extend(project_rules(settings).iter().map(|rule| {
        json!({
            "id": rule["id"],
            "source": "project",
            "refuses": rule["reason"].as_str().map_or_else(
                || format!("commands starting with `{}`", words_of(rule).join(" ")),
                str::to_owned,
            ),
        })
    }));
    let off: Vec<&str> = RULES
        .iter()
        .map(|(id, _)| *id)
        .filter(|id| disabled(settings, id))
        .collect();
    let patterns: Vec<Value> = settings["safety"]["secretFiles"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|entry| !crate::layers::disabled(entry))
        .map(|entry| json!({"id": entry["id"], "pattern": entry["pattern"], "source": "project"}))
        .collect();
    json!({
        "enabled": enabled(settings),
        "origin": "bundled",
        "rules": rules,
        "disabled": off,
        "secretFiles": patterns,
        "effect": "Refuses matching shell commands and reads of secret files where the host lets a hook refuse a tool call; where the host can ask the user instead (Claude), a secret read waits for the user. Approves nothing. A guardrail, not a security boundary.",
    })
}

/// `listing` for resolved settings (`settings`, `origins`, `entries`): the switch and each
/// declared rule name the layer they came from.
pub fn layered_listing(resolved: &Value) -> Value {
    let mut listing = listing(&resolved["settings"]);
    if let Some(origin) = resolved["origins"]["safety.enabled"].as_str() {
        listing["origin"] = json!(origin);
    }
    let layers = &resolved["entries"]["safety.deny"];
    for rule in listing["rules"].as_array_mut().into_iter().flatten() {
        let layer = rule["id"].as_str().and_then(|id| layers[id].as_str());
        if let Some(layer) = layer.filter(|_| rule["source"] == "project") {
            rule["source"] = json!(layer);
        }
    }
    let layers = &resolved["entries"]["safety.secretFiles"];
    for pattern in listing["secretFiles"].as_array_mut().into_iter().flatten() {
        if let Some(layer) = pattern["id"].as_str().and_then(|id| layers[id].as_str()) {
            pattern["source"] = json!(layer);
        }
    }
    listing
}

fn words_of(rule: &Value) -> Vec<&str> {
    rule["command"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .collect()
}

fn basename(word: &str) -> &str {
    word.rsplit('/').next().unwrap_or(word)
}

/// The command a segment runs: leading variable assignments and wrapper words removed.
pub(crate) fn command(words: &[String]) -> &[String] {
    let mut rest = words;
    loop {
        match rest.first().map(String::as_str) {
            Some(word) if assignment(word) => rest = &rest[1..],
            Some(word) if WRAPPERS.contains(&basename(word)) => {
                rest = &rest[1..];
                // `env -i`, `time -p`, `exec -a name`: options belong to the wrapper.
                while rest.first().is_some_and(|word| word.starts_with('-')) {
                    rest = &rest[1..];
                }
            }
            _ => return rest,
        }
    }
}

fn assignment(word: &str) -> bool {
    word.split_once('=').is_some_and(|(name, _)| {
        !name.is_empty()
            && !name.starts_with(|c: char| c.is_ascii_digit())
            && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
    })
}

/// A short option group holding `flag`, as `-f`, `-rf` or `-fd`; never a long option.
fn short(word: &str, flag: char) -> bool {
    word.strip_prefix('-')
        .is_some_and(|group| !group.starts_with('-') && group.contains(flag))
}

/// A path's segments with `.` and `..` resolved by name; `None` when it climbs above `/`.
fn normalized(path: &str) -> Option<Vec<&str>> {
    let mut parts = Vec::new();
    for part in path.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop()?;
            }
            part => parts.push(part),
        }
    }
    Some(parts)
}

/// Where a delete target lies, by name alone: nothing is read from disk.
#[derive(Debug, PartialEq, Eq)]
enum Target {
    Inside,
    /// The project root, all of its content or its Git directory.
    Root,
    Outside,
    /// A variable or substitution, which is not expanded here.
    Unexpanded,
}

/// A Windows drive segment such as `C:`.
fn drive(segment: &str) -> bool {
    let bytes = segment.as_bytes();
    bytes.len() == 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':'
}

/// `path` below `base`: the segments left, with drive letters compared without case.
fn below<'p>(path: &'p [&'p str], base: &[&str]) -> Option<&'p [&'p str]> {
    (path.len() >= base.len()
        && path
            .iter()
            .zip(base)
            .all(|(a, b)| a == b || drive(a) && a.eq_ignore_ascii_case(b)))
    .then(|| &path[base.len()..])
}

/// A segment that names everything in a directory: `*`, `.`, `.*` and the like.
fn everything(segment: &str) -> bool {
    segment.chars().all(|c| matches!(c, '*' | '.' | '?'))
}

fn target(place: &Place, word: &str) -> Target {
    if word.starts_with('~') {
        return Target::Outside;
    }
    if word.contains('$') || word.contains('`') {
        return Target::Unexpanded;
    }
    // A Windows drive or network path, with either separator; a bare drive (`D:`, `D:x`)
    // means that drive's own working directory, which is not known here.
    let windows = word.starts_with("\\\\") || word.get(..2).is_some_and(drive);
    let word = if windows {
        word.replace('\\', "/")
    } else {
        word.to_string()
    };
    let absolute = if word.starts_with('/') || word.get(2..3) == Some("/") && windows {
        word
    } else if windows {
        return Target::Outside;
    } else {
        format!("{}/{word}", place.cwd)
    };
    let Some(path) = normalized(&absolute) else {
        return Target::Outside;
    };
    for root in place.roots.iter().filter_map(|root| normalized(root)) {
        if let Some(rest) = below(&path, &root) {
            return match rest {
                [] | [".git"] => Target::Root,
                [all] if everything(all) => Target::Root,
                _ => Target::Inside,
            };
        }
    }
    // Strictly below a temporary directory: never the directory itself or all of it.
    let temporary = place
        .temp
        .iter()
        .filter_map(|temp| normalized(temp))
        .any(|temp| match below(&path, &temp) {
            None | Some([]) => false,
            Some([one]) => !everything(one),
            Some(_) => true,
        });
    if temporary {
        Target::Inside
    } else {
        Target::Outside
    }
}

fn recursive(word: &str) -> bool {
    word == "--recursive" || short(word, 'r') || short(word, 'R')
}

/// A built-in rule a command breaks and, for a delete refused only for a target the policy
/// cannot expand, that target as written.
type Broken = (&'static str, Option<String>);

/// The refusal for delete targets judged at `places`: outside the project or read from
/// `input` first, then a target holding a variable, then the project root unless `narrowed`.
fn verdict(places: &[(Target, &str)], input: bool, narrowed: bool) -> Option<Broken> {
    let found = |kind: Target| places.iter().find(|(place, _)| *place == kind);
    if input || found(Target::Outside).is_some() {
        Some(("delete-outside-project", None))
    } else if let Some((_, word)) = found(Target::Unexpanded) {
        Some(("delete-outside-project", Some((*word).to_string())))
    } else if found(Target::Root).is_some() && !narrowed {
        Some(("delete-project-root", None))
    } else {
        None
    }
}

/// A recursive `rm` judged by its targets; `input` adds targets read from standard input,
/// which are not known here.
fn delete(args: &[String], place: &Place, input: bool) -> Option<Broken> {
    let mut recursive = false;
    let mut targets = Vec::new();
    let mut options = true;
    for word in args {
        if options && word == "--" {
            options = false;
        } else if options && word.starts_with('-') && word.len() > 1 {
            recursive |= self::recursive(word);
        } else {
            targets.push(word.as_str());
        }
    }
    if !recursive {
        return None;
    }
    let places: Vec<(Target, &str)> = targets.iter().map(|t| (target(place, t), *t)).collect();
    verdict(&places, input, false)
}

fn git(args: &[String]) -> Option<&'static str> {
    let mut index = 0;
    while index < args.len() && args[index].starts_with('-') {
        if GIT_VALUE_OPTIONS.contains(&args[index].as_str()) {
            index += 1;
        }
        index += 1;
    }
    let (verb, rest) = args.get(index..)?.split_first()?;
    let has = |name: &str| rest.iter().any(|word| word == name);
    let whole_tree = || {
        rest.iter()
            .any(|word| matches!(word.as_str(), "." | ":/" | "*"))
    };
    match verb.as_str() {
        "push"
            if has("--force")
                || has("--mirror")
                || rest.iter().any(|word| short(word, 'f'))
                || rest
                    .iter()
                    .any(|word| word.starts_with('+') && word.len() > 1) =>
        {
            Some("force-push")
        }
        // `--delete`, `-d`, `--prune` or an empty source, as in `:branch`.
        "push" => (has("--delete")
            || has("--prune")
            || rest.iter().any(|word| short(word, 'd'))
            || rest
                .iter()
                .any(|word| word.starts_with(':') && word.len() > 1))
        .then_some("delete-remote-branch"),
        "reset" => has("--hard").then_some("discard-changes"),
        "checkout" => (whole_tree() || has("--force") || rest.iter().any(|w| short(w, 'f')))
            .then_some("discard-changes"),
        "restore" => (whole_tree() && (!has("--staged") && !has("-S") || has("--worktree")))
            .then_some("discard-changes"),
        "clean" => ((has("--force") || rest.iter().any(|w| short(w, 'f')))
            && !has("--dry-run")
            && !rest.iter().any(|w| short(w, 'n')))
        .then_some("discard-changes"),
        "stash" => rest
            .first()
            .is_some_and(|action| action == "drop" || action == "clear")
            .then_some("discard-changes"),
        _ => None,
    }
}

fn system(executable: &str, args: &[String]) -> bool {
    if SYSTEM.contains(&executable) || executable.starts_with("mkfs") {
        return true;
    }
    let positional = || args.iter().filter(|word| !word.starts_with('-'));
    match executable {
        "systemctl" | "service" | "launchctl" => {
            positional().any(|word| SERVICE_ACTIONS.contains(&word.as_str()))
                || (executable == "launchctl"
                    && positional()
                        .any(|word| matches!(word.as_str(), "unload" | "bootout" | "remove")))
        }
        // Without a device and a place, `mount` only lists.
        "mount" => positional().count() >= 2,
        "dd" => args.iter().any(|word| word.starts_with("of=/dev/")),
        _ => false,
    }
}

/// The command `xargs` runs, with the targets it appends from standard input.
fn xargs(args: &[String], place: &Place) -> Option<Broken> {
    let mut index = 0;
    while let Some(word) = args.get(index).filter(|word| word.starts_with('-')) {
        index += if XARGS_VALUE_OPTIONS.contains(&word.as_str()) {
            2
        } else {
            1
        };
    }
    let words = command(args.get(index..)?);
    match words.split_first() {
        Some((rm, args)) if basename(rm) == "rm" => delete(args, place, true),
        _ => builtin(words, place),
    }
}

/// `find … -delete`, or `-exec rm -r`, judged by the places the search starts from: one
/// outside the project, or the project root with nothing narrowing what is matched.
fn find(args: &[String], place: &Place) -> Option<Broken> {
    let mut index = 0;
    while let Some(word) = args.get(index) {
        match word.as_str() {
            "-H" | "-L" | "-P" => index += 1,
            "-D" => index += 2,
            word if word.starts_with("-O") => index += 1,
            _ => break,
        }
    }
    let rest = args.get(index..).unwrap_or_default();
    let starts: Vec<&str> = rest
        .iter()
        .map(String::as_str)
        .take_while(|word| !word.starts_with('-') && !matches!(*word, "(" | "!"))
        .collect();
    let expression = &rest[starts.len()..];
    let mut deletes = false;
    let mut at = 0;
    while at < expression.len() {
        match expression[at].as_str() {
            "-delete" => deletes = true,
            "-exec" | "-execdir" | "-ok" | "-okdir" => {
                let end = expression[at + 1..]
                    .iter()
                    .position(|word| matches!(word.as_str(), ";" | "\\;" | "\\" | "+"))
                    .map_or(expression.len(), |end| at + 1 + end);
                let run = command(&expression[at + 1..end]);
                deletes |= run.split_first().is_some_and(|(rm, args)| {
                    basename(rm) == "rm" && args.iter().any(|word| recursive(word))
                });
                at = end;
            }
            _ => {}
        }
        at += 1;
    }
    if !deletes {
        return None;
    }
    let narrowed = expression
        .iter()
        .any(|word| FIND_TESTS.contains(&word.as_str()));
    let starts = if starts.is_empty() { vec!["."] } else { starts };
    let places: Vec<(Target, &str)> = starts
        .iter()
        .map(|start| (target(place, start), *start))
        .collect();
    verdict(&places, false, narrowed)
}

/// The built-in rule one command segment breaks, if any.
fn builtin(words: &[String], place: &Place) -> Option<Broken> {
    let (executable, args) = words.split_first()?;
    let executable = basename(executable);
    match executable {
        "rm" => delete(args, place, false),
        "xargs" => xargs(args, place),
        "find" => find(args, place),
        "git" => git(args).map(|id| (id, None)),
        _ => system(executable, args).then_some(("system-administration", None)),
    }
}

/// Why a shell command is refused, said to the model.
const REFUSE_COMMAND: Template = Template {
    id: "safety.refuse-command",
    slots: &["rule", "refuses"],
    source: include_str!("../text/safety/refuse-command.txt"),
};

/// Why a secret file stays out of the model's context, said to the model.
const REFUSE_SECRET: Template = Template {
    id: "safety.refuse-secret",
    slots: &["file", "rule"],
    source: include_str!("../text/safety/refuse-secret.txt"),
};

/// The question shown to the user where the host hands a secret read to them.
const ASK_SECRET: Template = Template {
    id: "safety.ask-secret",
    slots: &["file", "rule"],
    source: include_str!("../text/safety/ask-secret.txt"),
};

/// Why a recursive delete of a target holding a variable is refused, said to the model.
const REFUSE_UNEXPANDED: Template = Template {
    id: "safety.refuse-unexpanded",
    slots: &["rule", "target"],
    source: include_str!("../text/safety/refuse-unexpanded.txt"),
};

pub(crate) const TEMPLATES: [&Template; 4] = [
    &REFUSE_COMMAND,
    &REFUSE_UNEXPANDED,
    &REFUSE_SECRET,
    &ASK_SECRET,
];

/// The most characters of a delete target repeated in a refusal.
const MAX_TARGET: usize = 120;

/// A refusal of `rule`. A secret read also carries `ask`: the words shown to the user
/// where the host can hand the call to them instead of refusing it.
fn refusal(rule: &str, refuses: &str, secret: Option<&str>) -> Value {
    let Some(file) = secret else {
        return json!({
            "decision": "deny",
            "rule": rule,
            "reason": REFUSE_COMMAND.render(&[("rule", rule), ("refuses", refuses)]),
        });
    };
    let values = [("file", file), ("rule", rule)];
    json!({
        "decision": "deny",
        "rule": rule,
        "file": file,
        "reason": REFUSE_SECRET.render(&values),
        "ask": ASK_SECRET.render(&values),
    })
}

/// The refusal of a recursive delete whose `target` holds a variable or substitution: it
/// names the target and the literal paths that pass instead.
fn unexpanded_refusal(rule: &str, target: &str) -> Value {
    let target: String = target.chars().take(MAX_TARGET).collect();
    json!({
        "decision": "deny",
        "rule": rule,
        "reason": REFUSE_UNEXPANDED.render(&[("rule", rule), ("target", &target)]),
    })
}

/// The first file or search filter a tool call names that reaches secret files by the
/// built-in names; [`declared_secret`] adds the patterns a layer declared.
pub fn named_secret<'a>(files: &[&'a str], filters: &[&'a str]) -> Option<&'a str> {
    declared(&secrets::Declared::default(), files, filters)
}

/// The first file or search filter a tool call names that reaches secret files by the
/// built-in names or a pattern `settings` declare.
pub fn declared_secret<'a>(
    settings: &Value,
    files: &[&'a str],
    filters: &[&'a str],
) -> Option<&'a str> {
    declared(&secrets::Declared::of(settings), files, filters)
}

fn declared<'a>(
    patterns: &secrets::Declared,
    files: &[&'a str],
    filters: &[&'a str],
) -> Option<&'a str> {
    files
        .iter()
        .find(|file| patterns.secret_file(file))
        .or_else(|| filters.iter().find(|filter| patterns.secret_filter(filter)))
        .copied()
}

/// The decision for a tool call that reads, searches or rewrites `secret`, a file or filter
/// [`named_secret`] found: the secret-file refusal while the rule is in force, else `allow`.
pub fn read_decision(settings: &Value, secret: &str) -> Value {
    if !guards_secret_files(settings) {
        return json!({"decision": "allow"});
    }
    refusal(SECRET_RULE, "", Some(secret))
}

/// The decision for one shell command run at `place`: `allow`, or `deny` with the rule and
/// a reason. With the policy off every command is allowed.
pub fn decision(settings: &Value, place: &Place, shell: &str) -> Value {
    if !enabled(settings) {
        return json!({"decision": "allow"});
    }
    let secrets_on = !disabled(settings, SECRET_RULE);
    let patterns = secrets::Declared::of(settings);
    let mut broken: Option<Value> = None;
    let too_deep = walk(shell, &mut |words| {
        let words = command(words);
        if let Some((id, unexpanded)) =
            builtin(words, place).filter(|(id, _)| !disabled(settings, id))
        {
            broken = Some(match unexpanded {
                Some(target) => unexpanded_refusal(id, &target),
                None => {
                    let refuses = RULES
                        .iter()
                        .find(|(known, _)| *known == id)
                        .map(|(_, r)| *r);
                    refusal(id, refuses.unwrap_or_default(), None)
                }
            });
            return true;
        }
        if let Some(file) = patterns.shell_read(words).filter(|_| secrets_on) {
            broken = Some(refusal(SECRET_RULE, "", Some(file)));
            return true;
        }
        for rule in project_rules(settings) {
            let prefix = words_of(rule);
            let matches = words.len() >= prefix.len()
                && !prefix.is_empty()
                && basename(&words[0]) == prefix[0]
                && words[1..prefix.len()]
                    .iter()
                    .zip(&prefix[1..])
                    .all(|(word, expected)| word == expected);
            if matches {
                let refuses = rule["reason"].as_str().map_or_else(
                    || format!("commands starting with `{}`", prefix.join(" ")),
                    str::to_owned,
                );
                let id = rule["id"].as_str().unwrap_or_default();
                broken = Some(refusal(id, &refuses, None));
                return true;
            }
        }
        false
    });
    match broken {
        Some(refused) => refused,
        None if too_deep => refusal(UNREADABLE.0, UNREADABLE.1, None),
        None => json!({"decision": "allow"}),
    }
}

#[cfg(test)]
mod tests;
