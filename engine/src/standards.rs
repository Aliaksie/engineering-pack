//! Consumer standards: what the project declares "good" means, where it applies and how it
//! is kept. A declaration names a file, a folder or a known anchor; routing selects those
//! whose scope matches the work, and anchors with a deterministic check are verified without
//! a model. Declarations are the consumer's; nothing here reads or runs their content.
use crate::anchors::{self, Anchor, Input, Tier};
use crate::contracts::{LANGUAGES, LAYERS, kebab, relative_file};
use crate::routing::member;
use crate::values::object;
use serde_json::{Map, Value, json};

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

/// The anchor a declaration names, as it resolves for the project.
fn anchor(settings: &Value, standard: &Value) -> Option<Anchor> {
    anchors::resolve(settings, standard["anchor"].as_str()?)
}

/// The level a declaration is kept at: as declared, else what its form implies. An anchor
/// with a check is checked, one with a checklist reviewed, any other advised.
pub fn level<'a>(settings: &Value, standard: &'a Value) -> &'a str {
    if let Some(level) = standard["level"].as_str() {
        return level;
    }
    if standard.get("run").is_some() {
        return "check";
    }
    if standard.get("checklist").is_some() {
        return "review";
    }
    match anchor(settings, standard) {
        Some(anchor) if anchor.check.is_some() => "check",
        Some(anchor) if !anchor.checklist.is_empty() => "review",
        _ => "advise",
    }
}

/// Validate a declaration's `scope` against the declared modules and routing intents;
/// `label` names the declaration in errors.
pub(crate) fn validate_scope(
    scope: &Value,
    label: &str,
    modules: &[&str],
    intents: &[&str],
) -> Result {
    let kind = label.split(' ').next().unwrap_or("declaration");
    let scope = object(scope, &SCOPE, &format!("{kind} scope"))?;
    for module in names(scope.get("modules"), "scope.modules")? {
        if !modules.contains(&module) {
            return Err(format!("{label}: unknown module {module}"));
        }
    }
    for (field, allowed) in [
        ("languages", LANGUAGES),
        ("layers", LAYERS),
        ("intents", intents),
    ] {
        for value in names(scope.get(field), &format!("scope.{field}"))? {
            if !allowed.contains(&value) {
                return Err(format!("{label}: unknown {field} value {value}"));
            }
        }
    }
    for path in names(scope.get("paths"), "scope.paths")? {
        relative_file(&json!(path.strip_suffix('/').unwrap_or(path)))
            .map_err(|error| format!("{label}: {error}"))?;
    }
    Ok(())
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
                if !name.as_str().is_some_and(kebab) {
                    return Err(format!("standard {id}: anchor must be a kebab-case name"));
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
            None => None,
            Some(level) => Some(
                level
                    .as_str()
                    .filter(|level| LEVELS.contains(level))
                    .ok_or_else(|| {
                        format!("standard {id} level must be advise, check or review")
                    })?,
            ),
        };
        if let Some(scope) = standard.get("scope") {
            validate_scope(scope, &format!("standard {id}"), modules, intents)?;
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
            Some("check") if !standard.contains_key("run") && !standard.contains_key("anchor") => {
                return Err(format!(
                    "standard {id}: check needs an anchor or a run command"
                ));
            }
            Some("review") if !standard.contains_key("checklist") && source(standard).is_none() => {
                return Err(format!(
                    "standard {id}: review needs a checklist or a source"
                ));
            }
            _ => {}
        }
    }
    Ok(())
}

