//! One discovery invocation: validated session observations are supplied by the W2 bridge.
use crate::{
    cli::Failure,
    project,
    telemetry::{Operation, Recorder},
};
use craft_engine::discovery::{self, planning};
use craft_platform::{search, state};
use serde_json::{Map, Value, json};
use std::path::Path;

impl From<search::SearchError> for Failure {
    fn from(error: search::SearchError) -> Self {
        match error {
            search::SearchError::Io(error) => Self::from_io(error),
            search::SearchError::Invalid(message) => message.into(),
        }
    }
}

pub struct Context<'a> {
    pub root: &'a Path,
    pub session: Option<&'a str>,
    pub task: Option<&'a str>,
    pub preferences: Option<&'a Path>,
    pub bridge: &'a Value,
}

pub fn run(
    input: &Value,
    context: Context<'_>,
    recorder: &mut Recorder,
    parent: &Operation,
) -> Result<Value, Failure> {
    let operation = recorder.start("discovery.completed", Some(parent));
    let result = discover(input, context, recorder, &operation);
    let (attributes, failure) = match &result {
        Ok(result) => (
            Map::from_iter([
                ("batchSize".into(), result["discovery"]["fresh"].clone()),
                (
                    "evidenceCount".into(),
                    json!(result["matches"].as_array().map_or(0, Vec::len)),
                ),
                (
                    "reason".into(),
                    json!(if result["terms"].as_array().is_some_and(Vec::is_empty) {
                        "deferred"
                    } else {
                        "observed"
                    }),
                ),
            ]),
            None,
        ),
        Err(error) => (Map::new(), Some(error.code("SEARCH_FAILED"))),
    };
    recorder.complete(operation, attributes, failure);
    result
}

