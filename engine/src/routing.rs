//! Routing over validated effective settings: which declared modules own a set of
//! project-relative files, and which guidance, execution and operations a request needs.
//! Declarations are not verified against the actual project; nothing here runs work.
use crate::contracts::relative_file;
use crate::{models, services};
use serde_json::{Map, Value, json};

pub mod catalog;
mod loading;

type Result<T> = std::result::Result<T, String>;

const DECLARED: &str = "declarations-only; verify paths and checks in the actual project";
const PRIVATE: &str = "private workspace excluded";

/// Private state and Git internals, which no lookup, route or review may select.
pub fn private_path(path: &str) -> bool {
    let parts: Vec<&str> = path.split('/').collect();
    parts.contains(&".git")
        || parts
            .windows(2)
            .any(|pair| [".craft", ".capabilities"].contains(&pair[0]) && pair[1] == "state")
}

fn member(file: &str, directory: &str) -> bool {
    directory == "."
        || file == directory
        || file
            .strip_prefix(directory)
            .is_some_and(|rest| rest.starts_with('/'))
}

fn modules(settings: &Value) -> &[Value] {
    settings
        .get("modules")
        .and_then(Value::as_array)
        .map_or(&[], Vec::as_slice)
}

/// A declared module with its path normalized; settings are validated, so the path is too.
fn normalized(module: &Value) -> Value {
    let mut module = module.clone();
    module["path"] = json!(relative_file(&module["path"]).expect("validated module path"));
    module
}

/// Modules owning each file: the longest matching declared paths, in file then declaration
/// order, each module once.
pub fn matching_modules(settings: &Value, files: &[String]) -> Vec<Value> {
    let declared: Vec<Value> = modules(settings).iter().map(normalized).collect();
    let mut selected: Vec<Value> = Vec::new();
    for file in files {
        let matches: Vec<&Value> = declared
            .iter()
            .filter(|module| member(file, module["path"].as_str().unwrap_or_default()))
            .collect();
        let longest = matches
            .iter()
            .map(|module| module["path"].as_str().unwrap_or_default().len())
            .max();
        for module in matches {
            if Some(module["path"].as_str().unwrap_or_default().len()) == longest
                && !selected.iter().any(|item| item["name"] == module["name"])
            {
                selected.push(module.clone());
            }
        }
    }
    selected
}

/// Declared modules for `files`, for one `module` by name, or all of them. Private files
/// are excluded; when every given file is private, nothing is selected.
pub fn lookup(settings: &Value, files: &[Value], module: Option<&str>) -> Result<Value> {
    let result = |modules: Vec<Value>, availability: &str| json!({ "modules": modules, "availability": availability, "execution": "deterministic-lookup" });
    if let Some(name) = module {
        if !files.is_empty() {
            return Err("choose --file or --module for lookup".into());
        }
        let selected: Vec<Value> = modules(settings)
            .iter()
            .filter(|entry| entry["name"] == name)
            .map(normalized)
            .collect();
        if selected.is_empty() {
            return Err(format!("unknown module: {name}"));
        }
        return Ok(result(selected, DECLARED));
    }
    if files.is_empty() {
        return Ok(result(
            modules(settings).iter().map(normalized).collect(),
            DECLARED,
        ));
    }
    let files = files
        .iter()
        .map(relative_file)
        .collect::<Result<Vec<_>>>()?;
    let public: Vec<String> = files
        .into_iter()
        .filter(|file| !private_path(file))
        .collect();
    if public.is_empty() {
        return Ok(result(Vec::new(), PRIVATE));
    }
    Ok(result(matching_modules(settings, &public), DECLARED))
}

const STRONG_RISKS: [&str; 5] = [
    "architecture-ambiguity",
    "ambiguous-debugging",
    "distributed-consistency",
    "security-sensitive",
    "complex-cross-module",
];
const ECONOMICAL: [&str; 10] = [
    "locate",
    "lookup",
    "discover",
    "context-report",
    "status",
    "metadata",
    "format",
    "extract",
    "run-check",
    "operation",
];
const VERIFICATION_MODES: [&str; 3] = ["manual", "automation", "test-code"];
const NO_CHECKS: &str =
    "No checks declared for the affected modules; choose verification before planning execution.";
const LOCATE: &str = "Use bounded repository search before asking an LLM to find names.";
const NO_WORKERS: &str = "Delegation is disabled by the effective zero worker limit.";
const NOT_DELEGATED: &str =
    "Delegation was not selected: the role and host must both explicitly support it.";

