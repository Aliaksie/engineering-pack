//! Which catalog guidance a route loads. Each affected module is matched independently, so a
//! provider or framework declared by one module cannot borrow a layer declared by another.
use super::catalog::{entries, lists};
use super::member;
use serde_json::{Map, Value, json};

const DECLARED: [&str; 5] = ["languages", "runtimes", "frameworks", "providers", "layers"];
const SPECIALIZATIONS: [&str; 5] = ["language:", "runtime:", "framework:", "provider:", "layer:"];
const NO_RUNTIME: &str = "JavaScript/TypeScript does not establish Node or browser.";
const NO_PATHS: &str =
    "Affected paths are unspecified; project declarations do not establish the affected scope.";
const POLICY: &str = "Declarations and extension inference require repository verification. Existing behavior is evidence, not accepted policy; surface conflicts with project instructions.";

pub struct Request<'a> {
    pub intent: &'a str,
    pub files: &'a [String],
    /// Selected modules, paths normalized.
    pub modules: &'a [Value],
    pub settings: &'a Value,
    pub risks: &'a [String],
    pub verification_mode: Option<&'a str>,
}

/// The file extension, without a leading-dot file name counting as one.
pub(super) fn extension(file: &str) -> &str {
    let name = file.rsplit('/').next().unwrap_or(file);
    match name.rfind('.') {
        Some(dot) if dot > 0 => &name[dot..],
        _ => "",
    }
}

fn path(module: &Value) -> &str {
    module["path"].as_str().unwrap_or_default()
}

fn names(value: Option<&Value>) -> Vec<Value> {
    value.and_then(Value::as_array).cloned().unwrap_or_default()
}

fn evidence(dimension: &str, values: Value, source: &str, inferred: bool) -> Value {
    json!({ "dimension": dimension, "values": values, "source": source, "inferred": inferred })
}

/// Declared and inferred stack values of one scope, with the evidence for each.
struct Context {
    paths: Vec<String>,
    values: Map<String, Value>,
    evidence: Vec<Value>,
}

impl Context {
    fn has(&self, dimension: &str, value: &str) -> bool {
        self.values[dimension]
            .as_array()
            .is_some_and(|items| items.iter().any(|item| item == value))
    }

    fn push(&mut self, dimension: &str, value: &str) {
        self.values[dimension]
            .as_array_mut()
            .expect("dimension list")
            .push(json!(value));
    }
}

fn context(
    request: &Request,
    catalog: &Value,
    module: Option<&Value>,
    paths: Vec<String>,
    missing: &mut Vec<String>,
    stack: bool,
) -> Context {
    let project = request.settings;
    let mut context = Context {
        paths,
        values: Map::new(),
        evidence: vec![
            evidence("intents", json!([request.intent]), "request", false),
            evidence("risks", json!(request.risks), "request", false),
        ],
    };
    for dimension in DECLARED {
        let own = module.and_then(|module| module.get(dimension));
        let values = names(own.or_else(|| project.get(dimension)));
        if !values.is_empty() {
            let source = match (own, module) {
                (Some(_), Some(module)) => {
                    format!("module:{}", module["name"].as_str().unwrap_or_default())
                }
                _ => "project".into(),
            };
            context
                .evidence
                .push(evidence(dimension, json!(values), &source, false));
        }
        context
            .values
            .insert(dimension.into(), Value::Array(values));
    }
    for file in context.paths.clone() {
        let extension = extension(&file);
        for (dimension, mapping) in [("languages", "extensions"), ("layers", "layerExtensions")] {
            if let Some(value) = catalog[mapping][extension].as_str() {
                context.push(dimension, value);
                context
                    .evidence
                    .push(evidence(dimension, json!([value]), &file, true));
            }
        }
    }
    let languages: Vec<String> = names(context.values.get("languages"))
        .iter()
        .filter_map(|language| language.as_str().map(str::to_string))
        .collect();
    for language in &languages {
        if let Some(runtime) = catalog["runtimeImplications"][language].as_str()
            && !context.has("runtimes", runtime)
        {
            context.push("runtimes", runtime);
            context.evidence.push(evidence(
                "runtimes",
                json!([runtime]),
                &format!("language:{language}"),
                true,
            ));
        }
    }
    let scripted = languages
        .iter()
        .any(|language| language == "javascript" || language == "typescript");
    if stack && scripted && names(context.values.get("runtimes")).is_empty() {
        let name = module.map_or("project", |module| {
            module["name"].as_str().unwrap_or_default()
        });
        missing.push(format!("Runtime is unspecified for {name}; {NO_RUNTIME}"));
    }
    context
}

