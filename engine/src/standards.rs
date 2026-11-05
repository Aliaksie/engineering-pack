//! Consumer standards: what the project declares "good" means, where it applies and how it
//! is kept. A declaration names a file, a folder or a known anchor; routing selects those
//! whose scope matches the work, and anchors with a deterministic check are verified without
//! a model. Declarations are the consumer's; nothing here reads or runs their content.
use crate::contracts::{LANGUAGES, LAYERS, kebab, relative_file};
use crate::routing::member;
use crate::values::object;
use serde_json::{Map, Value, json};

pub mod anchors;

type Result<T = ()> = std::result::Result<T, String>;

pub const LEVELS: &[&str] = &["advise", "check", "review"];
const SOURCES: [&str; 3] = ["file", "folder", "anchor"];
const SCOPE: [&str; 5] = ["modules", "languages", "layers", "paths", "intents"];
/// How many standards a project declares; routes show only the matching ones.
const MAX_STANDARDS: usize = 64;
const MAX_CHECKLIST: usize = 16;
const MAX_TEXT: usize = 200;

fn text(value: Option<&Value>, label: &str) -> Result<Option<String>> {
    match value {
        None => Ok(None),
        Some(Value::String(text))
            if !text.trim().is_empty() && text.chars().count() <= MAX_TEXT =>
        {
            Ok(Some(text.clone()))
        }
        Some(_) => Err(format!(
            "{label} must be text of at most {MAX_TEXT} characters"
        )),
    }
}

fn names<'a>(value: Option<&'a Value>, label: &str) -> Result<Vec<&'a str>> {
    let Some(value) = value else {
        return Ok(Vec::new());
    };
    let items = value
        .as_array()
        .filter(|items| !items.is_empty())
        .ok_or_else(|| format!("{label} must be a non-empty array"))?;
    items
        .iter()
        .map(|item| {
            item.as_str()
                .ok_or_else(|| format!("{label} entries must be text"))
        })
        .collect()
}

/// The declared source: exactly one of `file`, `folder` or `anchor`; a `review` with an
/// inline checklist needs none.
fn source(standard: &Map<String, Value>) -> Option<(&'static str, &Value)> {
    SOURCES
        .iter()
        .find_map(|kind| standard.get(*kind).map(|value| (*kind, value)))
}

/// The level a declaration is kept at: as declared, else what its form implies.
pub fn level(standard: &Value) -> &str {
    if let Some(level) = standard["level"].as_str() {
        return level;
    }
    if standard.get("run").is_some() {
        "check"
    } else if standard.get("checklist").is_some() {
        "review"
    } else if standard.get("anchor").is_some() {
        "check"
    } else {
        "advise"
    }
}

