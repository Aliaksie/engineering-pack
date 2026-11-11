//! Project configuration and effective settings, replayed against
//! `tests/fixtures/runtime/settings-contracts.json`.
use crate::layers::Layer;
use crate::values::{MAX_SAFE_INTEGER, object, safe_integer};
use serde_json::{Map, Value, json};

pub const LAYERS: &[&str] = &["ui", "backend", "agent"];
pub const LANGUAGES: &[&str] = &[
    "javascript",
    "typescript",
    "python",
    "java",
    "kotlin",
    "jvm",
    "csharp",
    "go",
    "rust",
    "ruby",
];
pub const READ_OPERATIONS: &[&str] = &[
    "git.status",
    "git.diff",
    "change.read",
    "change.diff",
    "task.read",
    "task.search",
];
pub const WRITE_OPERATIONS: &[&str] = &[
    "git.commit",
    "git.push",
    "change.publish",
    "task.transition",
    "task.comment",
];
pub const ROLES: &[&str] = &[
    "locator",
    "researcher",
    "implementer",
    "reviewer",
    "planner",
    "verifier",
];
/// Sections a layer or task overrides may set in part, against the bundled defaults.
pub(crate) const SWITCHES: [&str; 4] = ["routing", "checkpoint", "workspace", "efficiency"];
type Result<T = ()> = std::result::Result<T, String>;

fn strings(value: Option<&Value>, field: &str, allowed: Option<&[&str]>) -> Result {
    let valid = value.and_then(Value::as_array).is_some_and(|items| {
        items.iter().all(|item| {
            item.as_str()
                .is_some_and(|text| allowed.is_none_or(|allowed| allowed.contains(&text)))
        })
    });
    if valid {
        Ok(())
    } else {
        Err(format!("invalid {field}"))
    }
}

fn optional_strings(
    map: &Map<String, Value>,
    field: &str,
    label: &str,
    allowed: Option<&[&str]>,
) -> Result {
    match map.get(field) {
        Some(value) => strings(Some(value), label, allowed),
        None => Ok(()),
    }
}

pub fn kebab(text: &str) -> bool {
    !text.is_empty()
        && text.split('-').all(|part| {
            !part.is_empty()
                && part
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
        })
}

fn host_name(text: &str) -> bool {
    let (name, port) = match text.split_once(':') {
        Some((name, port)) => (name, Some(port)),
        None => (text, None),
    };
    !name.is_empty()
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'.' || b == b'-')
        && port.is_none_or(|port| !port.is_empty() && port.bytes().all(|b| b.is_ascii_digit()))
}

fn repository_identity(text: &str) -> bool {
    let parts: Vec<_> = text.split('/').collect();
    parts.len() >= 2
        && parts.iter().all(|part| {
            !part.is_empty()
                && *part != "."
                && *part != ".."
                && part
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"_.-".contains(&b))
        })
}

/// A project-relative path, normalized without `.` components.
pub fn relative_file(value: &Value) -> Result<String> {
    let text = value.as_str().unwrap_or_default();
    let drive =
        text.len() >= 2 && text.as_bytes()[0].is_ascii_alphabetic() && text.as_bytes()[1] == b':';
    if text.is_empty()
        || text.contains(['\\', '\0'])
        || text.starts_with('/')
        || drive
        || text.split('/').any(|p| p == ".." || p.is_empty())
    {
        return Err(format!(
            "expected project-relative path: {}",
            value
                .as_str()
                .map_or_else(|| value.to_string(), str::to_string)
        ));
    }
    let joined = text
        .split('/')
        .filter(|part| *part != ".")
        .collect::<Vec<_>>()
        .join("/");
    Ok(if joined.is_empty() {
        ".".into()
    } else {
        joined
    })
}

/// `value ?? fallback` treats JSON null as absent.
fn present(value: Option<&Value>) -> Option<&Value> {
    value.filter(|value| !value.is_null())
}

fn boolean(value: Option<&Value>, message: &str) -> Result {
    if value.is_some_and(|v| !v.is_boolean()) {
        Err(message.into())
    } else {
        Ok(())
    }
}

