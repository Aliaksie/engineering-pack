//! The project's safety policy for shell commands: opt-in rules that refuse destructive
//! commands and commands that reach outside the project. A rule only ever refuses; nothing
//! here approves a command, so the host's own prompts, sandbox and deny rules keep applying.
//!
//! It is a guardrail, not a security boundary. The recognizer reads literal commands,
//! scripts passed to supported shells with `-c` and `$()`/backtick substitutions; aliases,
//! other interpreters, variable expansion and scripts on disk are outside it.
use crate::contracts::kebab;
use crate::review::walk;
use crate::values::object;
use serde_json::{Value, json};

type Result<T = ()> = std::result::Result<T, String>;

/// A built-in rule: its id and what it refuses, in the words shown to the user and model.
pub const RULES: [(&str, &str); 5] = [
    (
        "delete-outside-project",
        "a recursive delete whose target is outside the project, in the home directory or an unexpanded variable",
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
        "discard-changes",
        "a Git command that discards uncommitted work or stashes",
    ),
    (
        "system-administration",
        "a privileged or system-level command (sudo, shutdown, service, mount or disk changes)",
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

/// Whether the policy is on in resolved settings, where it is on unless a layer or the
/// project sets `safety.enabled` to false.
pub fn enabled(settings: &Value) -> bool {
    settings["safety"]["enabled"] == true
}

/// Validate `safety`: `enabled`, `disables` naming built-in rules, and `deny` rules of the
/// project's own, each an id, the leading words of a command and an optional reason.
pub fn validate(safety: &Value) -> Result {
    let map = object(safety, &["enabled", "disables", "deny"], "safety")?;
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
/// it comes from, and the built-in rules the project turned off.
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
    json!({
        "enabled": enabled(settings),
        "origin": "bundled",
        "rules": rules,
        "disabled": off,
        "effect": "Refuses matching shell commands where the host lets a hook refuse a tool call; approves nothing. A guardrail, not a security boundary.",
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
fn command(words: &[String]) -> &[String] {
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
}

fn target(root: &str, cwd: &str, word: &str) -> Target {
    if word.starts_with('~') || word.contains('$') || word.contains('`') {
        return Target::Outside;
    }
    let absolute = if word.starts_with('/') {
        word.to_string()
    } else {
        format!("{cwd}/{word}")
    };
    let (Some(path), Some(root)) = (normalized(&absolute), normalized(root)) else {
        return Target::Outside;
    };
    if !path.starts_with(&root) {
        return Target::Outside;
    }
    match &path[root.len()..] {
        [] | [".git"] => Target::Root,
        [all] if all.chars().all(|c| matches!(c, '*' | '.' | '?')) => Target::Root,
        _ => Target::Inside,
    }
}

fn delete(args: &[String], root: &str, cwd: &str) -> Option<&'static str> {
    let mut recursive = false;
    let mut targets = Vec::new();
    let mut options = true;
    for word in args {
        if options && word == "--" {
            options = false;
        } else if options && word.starts_with('-') && word.len() > 1 {
            recursive |= word == "--recursive" || short(word, 'r') || short(word, 'R');
        } else {
            targets.push(word.as_str());
        }
    }
    if !recursive {
        return None;
    }
    let places: Vec<Target> = targets.iter().map(|t| target(root, cwd, t)).collect();
    if places.contains(&Target::Outside) {
        Some("delete-outside-project")
    } else if places.contains(&Target::Root) {
        Some("delete-project-root")
    } else {
        None
    }
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
        "push" => (has("--force")
            || has("--mirror")
            || rest.iter().any(|word| short(word, 'f'))
            || rest
                .iter()
                .any(|word| word.starts_with('+') && word.len() > 1))
        .then_some("force-push"),
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

/// The built-in rule one command segment breaks, if any.
fn builtin(words: &[String], root: &str, cwd: &str) -> Option<&'static str> {
    let (executable, args) = words.split_first()?;
    let executable = basename(executable);
    match executable {
        "rm" => delete(args, root, cwd),
        "git" => git(args),
        _ => system(executable, args).then_some("system-administration"),
    }
}

/// The decision for one shell command run from `cwd` in the project at `root` (both
/// absolute, `/`-separated): `allow`, or `deny` with the rule and a reason. With the policy
/// off every command is allowed.
pub fn decision(settings: &Value, root: &str, cwd: &str, shell: &str) -> Value {
    if !enabled(settings) {
        return json!({"decision": "allow"});
    }
    let mut broken: Option<(String, String)> = None;
    let too_deep = walk(shell, &mut |words| {
        let words = command(words);
        if let Some(id) = builtin(words, root, cwd).filter(|id| !disabled(settings, id)) {
            let refuses = RULES
                .iter()
                .find(|(known, _)| *known == id)
                .map(|(_, r)| *r);
            broken = Some((id.to_string(), refuses.unwrap_or_default().to_string()));
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
                broken = Some((rule["id"].as_str().unwrap_or_default().to_string(), refuses));
                return true;
            }
        }
        false
    });
    let (rule, refuses) = match broken {
        Some(found) => found,
        None if too_deep => (UNREADABLE.0.to_string(), UNREADABLE.1.to_string()),
        None => return json!({"decision": "allow"}),
    };
    json!({
        "decision": "deny",
        "rule": rule,
        "reason": format!(
            "This project's safety policy refuses this command (rule `{rule}`: {refuses}). Do not reword it to get past the rule; tell the user what you wanted to run and why, so they can run it themselves or change the request."
        ),
    })
}

#[cfg(test)]
mod tests;