/// Validate `standards` against the declared module names and the routing intents.
pub fn validate(standards: &Value, modules: &[&str], intents: &[&str]) -> Result {
    let list = standards
        .as_array()
        .filter(|list| list.len() <= MAX_STANDARDS)
        .ok_or_else(|| format!("standards must be an array of at most {MAX_STANDARDS}"))?;
    let mut ids: Vec<&str> = Vec::new();
    for entry in list {
        let standard = object(
            entry,
            &[
                "id",
                "file",
                "folder",
                "anchor",
                "summary",
                "level",
                "scope",
                "run",
                "checklist",
            ],
            "standard",
        )?;
        let id = standard
            .get("id")
            .and_then(Value::as_str)
            .filter(|id| kebab(id) && id.len() <= 64)
            .ok_or("standard id must be kebab-case")?;
        if ids.contains(&id) {
            return Err(format!("duplicate standard id: {id}"));
        }
        ids.push(id);
        let sources = SOURCES
            .iter()
            .filter(|kind| standard.contains_key(**kind))
            .count();
        if sources > 1 {
            return Err(format!(
                "standard {id} names more than one of file, folder, anchor"
            ));
        }
        match source(standard) {
            Some(("anchor", name)) => {
                let name = name.as_str().unwrap_or_default();
                if anchors::find(name).is_none() {
                    return Err(format!("standard {id}: unknown anchor {name}"));
                }
            }
            Some((_, path)) => {
                relative_file(path).map_err(|error| format!("standard {id}: {error}"))?;
            }
            None if standard.contains_key("checklist") || standard.contains_key("run") => {}
            None => {
                return Err(format!(
                    "standard {id} needs a file, folder, anchor, run or checklist"
                ));
            }
        }
        text(standard.get("summary"), &format!("standard {id} summary"))?;
        let level = match standard.get("level") {
            None => level(entry),
            Some(level) => level
                .as_str()
                .filter(|level| LEVELS.contains(level))
                .ok_or_else(|| format!("standard {id} level must be advise, check or review"))?,
        };
        if let Some(scope) = standard.get("scope") {
            let scope = object(scope, &SCOPE, "standard scope")?;
            for module in names(scope.get("modules"), "scope.modules")? {
                if !modules.contains(&module) {
                    return Err(format!("standard {id}: unknown module {module}"));
                }
            }
            for (field, allowed) in [
                ("languages", LANGUAGES),
                ("layers", LAYERS),
                ("intents", intents),
            ] {
                for value in names(scope.get(field), &format!("scope.{field}"))? {
                    if !allowed.contains(&value) {
                        return Err(format!("standard {id}: unknown {field} value {value}"));
                    }
                }
            }
            for path in names(scope.get("paths"), "scope.paths")? {
                relative_file(&json!(path.strip_suffix('/').unwrap_or(path)))
                    .map_err(|error| format!("standard {id}: {error}"))?;
            }
        }
        if let Some(run) = standard.get("run") {
            let run = object(run, &["command", "args", "cwd"], "standard run")?;
            if !run["command"]
                .as_str()
                .is_some_and(|c| !c.trim().is_empty())
            {
                return Err(format!("standard {id} run needs a command"));
            }
            if run.get("args").is_some() {
                names(run.get("args"), "run.args")?;
            }
            if let Some(cwd) = run.get("cwd") {
                relative_file(cwd).map_err(|error| format!("standard {id}: {error}"))?;
            }
        }
        if let Some(checklist) = standard.get("checklist") {
            let items = names(Some(checklist), "checklist")?;
            if items.len() > MAX_CHECKLIST
                || items
                    .iter()
                    .any(|item| item.trim().is_empty() || item.chars().count() > MAX_TEXT)
            {
                return Err(format!(
                    "standard {id} checklist takes 1..{MAX_CHECKLIST} items of at most {MAX_TEXT} characters"
                ));
            }
        }
        match level {
            "check" if !standard.contains_key("run") && !standard.contains_key("anchor") => {
                return Err(format!(
                    "standard {id}: check needs an anchor or a run command"
                ));
            }
            "review" if !standard.contains_key("checklist") && source(standard).is_none() => {
                return Err(format!(
                    "standard {id}: review needs a checklist or a source"
                ));
            }
            _ => {}
        }
    }
    Ok(())
}

fn declared(settings: &Value) -> &[Value] {
    settings["standards"].as_array().map_or(&[], Vec::as_slice)
}

fn listed<'a>(standard: &'a Value, field: &str) -> Vec<&'a str> {
    standard["scope"][field]
        .as_array()
        .map(|items| items.iter().filter_map(Value::as_str).collect())
        .unwrap_or_default()
}

fn prefix(path: &str) -> String {
    relative_file(&json!(path.strip_suffix('/').unwrap_or(path))).unwrap_or_default()
}

/// The intents a declaration applies to: declared, else its anchor's; empty is any.
fn intents(standard: &Value) -> Vec<&str> {
    let declared = listed(standard, "intents");
    if !declared.is_empty() {
        return declared;
    }
    standard["anchor"]
        .as_str()
        .and_then(anchors::find)
        .map(|anchor| anchor.intents.to_vec())
        .unwrap_or_default()
}

/// What one route looked at: the facts a standard's scope is matched against.
pub struct Work<'a> {
    pub intent: &'a str,
    pub files: &'a [String],
    pub modules: &'a [Value],
    pub languages: &'a [Value],
    pub layers: &'a [Value],
}

fn matches(standard: &Value, work: &Work) -> bool {
    let intents = intents(standard);
    if !intents.is_empty() && !intents.contains(&work.intent) {
        return false;
    }
    let overlap = |field: &str, values: &[Value]| {
        let wanted = listed(standard, field);
        wanted.is_empty()
            || values
                .iter()
                .any(|value| value.as_str().is_some_and(|v| wanted.contains(&v)))
    };
    if !overlap("languages", work.languages) || !overlap("layers", work.layers) {
        return false;
    }
    let modules = listed(standard, "modules");
    let paths = listed(standard, "paths");
    if modules.is_empty() && paths.is_empty() {
        return true;
    }
    // Without files a place-scoped standard is shown only for the kind of work it names:
    // requirements in `docs/requirements` matter to story work, a UI lint rule to no plan.
    if work.files.is_empty() {
        return !intents.is_empty();
    }
    let in_module = modules.is_empty()
        || work.modules.iter().any(|module| {
            module["name"]
                .as_str()
                .is_some_and(|name| modules.contains(&name))
        });
    let in_path = paths.is_empty()
        || work
            .files
            .iter()
            .any(|file| paths.iter().any(|path| member(file, &prefix(path))));
    in_module && in_path
}