/// Every field a project file may hold.
pub(crate) const PROJECT_FIELDS: [&str; 21] = [
    "$schema",
    "formatVersion",
    "languages",
    "layers",
    "runtimes",
    "frameworks",
    "providers",
    "modules",
    "services",
    "execution",
    "permissions",
    "checkpoint",
    "review",
    "safety",
    "routing",
    "efficiency",
    "workspace",
    "evidence",
    "standards",
    "anchors",
    "skills",
];

pub fn validate_project(project: &Value) -> Result {
    let map = object(project, &PROJECT_FIELDS, "project")?;
    // An editor's schema link; read by editors only.
    if map.get("$schema").is_some_and(|link| !link.is_string()) {
        return Err("$schema must be a string".into());
    }
    if let Some(workspace) = map.get("workspace") {
        validate_settings(workspace, &default_settings()["workspace"], "workspace")?;
    }
    if safe_integer(map.get("formatVersion").unwrap_or(&Value::Null)) != Some(1) {
        return Err("unsupported project formatVersion".into());
    }
    for (field, allowed) in [
        ("languages", Some(LANGUAGES)),
        ("layers", Some(LAYERS)),
        ("runtimes", None),
        ("frameworks", None),
        ("providers", None),
    ] {
        optional_strings(map, field, field, allowed)?;
    }
    let modules = match map.get("modules") {
        Some(Value::Array(modules)) => modules.as_slice(),
        Some(_) => return Err("modules must be an array".into()),
        None => &[],
    };
    let mut names = Vec::new();
    for module in modules {
        let fields = [
            "name",
            "path",
            "owner",
            "languages",
            "layers",
            "skills",
            "dependsOn",
            "checks",
            "providers",
            "runtimes",
            "frameworks",
        ];
        let module = object(module, &fields, "module")?;
        match module.get("name").and_then(Value::as_str) {
            Some(name)
                if !name.is_empty()
                    && name
                        .bytes()
                        .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
                    && !names.contains(&name) =>
            {
                names.push(name)
            }
            _ => return Err("invalid or duplicate module name".into()),
        }
        relative_file(module.get("path").unwrap_or(&Value::Null))?;
        for (field, allowed) in [
            ("languages", Some(LANGUAGES)),
            ("layers", Some(LAYERS)),
            ("skills", None),
            ("dependsOn", None),
            ("providers", None),
            ("runtimes", None),
            ("frameworks", None),
        ] {
            optional_strings(module, field, &format!("module.{field}"), allowed)?;
        }
        if module.get("owner").is_some_and(|owner| !owner.is_string()) {
            return Err("module owner must be text".into());
        }
        let checks = match module.get("checks") {
            Some(Value::Array(checks)) => checks.as_slice(),
            Some(_) => return Err("checks must be an array".into()),
            None => &[],
        };
        for check in checks {
            let check = object(
                check,
                &["command", "args", "id", "stage", "timeoutSeconds"],
                "check",
            )?;
            crate::checks::validate(check, "check")?;
            if !check
                .get("command")
                .and_then(Value::as_str)
                .is_some_and(|c| !c.trim().is_empty())
            {
                return Err("check command required".into());
            }
            strings(
                Some(present(check.get("args")).unwrap_or(&json!([]))),
                "check args",
                None,
            )?;
        }
    }
    for module in modules {
        for dependency in module
            .get("dependsOn")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            let dependency = dependency.as_str().unwrap_or_default();
            if !names.contains(&dependency) {
                return Err(format!("unknown module dependency: {dependency}"));
            }
        }
    }
    let catalog = crate::routing::catalog::catalog()?;
    let intents: Vec<&str> = catalog["intents"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .collect();
    if let Some(standards) = map.get("standards") {
        crate::standards::validate(standards, &names, &intents)?;
    }
    if let Some(skills) = map.get("skills") {
        let known = crate::routing::catalog::skills(catalog);
        crate::skills::validate(skills, &known, &names, &intents)?;
    }
    if let Some(anchors) = map.get("anchors") {
        validate_anchor_settings(anchors)?;
    }
    if let Some(services) = map.get("services") {
        for (service, binding) in object(services, &["git", "codehost", "tasks"], "services")? {
            validate_binding(service, binding)?;
        }
    }
    if let Some(permissions) = map.get("permissions") {
        let permissions = object(permissions, &["writeOperations"], "permissions")?;
        strings(
            Some(present(permissions.get("writeOperations")).unwrap_or(&json!([]))),
            "writeOperations",
            Some(WRITE_OPERATIONS),
        )?;
    }
    if let Some(execution) = map.get("execution") {
        validate_execution(object(
            execution,
            &["roles", "hosts", "profiles"],
            "execution",
        )?)?;
    }
    if let Some(checkpoint) = map.get("checkpoint") {
        let checkpoint = object(checkpoint, &["enabled"], "checkpoint")?;
        if !checkpoint.get("enabled").is_some_and(Value::is_boolean) {
            return Err("checkpoint enabled must be boolean".into());
        }
    }
    if let Some(review) = map.get("review") {
        let review = object(review, &["readOnly"], "review")?;
        if !review.get("readOnly").is_some_and(Value::is_boolean) {
            return Err("review.readOnly must be boolean".into());
        }
    }
    if let Some(safety) = map.get("safety") {
        crate::safety::validate(safety)?;
    }
    if let Some(routing) = map.get("routing") {
        let routing = object(
            routing,
            &[
                "promptHints",
                "editHints",
                "dedupeHints",
                "standardChecks",
                "stopReview",
                "debtMarkers",
                "testRuns",
                "workerStandards",
                "sessionRecords",
                "sessionSummary",
                "conventionProposals",
                "projectChecks",
            ],
            "routing",
        )?;
        for field in [
            "promptHints",
            "editHints",
            "dedupeHints",
            "standardChecks",
            "stopReview",
            "debtMarkers",
            "testRuns",
            "workerStandards",
            "sessionRecords",
            "sessionSummary",
            "conventionProposals",
            "projectChecks",
        ] {
            boolean(
                routing.get(field),
                &format!("routing {field} must be boolean"),
            )?;
        }
    }
    if let Some(efficiency) = map.get("efficiency") {
        validate_settings(efficiency, &default_settings()["efficiency"], "efficiency")?;
    }
    evidence_exclusions(project)?;
    evidence_enabled(project)?;
    Ok(())
}

