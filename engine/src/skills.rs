//! Consumer skills in routes: a project declares which of its own skills replaces a package
//! skill or serves a scope, and which package skills it does not want. Hosts already list
//! every project skill; Craft decides only what a listing cannot — which skill a route
//! recommends — and never loads, injects or hides a skill itself.
use crate::contracts::{kebab, relative_file};
use crate::standards::{self, Work};
use crate::values::object;
use serde_json::{Value, json};
use std::sync::OnceLock;

type Result<T = ()> = std::result::Result<T, String>;

/// The source a route names consumer skills by: `project:<name>`.
pub const SOURCE: &str = "project";
const MAX_DECLARED: usize = 32;
/// The front matter Craft reads from a declared SKILL.md; the rest is the host's to load.
pub const MAX_HEADER: usize = 4096;
const HOSTS: &str = include_str!("../../hosts/hooks.json");
const STILL_LISTED: &str = "disabled in routing; the host may still list it";

fn hosts() -> &'static Value {
    static TABLE: OnceLock<Value> = OnceLock::new();
    TABLE.get_or_init(|| serde_json::from_str(HOSTS).expect("embedded host hook table"))
}

fn strings(value: &Value) -> impl Iterator<Item = &str> {
    value
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
}

/// The project skill folders `host` reads at the repository root.
pub fn folders(host: &str) -> Vec<&'static str> {
    strings(&hosts()[host]["listing"]["projectSkills"]).collect()
}

/// Every project skill folder some host reads.
fn known_folders() -> Vec<&'static str> {
    let mut all: Vec<&str> = Vec::new();
    for (_, host) in hosts().as_object().into_iter().flatten() {
        for folder in strings(&host["listing"]["projectSkills"]) {
            if !all.contains(&folder) {
                all.push(folder);
            }
        }
    }
    all
}

/// The folder a declared skill path lives in: `<folder>/<dir>` for a known folder.
fn folder(path: &str) -> Option<&'static str> {
    known_folders().into_iter().find(|folder| {
        path.strip_prefix(folder)
            .and_then(|rest| rest.strip_prefix('/'))
            .is_some_and(|dir| !dir.is_empty() && !dir.contains('/'))
    })
}

/// The same skill folder under another host's project skill folder: `.agents/skills/x` is
/// mirrored at `.claude/skills/x`.
fn mirror(path: &str, folder: &str) -> Option<String> {
    let own = self::folder(path)?;
    let dir = &path[own.len() + 1..];
    Some(format!("{folder}/{dir}"))
}

/// The SKILL.md files the project declares, for the application to read: each declared one,
/// then its mirrors in the other project skill folders hosts read.
pub fn declared_files(project: &Value) -> Vec<String> {
    let declared: Vec<String> = project["skills"]["declared"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|skill| relative_file(&skill["path"]).ok())
        .collect();
    let mut files: Vec<String> = declared.iter().map(|p| format!("{p}/SKILL.md")).collect();
    for path in &declared {
        for folder in known_folders() {
            if let Some(file) = mirror(path, folder).map(|m| format!("{m}/SKILL.md"))
                && !files.contains(&file)
            {
                files.push(file);
            }
        }
    }
    files
}

/// The `name` in a SKILL.md front matter, the identity hosts list the skill by.
pub fn front_matter_name(text: &str) -> Option<String> {
    let mut lines = text.lines();
    if lines.next()?.trim_end() != "---" {
        return None;
    }
    for line in lines {
        let line = line.trim_end();
        if line == "---" {
            return None;
        }
        if let Some(value) = line.strip_prefix("name:") {
            let value = value.trim().trim_matches(|c| c == '"' || c == '\'');
            return Some(value.to_string()).filter(|name| !name.is_empty());
        }
    }
    None
}

/// Facts about each declared skill: `(SKILL.md path, its text if readable)`, recorded as
/// `skills.found`: a readable file maps to its front matter name or null, an unreadable
/// one is absent.
pub fn found(files: &[(String, Option<String>)]) -> Value {
    let map = files
        .iter()
        .filter_map(|(path, text)| {
            let text = text.as_deref()?;
            Some((path.clone(), json!(front_matter_name(text))))
        })
        .collect();
    Value::Object(map)
}