/// A capability class suggestion: a task heuristic, never a vendor ranking, price or model switch.
fn capability(operation: &str, risks: &[String]) -> Value {
    let strong = operation == "architecture"
        || risks
            .iter()
            .any(|risk| STRONG_RISKS.contains(&risk.as_str()));
    let economical = ECONOMICAL.contains(&operation);
    let (class, reason) = if strong {
        (
            "strong",
            "The declared operation requires architecture or difficult cross-boundary reasoning.",
        )
    } else if economical {
        (
            "economical",
            "Bounded mechanical evidence gathering can use deterministic tools or an economical profile.",
        )
    } else {
        (
            "standard",
            "Routine bounded engineering work normally starts with the configured standard profile.",
        )
    };
    json!({
        "class": class,
        "execution": if economical && !strong { "prefer-deterministic-tool" } else { "recommendation-only" },
        "reason": reason,
        "pricing": "unknown; classes are not measured cost or capability",
        "authorization": "no model switch or delegation executed",
    })
}

fn role(intent: &str) -> Option<&'static str> {
    Some(match intent {
        "implement" => "implementer",
        "plan" => "planner",
        "investigate" | "research" => "researcher",
        "review" => "reviewer",
        "locate" => "locator",
        "verify" | "explore" => "verifier",
        _ => return None,
    })
}

/// Append `value` unless present, as a JavaScript `Set` does.
fn add(items: &mut Vec<Value>, value: &Value) {
    if !items.contains(value) {
        items.push(value.clone());
    }
}

fn list<'a>(value: &'a Value, field: &str) -> &'a [Value] {
    value
        .get(field)
        .and_then(Value::as_array)
        .map_or(&[], Vec::as_slice)
}

pub struct Request<'a> {
    pub intent: Option<&'a str>,
    pub files: &'a [Value],
    pub risks: &'a [String],
    pub host: &'a str,
    pub delegate: bool,
    /// A task to read through the configured task service.
    pub task: Option<&'a str>,
    pub verification_mode: Option<&'a str>,
}