/// Whether routed lookups use Project Intelligence: on unless the project turns it off.
pub fn evidence_enabled(project: &Value) -> Result<bool> {
    match project
        .get("evidence")
        .and_then(|evidence| evidence.get("enabled"))
    {
        None => Ok(true),
        Some(Value::Bool(enabled)) => Ok(*enabled),
        Some(_) => Err("evidence.enabled must be a boolean".into()),
    }
}

/// Project-relative path prefixes Project Intelligence leaves out of its inventory, such as
/// generated samples or vendored bundles. A trailing `/` is accepted; at most 64, sorted.
pub fn evidence_exclusions(project: &Value) -> Result<Vec<String>> {
    let Some(evidence) = project.get("evidence") else {
        return Ok(Vec::new());
    };
    let evidence = object(evidence, &["enabled", "exclude"], "evidence")?;
    let list = match evidence.get("exclude") {
        None => return Ok(Vec::new()),
        Some(Value::Array(list)) if list.len() <= 64 => list,
        Some(_) => return Err("evidence.exclude must be an array of at most 64 paths".into()),
    };
    let mut prefixes = list
        .iter()
        .map(|item| {
            let text = item
                .as_str()
                .ok_or("evidence.exclude entries must be text")?;
            relative_file(&json!(text.strip_suffix('/').unwrap_or(text)))
        })
        .collect::<Result<Vec<_>>>()?;
    prefixes.sort();
    prefixes.dedup();
    Ok(prefixes)
}

/// Whether `path` is one of `prefixes` or lies beneath one, by whole path components.
pub fn excluded(path: &str, prefixes: &[String]) -> bool {
    prefixes.iter().any(|prefix| {
        path.strip_prefix(prefix.as_str())
            .is_some_and(|rest| rest.is_empty() || rest.starts_with('/'))
    })
}