/// Validate what needs the project's anchors: every named anchor resolves and is offered,
/// a declared `check` level has a check to run, and `anchors.disables` names known anchors.
pub fn validate_anchors(settings: &Value) -> Result {
    anchors::anchors().map_err(|error| format!("anchor overlay: {error}"))?;
    let offered = |name: &str, what: &str| -> Result<Anchor> {
        match anchors::resolve(settings, name) {
            None => Err(format!("{what}: unknown anchor {name}")),
            Some(anchor) if anchor.tier == Tier::Refused => Err(format!(
                "{what}: anchor {name} is not offered: {}",
                anchor.evidence.as_deref().unwrap_or_default()
            )),
            Some(anchor) => Ok(anchor),
        }
    };
    for standard in declared(settings) {
        let Some(name) = standard["anchor"].as_str() else {
            continue;
        };
        let id = standard["id"].as_str().unwrap_or_default();
        let anchor = offered(name, &format!("standard {id}"))?;
        if standard["level"] == "check" && anchor.check.is_none() && standard.get("run").is_none() {
            return Err(format!(
                "standard {id}: anchor {name} has no check; use advise or review"
            ));
        }
    }
    for name in settings["anchors"]["disables"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
    {
        offered(name, "anchors.disables")?;
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
fn intents(settings: &Value, standard: &Value) -> Vec<String> {
    let declared = listed(standard, "intents");
    if !declared.is_empty() {
        return declared.into_iter().map(str::to_owned).collect();
    }
    anchor(settings, standard)
        .map(|anchor| anchor.intents)
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

/// Whether a declaration's `scope` (and its anchor's intents) covers `work`.
pub(crate) fn matches(settings: &Value, standard: &Value, work: &Work) -> bool {
    let intents = intents(settings, standard);
    if !intents.is_empty() && !intents.iter().any(|intent| intent == work.intent) {
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
fn shown(settings: &Value, standard: &Value) -> Value {
    let mut entry = json!({"id": standard["id"], "level": level(settings, standard)});
    for kind in SOURCES {
        if let Some(value) = standard.get(kind) {
            entry[kind] = value.clone();
        }
    }
    let anchor = anchor(settings, standard);
    let summary = standard["summary"]
        .as_str()
        .or_else(|| anchor.as_ref().and_then(|anchor| anchor.summary.as_deref()));
    if let Some(summary) = summary {
        entry["summary"] = json!(summary);
    }
    for field in ["run", "checklist"] {
        if let Some(value) = standard.get(field) {
            entry[field] = value.clone();
        }
    }
    if let Some(anchor) = anchor.filter(|anchor| !anchor.checklist.is_empty())
        && standard.get("checklist").is_none()
    {
        entry["checklist"] = json!(anchor.checklist);
    }
    entry
}

/// The declared standards a route's work falls under, in declaration order, then Craft's
/// defaults for the kind of work when the project turns them on.
pub fn select(settings: &Value, work: &Work) -> Vec<Value> {
    let mut selected: Vec<Value> = declared(settings)
        .iter()
        .filter(|standard| matches(settings, standard, work))
        .map(|standard| shown(settings, standard))
        .collect();
    if settings["anchors"]["defaults"] == true {
        selected.extend(defaults(settings, work.intent));
    }
    selected
}

/// Craft's anchors for `intent`, advised and never checked. The project's own word wins: a
/// declared standard for this kind of work replaces them all, and an anchor the project
/// disables or already names is not added. A project catalog entry with the id supplies its
/// summary.
fn defaults(settings: &Value, intent: &str) -> Vec<Value> {
    let declared = declared(settings);
    if declared
        .iter()
        .any(|standard| intents(settings, standard).iter().any(|i| i == intent))
    {
        return Vec::new();
    }
    let excluded: Vec<&str> = settings["anchors"]["disables"]
        .as_array()
        .into_iter()
        .flatten()
        .chain(declared.iter().map(|standard| &standard["anchor"]))
        .filter_map(Value::as_str)
        .map(anchors::canonical)
        .collect();
    anchors::anchors()
        .unwrap_or_default()
        .iter()
        .filter(|anchor| anchor.defaults.iter().any(|i| i == intent))
        .filter(|anchor| !excluded.contains(&anchor.id.as_str()))
        .filter_map(|anchor| anchors::resolve(settings, &anchor.id))
        .filter(|anchor| anchor.tier != Tier::Refused)
        .map(|anchor| {
            let mut entry = json!({
                "id": anchor.id,
                "level": "advise",
                "anchor": anchor.id,
                "default": true,
            });
            if let Some(summary) = anchor.summary {
                entry["summary"] = json!(summary);
            }
            entry
        })
        .collect()
}

/// Whether the project keeps its anchor checks on: on unless `routing.standardChecks` is false.
pub fn checks_enabled(settings: &Value) -> bool {
    settings["routing"]["standardChecks"] != false
}

/// Whether `file` is one an anchor reads by default: a named file anywhere, or an extension.
fn default_target(anchor: &Anchor, file: &str) -> bool {
    let name = file.rsplit('/').next().unwrap_or(file);
    anchor.targets.iter().any(|target| {
        if target.starts_with('.') {
            name.ends_with(target.as_str())
        } else {
            name == target
        }
    })
}

/// One anchor check a standard asks for.
#[derive(Debug, PartialEq, Eq)]
pub struct Check<'a> {
    pub standard: &'a str,
    /// The anchor as declared.
    pub anchor: &'a str,
    /// The deterministic check it runs.
    pub check: String,
}

/// The checked standards whose anchor has a check reading `input`, with the anchor.
fn checked(settings: &Value, input: Input) -> Vec<(&Value, Anchor)> {
    if !checks_enabled(settings) {
        return Vec::new();
    }
    declared(settings)
        .iter()
        .filter(|standard| level(settings, standard) == "check")
        .filter_map(|standard| {
            let anchor = anchor(settings, standard)?;
            (anchor.input() == Some(input)).then_some((standard, anchor))
        })
        .collect()
}

fn planned<'a>(standard: &'a Value, anchor: Anchor) -> Check<'a> {
    Check {
        standard: standard["id"].as_str().unwrap_or_default(),
        anchor: standard["anchor"].as_str().unwrap_or_default(),
        check: anchor.check.unwrap_or_default(),
    }
}

/// The anchor checks that read `file` after it is written. A declared `scope.paths`
/// replaces the anchor's default targets.
pub fn file_checks<'a>(settings: &'a Value, file: &str) -> Vec<Check<'a>> {
    checked(settings, Input::File)
        .into_iter()
        .filter(|(standard, anchor)| {
            let paths = listed(standard, "paths");
            if paths.is_empty() {
                default_target(anchor, file)
            } else {
                paths.iter().any(|path| member(file, &prefix(path)))
            }
        })
        .map(|(standard, anchor)| planned(standard, anchor))
        .collect()
}

/// The module-dependency checks for a source file just written: standards whose anchor
/// reads imports, narrowed by a declared `scope.paths`. Whether the file's module is checked
/// at all is decided with the imports, in `dependency_verdicts`.
pub fn import_checks<'a>(settings: &'a Value, file: &str) -> Vec<Check<'a>> {
    checked(settings, Input::Imports)
        .into_iter()
        .filter(|(standard, _)| {
            let paths = listed(standard, "paths");
            paths.is_empty() || paths.iter().any(|path| member(file, &prefix(path)))
        })
        .map(|(standard, anchor)| planned(standard, anchor))
        .collect()
}