fn package_skill(name: &str, known: &[&str], label: &str) -> Result {
    if known.contains(&name) {
        Ok(())
    } else {
        Err(format!("{label}: unknown package skill {name}"))
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

/// Validate `skills` against the package skills routes can select, the declared modules and
/// the routing intents. Conflicts are errors: one package skill replaced twice, or replaced
/// and disabled.
pub fn validate(skills: &Value, known: &[&str], modules: &[&str], intents: &[&str]) -> Result {
    let skills = object(skills, &["declared", "disables"], "skills")?;
    let disables = names(skills.get("disables"), "skills.disables")?;
    for name in &disables {
        package_skill(name, known, "skills.disables")?;
    }
    let declared = match skills.get("declared") {
        None => &[][..],
        Some(Value::Array(list)) if list.len() <= MAX_DECLARED => list.as_slice(),
        Some(_) => {
            return Err(format!(
                "skills.declared must be an array of at most {MAX_DECLARED}"
            ));
        }
    };
    let mut seen: Vec<&str> = Vec::new();
    let mut replaced: Vec<(&str, &str)> = Vec::new();
    for entry in declared {
        let skill = object(entry, &["name", "path", "replaces", "scope"], "skill")?;
        let name = skill
            .get("name")
            .and_then(Value::as_str)
            .filter(|name| kebab(name) && name.len() <= 64)
            .ok_or("skill name must be kebab-case")?;
        if seen.contains(&name) {
            return Err(format!("duplicate skill name: {name}"));
        }
        seen.push(name);
        let path = relative_file(skill.get("path").unwrap_or(&Value::Null))
            .map_err(|error| format!("skill {name}: {error}"))?;
        if folder(&path).is_none() {
            return Err(format!(
                "skill {name}: path must be <folder>/<skill> in one of {}",
                known_folders().join(", ")
            ));
        }
        for target in names(skill.get("replaces"), "replaces")? {
            package_skill(target, known, &format!("skill {name} replaces"))?;
            if disables.contains(&target) {
                return Err(format!(
                    "skill {name} replaces {target}, which skills.disables also names"
                ));
            }
            if let Some((other, _)) = replaced.iter().find(|(_, t)| *t == target) {
                return Err(format!(
                    "skills {other} and {name} both replace {target}; declare one"
                ));
            }
            replaced.push((name, target));
        }
        match skill.get("scope") {
            Some(scope) => {
                standards::validate_scope(scope, &format!("skill {name}"), modules, intents)?
            }
            None if !skill.contains_key("replaces") => {
                return Err(format!(
                    "skill {name} needs replaces or scope; otherwise no route selects it"
                ));
            }
            None => {}
        }
    }
    Ok(())
}

/// The folder `host` reads `skill` from: the declared one, or a copy under the same name in
/// a folder the host reads; otherwise why the skill cannot be recommended there.
fn served(settings: &Value, skill: &Value, host: &str) -> std::result::Result<String, String> {
    let name = skill["name"].as_str().unwrap_or_default();
    let path = skill["path"].as_str().unwrap_or_default();
    let file = format!("{path}/SKILL.md");
    let found = &settings["skills"]["found"][&file];
    if !found.is_string() {
        return Err(if settings["skills"]["found"].get(&file).is_some() {
            format!("{file} has no front matter name")
        } else {
            format!("{file} not found")
        });
    }
    if found != name {
        return Err(format!(
            "{file} names the skill {}, not {name}",
            found.as_str().unwrap_or_default()
        ));
    }
    let folder = folder(path).unwrap_or_default();
    let readable = folders(host);
    if readable.contains(&folder) {
        return Ok(path.to_string());
    }
    if let Some(copy) = readable
        .iter()
        .filter_map(|other| mirror(path, other))
        .find(|copy| settings["skills"]["found"][format!("{copy}/SKILL.md")] == name)
    {
        return Ok(copy);
    }
    Err(format!(
        "{host} does not read {folder}; mirror the skill to {}",
        readable
            .first()
            .and_then(|other| mirror(path, other))
            .unwrap_or_else(|| "a folder it reads".into())
    ))
}

/// Apply the project's skill declarations to the package skills a route selected: declared
/// replacements and scoped skills join, disabled package skills leave. Returns the
/// attribution of every decision taken, and what could not be recommended on `host`.
pub fn apply(
    settings: &Value,
    host: &str,
    work: &Work,
    skills: &mut Vec<Value>,
) -> (Vec<Value>, Vec<Value>) {
    let (mut sources, mut missing) = (Vec::new(), Vec::new());
    let declared = settings["skills"]["declared"]
        .as_array()
        .map_or(&[][..], Vec::as_slice);
    for skill in declared {
        let replaces: Vec<&str> = strings(&skill["replaces"]).collect();
        let selected: Vec<&str> = replaces
            .iter()
            .copied()
            .filter(|target| skills.iter().any(|s| s == target))
            .collect();
        let scoped = skill.get("scope").is_some();
        if (scoped && !standards::matches(settings, skill, work))
            || (!scoped && selected.is_empty())
            || (scoped && !replaces.is_empty() && selected.is_empty())
        {
            continue;
        }
        let id = format!("{SOURCE}:{}", skill["name"].as_str().unwrap_or_default());
        let path = match served(settings, skill, host) {
            Ok(path) => path,
            Err(reason) => {
                let kept = if selected.is_empty() {
                    String::new()
                } else {
                    format!("; routed {} instead", selected.join(", "))
                };
                missing.push(json!(format!("{id} unavailable: {reason}{kept}")));
                sources.push(json!({"skill": id, "source": SOURCE, "path": skill["path"], "status": "unavailable", "reason": reason}));
                continue;
            }
        };
        let position = skills.iter().position(|s| selected.iter().any(|t| s == t));
        skills.retain(|s| !selected.iter().any(|t| s == t));
        let entry = json!(id);
        if !skills.contains(&entry) {
            match position {
                Some(index) => skills.insert(index, entry),
                None => skills.push(entry),
            }
        }
        let mut source = json!({"skill": id, "source": SOURCE, "path": path});
        if !selected.is_empty() {
            source["replaces"] = json!(selected);
        }
        sources.push(source);
    }
    for disabled in strings(&settings["skills"]["disables"]) {
        let before = skills.len();
        skills.retain(|s| s != disabled);
        if skills.len() != before {
            sources.push(json!({"skill": disabled, "source": "package", "status": "disabled", "host": STILL_LISTED}));
        }
    }
    (sources, missing)
}

/// Keep the package skills and references whose package the host has installed; a route
/// must not name a skill the host cannot load. Consumer skills stay: the project serves them.
/// Returns the attribution of every package skill left out.
pub fn installed(
    packages: &[String],
    skills: &mut Vec<Value>,
    references: &mut Vec<Value>,
) -> Vec<Value> {
    let present = |target: &Value| {
        target.as_str().is_none_or(|text| {
            consumer(text)
                || text
                    .split_once(':')
                    .is_none_or(|(package, _)| packages.iter().any(|p| p == package))
        })
    };
    let sources = skills
        .iter()
        .filter(|skill| !present(skill))
        .map(|skill| json!({"skill": skill, "source": "package", "status": "not-installed"}))
        .collect();
    skills.retain(present);
    references.retain(present);
    sources
}

/// The name `host` lists a route skill by: consumer skills by their bare name, package
/// skills as `package:skill` on plugin hosts and `package-skill` elsewhere.
pub fn host_name(skill: &str, plugin_names: bool) -> String {
    match skill
        .strip_prefix(SOURCE)
        .and_then(|rest| rest.strip_prefix(':'))
    {
        Some(name) => name.to_string(),
        None if plugin_names => skill.to_string(),
        None => skill.replacen(':', "-", 1),
    }
}

/// Whether a route skill is the consumer's rather than a package's.
pub fn consumer(skill: &str) -> bool {
    skill
        .strip_prefix(SOURCE)
        .is_some_and(|rest| rest.starts_with(':'))
}

#[cfg(test)]
mod tests;