fn validate_binding(service: &str, binding: &Value) -> Result {
    let binding = object(
        binding,
        &[
            "provider",
            "transport",
            "command",
            "host",
            "repository",
            "operations",
        ],
        "binding",
    )?;
    let transport = binding
        .get("transport")
        .and_then(Value::as_str)
        .unwrap_or_default();
    if !binding
        .get("provider")
        .and_then(Value::as_str)
        .is_some_and(|p| !p.is_empty())
        || !["cli", "mcp"].contains(&transport)
    {
        return Err("invalid service binding".into());
    }
    if transport == "cli"
        && !binding
            .get("command")
            .and_then(Value::as_str)
            .is_some_and(|c| !c.trim().is_empty())
    {
        return Err("binding command required".into());
    }
    for field in ["host", "repository"] {
        if binding
            .get(field)
            .is_some_and(|value| !value.as_str().is_some_and(|text| !text.trim().is_empty()))
        {
            return Err(format!("invalid binding {field}"));
        }
    }
    if binding
        .get("host")
        .and_then(Value::as_str)
        .is_some_and(|host| !host_name(host))
    {
        return Err("invalid binding host".into());
    }
    if binding
        .get("repository")
        .and_then(Value::as_str)
        .is_some_and(|repository| !repository_identity(repository))
    {
        return Err("invalid repository identity".into());
    }
    let operations = match binding.get("operations") {
        Some(Value::Object(operations)) => Some(operations),
        Some(_) => return Err("binding operations must be an object".into()),
        None => None,
    };
    let prefix = match service {
        "tasks" => "task.",
        "codehost" => "change.",
        _ => "git.",
    };
    for (operation, mapping) in operations.into_iter().flatten() {
        if !operation.starts_with(prefix) {
            return Err("operation belongs to a different service".into());
        }
        if !READ_OPERATIONS.contains(&operation.as_str())
            && !WRITE_OPERATIONS.contains(&operation.as_str())
        {
            return Err(format!("unknown operation {operation}"));
        }
        let mapping = object(
            mapping,
            if transport == "cli" {
                &["args"]
            } else {
                &["tool", "arguments"]
            },
            "operation mapping",
        )?;
        if transport == "cli" {
            strings(mapping.get("args"), "operation args", None)?;
        } else if !mapping
            .get("tool")
            .and_then(Value::as_str)
            .is_some_and(|tool| {
                !tool.is_empty()
                    && tool
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b"_.:-".contains(&b))
            })
            || !mapping.get("arguments").is_some_and(Value::is_object)
        {
            return Err("MCP mapping requires exact tool name and arguments".into());
        }
    }
    Ok(())
}

fn validate_execution(execution: &Map<String, Value>) -> Result {
    match execution.get("roles") {
        Some(Value::Object(roles)) => {
            for (role, rule) in roles {
                if !ROLES.contains(&role.as_str()) {
                    return Err("unknown execution role".into());
                }
                let rule = object(rule, &["mode", "profile"], "execution role")?;
                let mode = rule.get("mode").and_then(Value::as_str).unwrap_or_default();
                if !["main", "delegate", "tool"].contains(&mode)
                    || rule.get("profile").is_some_and(|p| !p.is_string())
                {
                    return Err("invalid execution rule".into());
                }
            }
        }
        Some(_) => return Err("execution roles must be an object".into()),
        None => {}
    }
    match execution.get("hosts") {
        Some(Value::Object(hosts)) => {
            for host in hosts.values() {
                let host = object(
                    host,
                    &["delegation", "models", "mainModelSwitch", "contextLimits"],
                    "execution host",
                )?;
                if let Some(limits) = host.get("contextLimits") {
                    validate_context_limits(limits)?;
                }
                boolean(host.get("delegation"), "delegation must be boolean")?;
                boolean(
                    host.get("mainModelSwitch"),
                    "mainModelSwitch must be boolean",
                )?;
                if host.get("models").is_some_and(|models| {
                    !models.as_object().is_some_and(|models| {
                        models
                            .values()
                            .all(|m| m.as_str().is_some_and(|m| !m.is_empty()))
                    })
                }) {
                    return Err("model profiles must map to explicit host model names".into());
                }
            }
        }
        Some(_) => return Err("execution hosts must be an object".into()),
        None => {}
    }
    let Some(profiles) = execution.get("profiles") else {
        return Ok(());
    };
    let profiles = match profiles.as_object() {
        Some(profiles) if profiles.len() <= 16 => profiles,
        _ => return Err("execution profiles must be an object with at most 16 profiles".into()),
    };
    for (name, profile) in profiles {
        if !kebab(name) {
            return Err("invalid execution profile name".into());
        }
        let profile = object(
            profile,
            &["capabilities", "maxAttempts", "escalateTo", "class"],
            "execution profile",
        )?;
        if profile.get("class").is_some_and(|class| {
            !class
                .as_str()
                .is_some_and(|c| ["economical", "standard", "strong"].contains(&c))
        }) {
            return Err("invalid profile class".into());
        }
        strings(profile.get("capabilities"), "profile capabilities", None)?;
        let capabilities = profile["capabilities"]
            .as_array()
            .expect("validated capabilities");
        if capabilities.len() > 32
            || capabilities
                .iter()
                .any(|item| !kebab(item.as_str().unwrap_or_default()))
        {
            return Err("invalid profile capabilities".into());
        }
        if profile
            .get("maxAttempts")
            .is_some_and(|n| !safe_integer(n).is_some_and(|n| (1..=5).contains(&n)))
        {
            return Err("profile maxAttempts must be 1..5".into());
        }
        if profile
            .get("escalateTo")
            .is_some_and(|target| !target.as_str().is_some_and(|t| profiles.contains_key(t)))
        {
            return Err("unknown escalation profile".into());
        }
    }
    for name in profiles.keys() {
        let (mut seen, mut next) = (Vec::new(), Some(name.as_str()));
        while let Some(current) = next {
            if seen.contains(&current) {
                return Err("execution profile escalation cycle".into());
            }
            seen.push(current);
            next = profiles[current].get("escalateTo").and_then(Value::as_str);
        }
    }
    Ok(())
}