/// Whether one selector dimension matches a context.
fn matches(context: &Context, request: &Request, dimension: &str, choices: &Value) -> bool {
    choices.as_array().into_iter().flatten().any(|choice| {
        let Some(choice) = choice.as_str() else {
            return false;
        };
        match dimension {
            "paths" => context.paths.iter().any(|file| member(file, choice)),
            "intents" => request.intent == choice,
            "risks" => request.risks.iter().any(|risk| risk == choice),
            _ => context.has(dimension, choice),
        }
    })
}

fn selection(entry: &Value, matched: &[&Context]) -> Value {
    let when = entry["when"].as_object().expect("validated selector");
    let mut evidence = Vec::new();
    for context in matched {
        evidence.extend(
            context
                .evidence
                .iter()
                .filter(|item| when.contains_key(item["dimension"].as_str().unwrap_or_default()))
                .cloned(),
        );
        if let Some(paths) = when.get("paths").and_then(Value::as_array) {
            let files: Vec<&String> = context
                .paths
                .iter()
                .filter(|file| {
                    paths
                        .iter()
                        .any(|path| member(file, path.as_str().unwrap_or_default()))
                })
                .collect();
            evidence.push(json!({ "dimension": "paths", "values": files, "source": "request", "inferred": false }));
        }
    }
    let mut selection = entry.as_object().expect("validated entry").clone();
    selection.shift_remove("value");
    selection.insert("evidence".into(), Value::Array(evidence));
    selection.insert("reason".into(), entry["when"].clone());
    Value::Object(selection)
}

/// The loading plan: selected guidance entries, the evidence behind each, and what is missing.
pub fn plan(request: &Request, catalog: &Value) -> Value {
    let stack = lists(catalog, "stackIntents", request.intent)
        && !(request.intent == "verify" && request.verification_mode == Some("manual"));
    let modules = request.modules;
    let unmatched: Vec<String> = request
        .files
        .iter()
        .filter(|file| !modules.iter().any(|module| member(file, path(module))))
        .cloned()
        .collect();
    let mut missing = Vec::new();
    let mut contexts = Vec::new();
    for module in modules {
        let files = request
            .files
            .iter()
            .filter(|file| {
                member(file, path(module))
                    && !modules.iter().any(|other| {
                        path(other).len() > path(module).len() && member(file, path(other))
                    })
            })
            .cloned()
            .collect();
        contexts.push(context(
            request,
            catalog,
            Some(module),
            files,
            &mut missing,
            stack,
        ));
    }
    if modules.is_empty() || !unmatched.is_empty() {
        let files = if modules.is_empty() {
            request.files.to_vec()
        } else {
            unmatched
        };
        contexts.push(context(request, catalog, None, files, &mut missing, stack));
    }
    let mut selections = Vec::new();
    for entry in entries(catalog) {
        let when = entry["when"].as_object().expect("validated selector");
        if !stack && !when.contains_key("intents") {
            continue;
        }
        let matched: Vec<&Context> = contexts
            .iter()
            .filter(|context| {
                when.iter()
                    .all(|(dimension, choices)| matches(context, request, dimension, choices))
            })
            .collect();
        if !matched.is_empty() {
            selections.push(selection(entry, &matched));
        }
    }
    let id = |entry: &Value| entry["id"].as_str().unwrap_or_default().to_string();
    let ids = |prefixes: &[&str]| -> Vec<String> {
        selections
            .iter()
            .map(id)
            .filter(|id| prefixes.iter().any(|prefix| id.starts_with(prefix)))
            .collect()
    };
    let documentation: Vec<Value> = selections
        .iter()
        .filter_map(|entry| {
            let question = entry.get("documentationQuestion")?;
            Some(json!({ "id": entry["id"], "question": question, "status": "resolve-only-if-affected; pinned-version-required" }))
        })
        .collect();
    let unavailable: Vec<Value> = selections
        .iter()
        .filter_map(|entry| {
            let reason = entry.get("unavailable")?;
            Some(json!({ "id": entry["id"], "reason": reason }))
        })
        .collect();
    let evidence: Vec<Value> = if stack {
        contexts
            .iter()
            .flat_map(|context| context.evidence.clone())
            .collect()
    } else {
        Vec::new()
    };
    if stack && request.files.is_empty() {
        missing.push(NO_PATHS.into());
    }
    json!({
        "workflow": ids(&["workflow:"]).first(),
        "practices": ids(&["practice:"]),
        "specializations": ids(&SPECIALIZATIONS),
        "selections": selections,
        "documentationQuestions": documentation,
        "unavailable": unavailable,
        "evidence": evidence,
        "missingContext": missing,
        "evidencePolicy": POLICY,
    })
}