/// What a route shows for a standard: its handle, level and source, a one-line summary in
/// place of content, and content only where it is the thing to keep (a checklist, a command).
fn shown(standard: &Value) -> Value {
    let mut entry = json!({"id": standard["id"], "level": level(standard)});
    for kind in SOURCES {
        if let Some(value) = standard.get(kind) {
            entry[kind] = value.clone();
        }
    }
    let summary = standard["summary"].as_str().or_else(|| {
        standard["anchor"]
            .as_str()
            .and_then(anchors::find)
            .map(|anchor| anchor.summary)
    });
    if let Some(summary) = summary {
        entry["summary"] = json!(summary);
    }
    for field in ["run", "checklist"] {
        if let Some(value) = standard.get(field) {
            entry[field] = value.clone();
        }
    }
    entry
}

/// The declared standards a route's work falls under, in declaration order.
pub fn select(settings: &Value, work: &Work) -> Vec<Value> {
    declared(settings)
        .iter()
        .filter(|standard| matches(standard, work))
        .map(shown)
        .collect()
}

/// Whether the project keeps its anchor checks on: on unless `routing.standardChecks` is false.
pub fn checks_enabled(settings: &Value) -> bool {
    settings["routing"]["standardChecks"] != false
}

/// Whether `file` is one an anchor reads by default: a named file anywhere, or an extension.
fn default_target(anchor: &anchors::Anchor, file: &str) -> bool {
    let name = file.rsplit('/').next().unwrap_or(file);
    anchor.targets.iter().any(|target| {
        if target.starts_with('.') {
            name.ends_with(target)
        } else {
            name == *target
        }
    })
}

/// The anchor checks that read `file` after it is written: `(standard id, anchor name)`.
/// A declared `scope.paths` replaces the anchor's default targets; commit-message anchors read
/// no files.
pub fn file_checks<'a>(settings: &'a Value, file: &str) -> Vec<(&'a str, &'a str)> {
    if !checks_enabled(settings) {
        return Vec::new();
    }
    declared(settings)
        .iter()
        .filter(|standard| level(standard) == "check")
        .filter_map(|standard| {
            let name = standard["anchor"].as_str()?;
            let anchor = anchors::find(name)?;
            if anchor.input == anchors::Input::CommitMessage {
                return None;
            }
            let paths = listed(standard, "paths");
            let applies = if paths.is_empty() {
                default_target(anchor, file)
            } else {
                paths.iter().any(|path| member(file, &prefix(path)))
            };
            applies.then(|| (standard["id"].as_str().unwrap_or_default(), name))
        })
        .collect()
}

/// The anchor checks for a commit message: `(standard id, anchor name)`.
pub fn commit_checks(settings: &Value) -> Vec<(&str, &str)> {
    if !checks_enabled(settings) {
        return Vec::new();
    }
    declared(settings)
        .iter()
        .filter(|standard| level(standard) == "check")
        .filter_map(|standard| {
            let name = standard["anchor"].as_str()?;
            (anchors::find(name)?.input == anchors::Input::CommitMessage)
                .then(|| (standard["id"].as_str().unwrap_or_default(), name))
        })
        .collect()
}

/// Whether a shell command runs `git commit`: in any `;`, `&&`, `||` or `|` segment, `git`
/// with its global options (`-C <dir>`, `-c <key=value>`, flags) then `commit`.
pub fn commits(command: &str) -> bool {
    command.split(['\n', ';', '&', '|']).any(|segment| {
        // The segment's command word, after environment assignments: `echo git commit`
        // only prints.
        let mut words = segment
            .split_whitespace()
            .skip_while(|word| word.contains('=') && !word.starts_with('-'));
        if !words
            .next()
            .is_some_and(|word| word == "git" || word.ends_with("/git"))
        {
            return false;
        }
        while let Some(next) = words.next() {
            match next {
                "-C" | "-c" | "--git-dir" | "--work-tree" => {
                    words.next();
                }
                flag if flag.starts_with('-') => {}
                verb => return verb == "commit",
            }
        }
        false
    })
}

/// The result of running `checks` over `text`: only failing standards, each with findings.
pub fn verdicts(checks: &[(&str, &str)], text: &str, path: Option<&str>) -> Vec<Value> {
    checks
        .iter()
        .filter_map(|(id, anchor)| {
            let findings = anchors::check(anchor, text, path);
            (!findings.is_empty()).then(|| {
                json!({
                    "standard": id,
                    "anchor": anchor,
                    "findings": findings.iter().map(|f| {
                        let mut finding = json!({"rule": f.rule, "message": f.message});
                        if let Some(line) = f.line {
                            finding["line"] = json!(line);
                        }
                        finding
                    }).collect::<Vec<_>>(),
                })
            })
        })
        .collect()
}

#[cfg(test)]
mod tests;