/// Bundled defaults. Preferences select behavior; authorization and host support stay separate facts.
pub fn default_settings() -> Value {
    json!({
        // Every switch over what a hook shows or records, so `craft settings` lists each with
        // its origin.
        "routing": {
            "promptHints": true, "editHints": true, "dedupeHints": true, "standardChecks": true,
            "stopReview": true, "debtMarkers": false, "testRuns": true, "workerStandards": true,
            "sessionRecords": true, "sessionSummary": true, "conventionProposals": true,
            "projectChecks": true
        },
        "checkpoint": { "enabled": true },
        "workspace": {
            "enabled": true, "maxSessions": 20,
            "scratch": { "staleAfterHours": 24, "maxBytes": 8_388_608, "maxFiles": 64 },
            "evidence": { "enabled": true, "retentionDays": 7, "maxArtifacts": 16, "maxBytes": 1_048_576 }
        },
        "efficiency": {
            "lookup": { "enabled": true, "reuse": true },
            "batching": { "enabled": true, "maxConcurrency": 3 },
            "attempts": { "enabled": true, "maxAttempts": 3 },
            "advice": { "enabled": true, "at": "meaningful-boundaries", "nextStep": true },
            "review": { "mode": "recommend" },
            "context": { "elevatedAt": 0.60, "compactAt": 0.75, "restartAt": 0.85, "action": "recommend" },
            "delegation": { "mode": "recommend", "maxWorkers": 3 }
        }
    })
}

/// Logical classes are declarations, not measured rankings or vendor model names.
fn default_execution() -> Value {
    let roles: Map<String, Value> = ROLES
        .iter()
        .map(|role| {
            let (mode, profile) = match *role {
                "locator" => ("tool", "economical"),
                "reviewer" => ("main", "strong"),
                _ => ("main", "standard"),
            };
            (
                role.to_string(),
                json!({ "mode": mode, "profile": profile }),
            )
        })
        .collect();
    json!({ "execution": {
        "profiles": {
            "economical": { "class": "economical", "capabilities": [], "escalateTo": "standard" },
            "standard": { "class": "standard", "capabilities": [], "escalateTo": "strong" },
            "strong": { "class": "strong", "capabilities": [] }
        },
        "roles": roles
    } })
}

/// Context limits by exact host model name: a window and an optional working budget, both
/// positive token counts. Used for project declarations and the bundled host table.
pub fn validate_context_limits(limits: &Value) -> Result {
    let limits = limits
        .as_object()
        .filter(|limits| limits.len() <= 16)
        .ok_or("context limits must map at most 16 host model names")?;
    for (model, limit) in limits {
        let limit = object(limit, &["windowTokens", "budgetTokens"], "context limit")?;
        let positive = |v: &Value| safe_integer(v).is_some_and(|n| n > 0);
        if model.is_empty() || limit.is_empty() || !limit.values().all(positive) {
            return Err(format!("invalid context limit for model {model:?}"));
        }
    }
    Ok(())
}