/// One import a written source file adds, as the application resolved it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Import {
    /// 1-based.
    pub line: usize,
    /// The import as written.
    pub target: String,
    /// The project-relative file or directory it names; `None` when it is not resolvable
    /// (a third-party package, an alias, an unsupported language), which is never a finding.
    pub resolved: Option<String>,
}

/// The one declared module that owns `path`, or `None` when none or several (equally long
/// paths) do.
fn owner(settings: &Value, path: &str) -> Option<Value> {
    let mut owners = crate::routing::matching_modules(settings, &[path.to_owned()]);
    (owners.len() == 1).then(|| owners.remove(0))
}

/// Whether `file` lies in exactly one declared module that declares `dependsOn`, so the
/// imports it adds are worth resolving.
pub fn guards_dependencies(settings: &Value, file: &str) -> bool {
    owner(settings, file).is_some_and(|module| module["dependsOn"].is_array())
}

/// Results of `checks` over the imports `file` adds: an import whose resolved path lies in
/// another declared module that the file's module does not list in `dependsOn`. A module
/// without an explicit `dependsOn` is not checked, so partial declarations stay quiet.
pub fn dependency_verdicts(
    settings: &Value,
    checks: &[Check],
    file: &str,
    imports: &[Import],
) -> Vec<Value> {
    let Some(module) = owner(settings, file) else {
        return Vec::new();
    };
    let Some(allowed) = module.get("dependsOn").and_then(Value::as_array) else {
        return Vec::new();
    };
    let name = module["name"].as_str().unwrap_or_default();
    let mut findings = Vec::new();
    for import in imports {
        let Some(target) = import
            .resolved
            .as_deref()
            .and_then(|path| owner(settings, path))
        else {
            continue;
        };
        let other = target["name"].as_str().unwrap_or_default();
        if other == name || allowed.iter().any(|item| item == other) {
            continue;
        }
        findings.push(json!({
            "rule": "undeclared-dependency",
            "line": import.line,
            "message": format!(
                "imports {} from module {other}; module {name} does not declare dependsOn {other}",
                import.target
            ),
        }));
    }
    if findings.is_empty() {
        return Vec::new();
    }
    checks
        .iter()
        .map(|planned| {
            json!({
                "standard": planned.standard,
                "anchor": planned.anchor,
                "findings": findings,
            })
        })
        .collect()
}

/// The anchor checks for a commit message.
pub fn commit_checks(settings: &Value) -> Vec<Check<'_>> {
    checked(settings, Input::CommitMessage)
        .into_iter()
        .map(|(standard, anchor)| planned(standard, anchor))
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
pub fn verdicts(checks: &[Check], text: &str, path: Option<&str>) -> Vec<Value> {
    checks
        .iter()
        .filter_map(|planned| {
            let findings = anchors::checks::check(&planned.check, text, path);
            (!findings.is_empty()).then(|| {
                json!({
                    "standard": planned.standard,
                    "anchor": planned.anchor,
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