/// The routing plan for one request: guidance to load, execution, checks and operations.
pub fn route(settings: &Value, request: &Request) -> Result<Value> {
    let catalog = catalog::catalog()?;
    let intent = request
        .intent
        .filter(|intent| catalog::lists(catalog, "intents", intent))
        .ok_or_else(|| format!("unknown intent: {}", request.intent.unwrap_or("undefined")))?;
    if let Some(mode) = request.verification_mode
        && (intent != "verify" || !VERIFICATION_MODES.contains(&mode))
    {
        return Err(
            "verificationMode requires verify intent and manual, automation or test-code".into(),
        );
    }
    let mut changed: Vec<String> = Vec::new();
    for file in request.files {
        let file = relative_file(file)?;
        if !changed.contains(&file) {
            changed.push(file);
        }
    }
    changed.retain(|file| !private_path(file));
    let modules = matching_modules(settings, &changed);
    let mut languages = Vec::new();
    for file in &changed {
        let extension = loading::extension(file);
        let legacy = [".java", ".kt", ".kts"].contains(&extension);
        let language = if legacy {
            &json!("jvm")
        } else {
            &catalog["extensions"][extension]
        };
        if language.is_string() {
            add(&mut languages, language);
        }
    }
    let mut layers = Vec::new();
    for module in &modules {
        list(module, "layers")
            .iter()
            .for_each(|layer| add(&mut layers, layer));
    }
    if modules.is_empty() {
        list(settings, "layers")
            .iter()
            .for_each(|layer| add(&mut layers, layer));
    }
    if changed.is_empty() {
        list(settings, "languages")
            .iter()
            .for_each(|language| add(&mut languages, language));
        list(settings, "layers")
            .iter()
            .for_each(|layer| add(&mut layers, layer));
    }
    for module in &modules {
        list(module, "languages")
            .iter()
            .for_each(|language| add(&mut languages, language));
    }
    for file in &changed {
        let layer = &catalog["layerExtensions"][loading::extension(file)];
        if layer.is_string() {
            add(&mut layers, layer);
        }
    }
    let loading = loading::plan(
        &loading::Request {
            intent,
            files: &changed,
            modules: &modules,
            settings,
            risks: request.risks,
            verification_mode: request.verification_mode,
        },
        catalog,
    );
    let selections = list(&loading, "selections");
    let mut skills = Vec::new();
    let mut references = Vec::new();
    for entry in selections {
        list(entry, "skills")
            .iter()
            .for_each(|skill| add(&mut skills, skill));
        list(entry, "references")
            .iter()
            .for_each(|reference| add(&mut references, reference));
    }
    let mut reasons: Vec<Value> = selections
        .iter()
        .map(|entry| json!({ "id": entry["id"], "selectors": entry["reason"], "evidence": entry["evidence"] }))
        .collect();
    if catalog::lists(catalog, "stackIntents", intent) && intent != "verify" {
        for module in &modules {
            list(module, "skills")
                .iter()
                .for_each(|skill| add(&mut skills, skill));
        }
    }
    let mut operations = Vec::new();
    let mut missing: Vec<Value> = Vec::new();
    if intent == "status" {
        operations.push(services::plan(settings, "git.status", &json!({}))?);
    }
    if let Some(task) = request.task.filter(|task| !task.is_empty()) {
        match services::plan(settings, "task.read", &json!({ "id": task })) {
            Ok(operation) => operations.push(operation),
            Err(error) => missing.push(json!(error)),
        }
    }
    let role = role(intent);
    let checks: Vec<Value> = modules
        .iter()
        .flat_map(|module| {
            list(module, "checks").iter().map(|check| {
                let mut entry = Map::from_iter([
                    ("module".to_string(), module["name"].clone()),
                    ("cwd".to_string(), module["path"].clone()),
                ]);
                entry.extend(check.as_object().cloned().unwrap_or_default());
                entry.insert("execution".into(), json!("not-run"));
                Value::Object(entry)
            })
        })
        .collect();
    let tool =
        intent == "locate" || intent == "status" || (intent == "run-check" && !checks.is_empty());
    let mut execution = json!({ "mode": if tool { "tool" } else { "main" }, "role": role.unwrap_or("coordinator") });
    if intent == "run-check" && checks.is_empty() {
        missing.push(json!(NO_CHECKS));
    }
    if intent == "locate" {
        reasons.push(json!(LOCATE));
    }
    if request.delegate {
        let rule = role.and_then(|role| settings.pointer(&format!("/execution/roles/{role}")));
        let host_rule = settings
            .get("execution")
            .and_then(|e| e.get("hosts"))
            .and_then(|h| h.get(request.host));
        // `profile` is optional; an empty name selects no model, as JavaScript truthiness does.
        let profile = rule
            .and_then(|rule| rule["profile"].as_str())
            .filter(|profile| !profile.is_empty());
        let workers = settings.pointer("/efficiency/delegation/maxWorkers");
        if workers.and_then(Value::as_f64) == Some(0.0) {
            reasons.push(json!(NO_WORKERS));
        } else if rule.is_some_and(|rule| rule["mode"] == "delegate")
            && host_rule.is_some_and(|host| host["delegation"] == true)
        {
            let model = profile.and_then(|profile| host_rule?.get("models")?.get(profile));
            match (profile, model) {
                (Some(profile), None) => missing.push(json!(format!(
                    "host {} has no model mapping for profile {profile}",
                    request.host
                ))),
                _ => {
                    let role = role.expect("a delegated rule names a role");
                    if let Some(delegated) =
                        models::delegation(settings, role, request.host, profile)
                    {
                        execution = delegated;
                    }
                }
            }
        } else {
            reasons.push(json!(NOT_DELEGATED));
        }
    }
    let declared = list(settings, "modules");
    let mut impacted: Vec<&Value> = modules.iter().map(|module| &module["name"]).collect();
    let selected = impacted.len();
    let mut progress = true;
    while progress {
        progress = false;
        for module in declared {
            let depends = list(module, "dependsOn")
                .iter()
                .any(|id| impacted.contains(&id));
            if !impacted.contains(&&module["name"]) && depends {
                impacted.push(&module["name"]);
                progress = true;
            }
        }
    }
    let mut plan = json!({
        "intent": intent,
        "capability": capability(intent, request.risks),
        "skills": skills,
        "skillAvailability": "not-verified",
        "execution": execution,
    });
    if intent == "verify" {
        plan["taskBoundary"] = json!({
            "kind": "testing-only",
            "mode": request.verification_mode.unwrap_or("choose-from-request"),
            "testEdits": if request.verification_mode == Some("test-code") { "requested-test-scope-only" } else { "not-selected" },
            "productEdits": "not-selected",
            "enforcement": "routing contract; host permissions and original scope remain authoritative",
        });
    }
    let summaries: Vec<Value> = modules
        .iter()
        .map(|module| {
            let mut summary = json!({ "name": module["name"], "path": module["path"] });
            if let Some(owner) = module
                .get("owner")
                .filter(|owner| owner.as_str().is_some_and(|o| !o.is_empty()))
            {
                summary["owner"] = owner.clone();
            }
            summary
        })
        .collect();
    let map = plan.as_object_mut().expect("plan object");
    map.insert("files".into(), json!(changed));
    map.insert("languages".into(), Value::Array(languages));
    map.insert("layers".into(), Value::Array(layers));
    map.insert("references".into(), Value::Array(references));
    map.insert("loading".into(), loading);
    map.insert("modules".into(), Value::Array(summaries));
    map.insert("affectedDependents".into(), json!(impacted[selected..]));
    map.insert("checks".into(), Value::Array(checks));
    map.insert("operations".into(), Value::Array(operations));
    map.insert("missing".into(), Value::Array(missing));
    map.insert("reasons".into(), Value::Array(reasons));
    map.insert(
        "authorization".into(),
        json!("route-only; host permissions and original user scope still apply"),
    );
    Ok(plan)
}

#[cfg(test)]
mod tests;