fn validate_settings(value: &Value, template: &Value, label: &str) -> Result {
    let map = value
        .as_object()
        .ok_or_else(|| format!("{label} must be an object"))?;
    for (key, item) in map {
        let expected = template
            .get(key)
            .ok_or_else(|| format!("unknown {label} field: {key}"))?;
        let field = format!("{label}.{key}");
        match expected {
            Value::Object(_) => validate_settings(item, expected, &field)?,
            Value::Bool(_) if !item.is_boolean() => return Err(format!("{field} must be boolean")),
            Value::Bool(_) => {}
            Value::String(_) => {
                let allowed: &[&str] = match key.as_str() {
                    "at" => &["meaningful-boundaries"],
                    _ => &["manual", "recommend", "execute-when-authorized"],
                };
                if !item.as_str().is_some_and(|text| allowed.contains(&text)) {
                    return Err(format!("invalid {field}"));
                }
            }
            _ if key.ends_with("At") => {
                if !item.as_f64().is_some_and(|n| n > 0.0 && n <= 1.0) {
                    return Err(format!("{field} must be greater than zero and at most one"));
                }
            }
            _ => {
                let maximum = match key.as_str() {
                    "maxBytes" => 67_108_864,
                    "maxFiles" => 1024,
                    "staleAfterHours" => 8760,
                    "retentionDays" => 365,
                    "maxWorkers" => MAX_SAFE_INTEGER,
                    "maxAttempts" => 5,
                    _ => 64,
                };
                let minimum = if key == "maxWorkers" { 0 } else { 1 };
                if !safe_integer(item).is_some_and(|n| n >= minimum && n <= maximum) {
                    return Err(format!("invalid {field}"));
                }
            }
        }
    }
    Ok(())
}

/// Switch sections (`routing`, `checkpoint`, `workspace`, `efficiency`) set in part, as
/// a baseline layer or task overrides set them.
pub(crate) fn validate_switches(value: &Value) -> Result {
    validate_settings(value, &default_settings(), "settings")
}

/// Where each merged value came from: the layer of every field, of every entry of an
/// id-merged list and, when asked for, each value a field took in merge order.
#[derive(Default)]
struct Trace {
    origins: Map<String, Value>,
    entries: Map<String, Value>,
    history: Option<Map<String, Value>>,
}

impl Trace {
    fn record(&mut self, field: &str, origin: &str, value: Option<&Value>) {
        let Some(history) = &mut self.history else {
            return;
        };
        let step = match value {
            Some(value) => json!({ "origin": origin, "value": value }),
            None => json!({ "origin": origin, "disabled": true }),
        };
        match history.get_mut(field).and_then(Value::as_array_mut) {
            Some(steps) => steps.push(step),
            None => {
                history.insert(field.into(), json!([step]));
            }
        }
    }
}

/// Merge the entries of an id-merged list: an entry replaces the one of its id in place or
/// is appended; a disabled entry removes it.
fn merge_entries(
    target: &mut Vec<Value>,
    source: &[Value],
    origin: &str,
    trace: &mut Trace,
    field: &str,
) {
    for entry in source {
        let id = entry["id"].as_str().unwrap_or_default();
        let at = target.iter().position(|known| known["id"] == entry["id"]);
        let off = crate::layers::disabled(entry);
        trace.record(&format!("{field}[{id}]"), origin, (!off).then_some(entry));
        let origins = trace
            .entries
            .entry(field)
            .or_insert_with(|| json!({}))
            .as_object_mut()
            .expect("entry origins");
        if off {
            origins.remove(id);
        } else {
            origins.insert(id.into(), json!(origin));
        }
        match (at, off) {
            (Some(at), true) => {
                target.remove(at);
            }
            (Some(at), false) => target[at] = entry.clone(),
            (None, false) => target.push(entry.clone()),
            (None, true) => {}
        }
    }
}