fn discover(
    input: &Value,
    context: Context<'_>,
    recorder: &mut Recorder,
    parent: &Operation,
) -> Result<Value, Failure> {
    let Context {
        root,
        session,
        task,
        preferences,
        bridge,
    } = context;
    if !input.is_object() {
        return Err("invalid bounded locator request".into());
    }
    if session.is_some() && bridge.is_null() {
        return Err("session lookup requires validated task context".into());
    }
    let mut input = input.clone();
    let repositories = bridge
        .get("repositories")
        .map(discovery::repository_scopes)
        .transpose()?;
    if let Some(repositories) = &repositories {
        let paths = match input
            .get("paths")
            .and_then(Value::as_array)
            .filter(|a| !a.is_empty())
        {
            Some(paths) => paths.clone(),
            None => repositories.iter().map(|s| json!(s)).collect(),
        };
        for path in &paths {
            let path = path
                .as_str()
                .ok_or("paths must be an array of relative scopes")?;
            if !repositories.iter().any(|scope| {
                scope == "." || path == scope || path.starts_with(&format!("{scope}/"))
            }) {
                return Err("lookup paths must belong to a selected repository".into());
            }
        }
        input["paths"] = json!(paths);
    }
    let request = planning::request(&input)?;
    if request["terms"].as_array().unwrap().is_empty() {
        return Ok(planning::deferred(&request)?);
    }
    let root = std::fs::canonicalize(root).map_err(Failure::from_io)?;
    let settings = loaded_settings(&root, preferences, recorder, parent)?;
    let policy = planning::digest(&settings);
    let enabled = session.is_some()
        && task.is_some()
        && settings["efficiency"]["lookup"]["enabled"] == true
        && settings["efficiency"]["lookup"]["reuse"] == true;
    let state = bridge.get("state").cloned().unwrap_or(Value::Null);
    let mut diagnostic = Value::Null;
    if enabled && state.is_null() {
        diagnostic = bridge
            .get("diagnostic")
            .cloned()
            .unwrap_or(json!("validated session state unavailable"));
        recorder.report("state", "STATE_UNAVAILABLE", Map::new(), parent);
    }
    let paths: Vec<_> = request["paths"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p.as_str().unwrap().to_string())
        .collect();
    let max_files = request["options"]["maxFiles"].as_u64().unwrap_or(500) as usize;
    let inventory = if enabled && !state.is_null() && repositories.is_none() {
        search::identity(&root, &paths, max_files)
    } else {
        None
    };
    let identity = inventory.as_ref().map_or(Value::Null, |inventory| {
        json!(planning::digest(&json!([inventory, policy])))
    });
    let cache = recorder.start("cache.lookup", Some(parent));
    let reuse = planning::reuse(&request["terms"], &request["paths"], &identity, &state)?;
    let reused = reuse["reused"].as_array().unwrap().len();
    let fresh = reuse["fresh"].as_array().unwrap().len();
    let mut attributes = Map::from_iter([("available".into(), json!(!identity.is_null()))]);
    if !identity.is_null() {
        attributes.insert("cacheHits".into(), json!(reused));
        attributes.insert("cacheMisses".into(), json!(fresh));
    }
    recorder.complete(cache, attributes, None);
    let attributes = if identity.is_null() {
        Map::from_iter([("reason".into(), json!("unavailable"))])
    } else {
        Map::from_iter([
            ("cacheHits".into(), json!(reused)),
            ("cacheMisses".into(), json!(fresh)),
            ("reused".into(), json!(reused > 0)),
            ("reason".into(), json!("observed")),
        ])
    };
    recorder.observe("cache.observed", attributes, parent, &[]);
    let scanned = if fresh > 0 {
        let mut options = request["options"].clone();
        options["terms"] = reuse["fresh"].clone();
        let search = discovery::request(&options)?;
        let operation = recorder.start("search.completed", Some(parent));
        let result = search::scan(&root, &search, repositories.as_deref());
        let (attributes, failure) = match &result {
            Ok(result) => {
                if result["truncated"] == true
                    || result["skipped"]
                        .as_object()
                        .is_some_and(|m| m.values().any(|v| v.as_u64().unwrap_or(0) > 0))
                {
                    recorder.report("discovery", "SEARCH_INCOMPLETE", Map::new(), &operation);
                }
                (
                    Map::from_iter([
                        (
                            "evidenceCount".into(),
                            json!(result["matches"].as_array().unwrap().len()),
                        ),
                        ("complete".into(), json!(result["truncated"] != true)),
                    ]),
                    None,
                )
            }
            Err(error) => (
                Map::new(),
                Some(Failure::from_error_code(error.code()).code("SEARCH_FAILED")),
            ),
        };
        recorder.complete(operation, attributes, failure);
        result?
    } else {
        json!({"inventory":"validated-reuse","terms":request["terms"],"paths":request["paths"],"filesScanned":0,"bytesRead":0,"matches":[],"skipped":{},"truncated":false,"truncation":[],"limitations":["Reused literal navigation evidence; not semantic definitions or proof of absence."]})
    };
    let stable = inventory
        .as_ref()
        .is_some_and(|before| search::identity(&root, &paths, max_files).as_ref() == Some(before))
        && planning::digest(&loaded_settings(&root, preferences, recorder, parent)?) == policy;
    let mut facts = json!({"request":request,"scanned":scanned,"state":state,"identity":identity,"reused":reuse["reused"],"fresh":reuse["fresh"],"stable":stable,"enabled":enabled,"diagnostic":diagnostic});
    if enabled && !state.is_null() {
        let operation = recorder.start("state.updated", Some(parent));
        let updated = (|| -> Result<(), String> {
            let session = session.unwrap();
            let task = task.unwrap();
            let name = format!(
                "session-{}.json",
                &planning::digest(&json!([session, task]))[..24]
            );
            let file = state::file(&root, &name, true, Some(session))?;
            let _ticket = state::lock(&file)?.ok_or("session state writer is busy")?;
            let mut current = state::read(&file, 131072)?
                .unwrap_or(json!({"formatVersion":1,"activity":{},"lookups":{},"attempts":[]}));
            craft_engine::contracts::normalize_numbers(&mut current);
            // Only the exact observation validated by the current session owner is writable.
            // A concurrent receipt must be preserved; the next request can validate it again.
            if current != state {
                return Err("session state changed during discovery; receipt preserved".into());
            }
            let updated = planning::update(current, &facts)?;
            state::write(&file, &updated, 131072, false)
        })();
        if let Err(error) = &updated {
            recorder.report("state", "STATE_UNAVAILABLE", Map::new(), &operation);
            facts["diagnostic"] = json!(error);
        }
        recorder.complete(
            operation,
            Map::from_iter([("area".into(), json!("session"))]),
            updated.err().map(|_| "STATE_UNAVAILABLE"),
        );
    }
    Ok(planning::result(&facts)?)
}

fn loaded_settings(
    root: &Path,
    preferences: Option<&Path>,
    recorder: &mut Recorder,
    parent: &Operation,
) -> Result<Value, Failure> {
    let operation = recorder.start("project.loaded", Some(parent));
    let result = project::settings(root, preferences);
    recorder.complete(
        operation,
        Map::new(),
        result.as_ref().err().map(|e| e.code("CONFIG_INVALID")),
    );
    result
}

/// Numeric flags follow JavaScript Number(), including whitespace and radix literals.
pub fn number(text: &str) -> Value {
    let text = text.trim_matches(|c: char| c == '\u{feff}' || (c.is_whitespace() && c != '\u{85}'));
    if text.is_empty() {
        return json!(0);
    }
    let radix = if text.starts_with("0x") || text.starts_with("0X") {
        Some(16)
    } else if text.starts_with("0o") || text.starts_with("0O") {
        Some(8)
    } else if text.starts_with("0b") || text.starts_with("0B") {
        Some(2)
    } else {
        None
    };
    let n = if let Some(radix) = radix {
        u64::from_str_radix(&text[2..], radix)
            .ok()
            .map(|n| n as f64)
    } else {
        text.parse::<f64>().ok()
    };
    n.and_then(serde_json::Number::from_f64)
        .map_or(Value::Null, Value::Number)
}