fn merge(
    target: &mut Map<String, Value>,
    source: &Map<String, Value>,
    origin: &str,
    trace: &mut Trace,
    prefix: &str,
) {
    for (key, value) in source {
        let field = if prefix.is_empty() {
            key.clone()
        } else {
            format!("{prefix}.{key}")
        };
        match value {
            Value::Array(source) if crate::layers::ENTRY_LISTS.contains(&field.as_str()) => {
                let entry = target.entry(key.clone()).or_insert_with(|| json!([]));
                if !entry.is_array() {
                    *entry = json!([]);
                }
                let entries = entry.as_array_mut().expect("array entry");
                merge_entries(entries, source, origin, trace, &field);
                trace.origins.insert(field, json!(origin));
            }
            Value::Object(source) => {
                let entry = target.entry(key.clone()).or_insert_with(|| json!({}));
                if !entry.is_object() {
                    *entry = json!({});
                }
                merge(
                    entry.as_object_mut().expect("object entry"),
                    source,
                    origin,
                    trace,
                    &field,
                );
            }
            _ => {
                target.insert(key.clone(), value.clone());
                trace.record(&field, origin, Some(value));
                trace.origins.insert(field, json!(origin));
            }
        }
    }
}

/// `anchors` in the project: catalog files, whether Craft's defaults apply and which are off.
fn validate_anchor_settings(anchors: &Value) -> Result {
    let anchors = object(anchors, &["catalogs", "defaults", "disables"], "anchors")?;
    if let Some(catalogs) = anchors.get("catalogs") {
        let files = catalogs
            .as_array()
            .filter(|files| files.len() <= crate::anchors::MAX_CATALOGS)
            .ok_or_else(|| {
                format!(
                    "anchors.catalogs must be an array of at most {} files",
                    crate::anchors::MAX_CATALOGS
                )
            })?;
        for file in files {
            relative_file(file).map_err(|error| format!("anchors.catalogs: {error}"))?;
        }
    }
    boolean(anchors.get("defaults"), "anchors.defaults must be boolean")?;
    if let Some(disables) = anchors.get("disables") {
        let names = disables
            .as_array()
            .ok_or("anchors.disables must be an array of anchor names")?;
        if names.iter().any(|name| !name.as_str().is_some_and(kebab)) {
            return Err("anchors.disables entries must be kebab-case anchor names".into());
        }
    }
    Ok(())
}

/// The project catalog files `anchors.catalogs` lists, in order, for the application to read.
pub fn anchor_catalogs(project: &Value) -> Vec<String> {
    project["anchors"]["catalogs"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|file| relative_file(file).ok())
        .collect()
}

/// Settings and value origins: bundled < user preferences < project < task overrides.
pub fn effective_settings(
    project: &Value,
    preferences: &Value,
    overrides: &Value,
) -> Result<Value> {
    effective_settings_with(project, preferences, overrides, &Documents::default())
}

/// Project files the caller read for the settings: each `(path, document)` that
/// `anchor_catalogs` named, and each `(path, text if readable)` of the SKILL.md files
/// `skills::declared_files` named.
#[derive(Default)]
pub struct Documents {
    pub catalogs: Vec<(String, Value)>,
    pub skills: Vec<(String, Option<String>)>,
}

/// Effective settings with the project's documents: catalog entries become
/// `anchors.loaded`, declared skill facts `skills.found`.
pub fn effective_settings_with(
    project: &Value,
    preferences: &Value,
    overrides: &Value,
    documents: &Documents,
) -> Result<Value> {
    let user = Layer {
        origin: "user",
        label: "preferences".into(),
        document: preferences,
    };
    layered_settings(project, &[user], overrides, documents, false)
}

/// Effective settings over the baseline `layers`, earliest first, beneath the project:
/// bundled < organization < user < project < task overrides. Each layer and the project are
/// validated alone, then the merged result as a whole. `entries` names the layer of every
/// entry of an id-merged list once a baseline layer supplies one; with `explain`, `history`
/// holds each value a field took, in merge order.
pub fn layered_settings(
    project: &Value,
    layers: &[Layer],
    overrides: &Value,
    documents: &Documents,
    explain: bool,
) -> Result<Value> {
    let catalogs = &documents.catalogs;
    validate_project(project)?;
    let files = crate::skills::declared_files(project);
    if documents.skills.len() != files.len()
        || documents
            .skills
            .iter()
            .zip(&files)
            .any(|((path, _), file)| path != file)
    {
        return Err("skill files do not match skills.declared".into());
    }
    let declared = anchor_catalogs(project);
    if catalogs.len() != declared.len()
        || catalogs
            .iter()
            .zip(&declared)
            .any(|((path, _), file)| path != file)
    {
        return Err("anchor catalogs do not match anchors.catalogs".into());
    }
    let defaults = default_settings();
    for layer in layers {
        crate::layers::validate(layer)?;
    }
    validate_settings(overrides, &defaults, "overrides")?;
    let mut trace = Trace {
        history: explain.then(Map::new),
        ..Trace::default()
    };
    let mut settings = Map::new();
    merge(
        &mut settings,
        defaults.as_object().expect("defaults"),
        "bundled",
        &mut trace,
        "",
    );
    // Only the project replaces the bundled profiles and roles; a baseline layer adds to them,
    // so a project that relies on a bundled profile keeps it whatever the baseline declares.
    if project.pointer("/execution/profiles").is_none() {
        merge(
            &mut settings,
            default_execution().as_object().expect("execution"),
            "bundled",
            &mut trace,
            "",
        );
    }
    let mut baseline = false;
    for layer in layers {
        let values = crate::layers::values(layer.document);
        baseline |= values.keys().any(|key| !SWITCHES.contains(&key.as_str()));
        merge(&mut settings, &values, layer.origin, &mut trace, "");
    }
    let mut project = project.clone();
    project
        .as_object_mut()
        .expect("validated object")
        .remove("$schema");
    for (source, origin) in [(&project, "project"), (overrides, "task")] {
        merge(
            &mut settings,
            source.as_object().expect("validated object"),
            origin,
            &mut trace,
            "",
        );
    }
    let context = &settings["efficiency"]["context"];
    let threshold = |name: &str| context[name].as_f64().unwrap_or(f64::NAN);
    if !(threshold("elevatedAt") < threshold("compactAt")
        && threshold("compactAt") < threshold("restartAt"))
    {
        return Err("context thresholds must satisfy elevatedAt < compactAt < restartAt".into());
    }
    let mut settings = Value::Object(settings);
    // Layers that are valid alone can still disagree: an entry limit, a rule id two of them
    // use, a role naming a profile the project's own profile set leaves out.
    if baseline {
        let merged = || {
            let names: Vec<&str> = layers.iter().map(|layer| layer.label.as_str()).collect();
            format!("merged with {}", names.join(" and "))
        };
        validate_project(&settings).map_err(|error| format!("{}: {error}", merged()))?;
        let execution = &settings["execution"];
        for (role, rule) in execution["roles"].as_object().into_iter().flatten() {
            let profile = rule["profile"].as_str();
            if let Some(profile) = profile.filter(|name| execution["profiles"].get(name).is_none())
            {
                return Err(format!(
                    "{}: execution role {role} names profile {profile}, which no layer declares",
                    merged()
                ));
            }
        }
    }
    if !catalogs.is_empty() {
        let routing = crate::routing::catalog::catalog()?;
        let intents: Vec<&str> = routing["intents"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .collect();
        settings["anchors"]["loaded"] = crate::anchors::load(catalogs, &intents)?;
    }
    if !files.is_empty() {
        settings["skills"]["found"] = crate::skills::found(&documents.skills);
    }
    crate::standards::validate_anchors(&settings)?;
    let mut result = json!({
        "settings": settings,
        "origins": trace.origins,
        "runtimeApplied": [
            "routing", "checkpoint", "workspace", "efficiency.lookup", "efficiency.attempts.maxAttempts",
            "efficiency.context.thresholds", "efficiency.context.action", "efficiency.review.mode",
            "efficiency.delegation.maxWorkers", "efficiency.delegation.mode"
        ],
        "prerequisites": [
            "Automatic semantic acceptance and continuation require agent-supplied task evidence.",
            "Native dispatch, worker-tree lifecycle observations and model switching need verified host integration; supplied facts and decision receipts do not establish execution.",
            "Independent tool batching remains a workflow instruction; preferences never dispatch actions."
        ],
        "authorization": "Preferences do not grant permissions, host support, hook trust or execution authorization."
    });
    if baseline {
        result["entries"] = Value::Object(trace.entries);
    }
    if let Some(history) = trace.history {
        result["history"] = Value::Object(history);
    }
    Ok(result)
}

#[cfg(test)]
mod tests;
