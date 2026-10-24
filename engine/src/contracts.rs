//! Project configuration and effective settings. The JS engine keeps its own copy
//! (`engine/contracts/project.mjs`, `engine/policy/effective-settings.mjs`) until its
//! last command migrates; both replay `tests/fixtures/runtime/engine-contracts.json`.
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
const MAX_SAFE_INTEGER: i64 = 9_007_199_254_740_991;

type Result<T = ()> = std::result::Result<T, String>;

/// `Number.isSafeInteger` over a JSON value; integral floats such as `1.0` qualify.
pub fn safe_integer(value: &Value) -> Option<i64> {
    let number = value.as_number()?;
    let integer = match (number.as_i64(), number.as_f64()) {
        (Some(integer), _) => integer,
        (None, Some(float)) if float.fract() == 0.0 && float.abs() <= MAX_SAFE_INTEGER as f64 => {
            float as i64
        }
        _ => return None,
    };
    (integer.abs() <= MAX_SAFE_INTEGER).then_some(integer)
}

/// JavaScript string length, which bounds identifiers in UTF-16 code units.
pub fn js_length(text: &str) -> usize {
    text.encode_utf16().count()
}

/// Whether `text.trim()` is empty in JavaScript, whose whitespace includes U+FEFF but not U+0085.
pub fn js_blank(text: &str) -> bool {
    text.chars()
        .all(|c| c == '\u{feff}' || (c.is_whitespace() && c != '\u{85}'))
}

fn object<'a>(value: &'a Value, allowed: &[&str], label: &str) -> Result<&'a Map<String, Value>> {
    let map = value
        .as_object()
        .ok_or_else(|| format!("{label} must be an object"))?;
    match map.keys().find(|key| !allowed.contains(&key.as_str())) {
        Some(key) => Err(format!("unknown {label} field: {key}")),
        None => Ok(map),
    }
}

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

pub fn validate_project(project: &Value) -> Result {
    let fields = [
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
        "routing",
        "efficiency",
        "workspace",
    ];
    let map = object(project, &fields, "project")?;
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
            let check = object(check, &["command", "args"], "check")?;
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
    if let Some(routing) = map.get("routing") {
        let routing = object(routing, &["editHints", "dedupeHints"], "routing")?;
        for field in ["editHints", "dedupeHints"] {
            boolean(
                routing.get(field),
                &format!("routing {field} must be boolean"),
            )?;
        }
    }
    if let Some(efficiency) = map.get("efficiency") {
        validate_settings(efficiency, &default_settings()["efficiency"], "efficiency")?;
    }
    Ok(())
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
                    &["delegation", "models", "mainModelSwitch"],
                    "execution host",
                )?;
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
        "routing": { "editHints": true, "dedupeHints": true },
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

/// Copy JSON-integral floats as integers, so `1.0` renders as `JSON.stringify` does.
pub fn normalize_numbers(value: &mut Value) {
    match value {
        Value::Number(number) if number.is_f64() => {
            if let Some(integer) = safe_integer(value) {
                *value = json!(integer);
            }
        }
        Value::Array(items) => items.iter_mut().for_each(normalize_numbers),
        Value::Object(map) => map.values_mut().for_each(normalize_numbers),
        _ => {}
    }
}

fn merge(
    target: &mut Map<String, Value>,
    source: &Map<String, Value>,
    origin: &str,
    origins: &mut Map<String, Value>,
    prefix: &str,
) {
    for (key, value) in source {
        let field = if prefix.is_empty() {
            key.clone()
        } else {
            format!("{prefix}.{key}")
        };
        if let Value::Object(source) = value {
            let entry = target.entry(key.clone()).or_insert_with(|| json!({}));
            if !entry.is_object() {
                *entry = json!({});
            }
            merge(
                entry.as_object_mut().expect("object entry"),
                source,
                origin,
                origins,
                &field,
            );
        } else {
            target.insert(key.clone(), value.clone());
            origins.insert(field, json!(origin));
        }
    }
}

/// Settings and value origins: bundled < user preferences < project < task overrides.
pub fn effective_settings(
    project: &Value,
    preferences: &Value,
    overrides: &Value,
) -> Result<Value> {
    validate_project(project)?;
    let defaults = default_settings();
    validate_settings(preferences, &defaults, "preferences")?;
    validate_settings(overrides, &defaults, "overrides")?;
    let (mut settings, mut origins) = (Map::new(), Map::new());
    merge(
        &mut settings,
        defaults.as_object().expect("defaults"),
        "bundled",
        &mut origins,
        "",
    );
    if project.pointer("/execution/profiles").is_none() {
        merge(
            &mut settings,
            default_execution().as_object().expect("execution"),
            "bundled",
            &mut origins,
            "",
        );
    }
    for (source, origin) in [
        (preferences, "user"),
        (project, "project"),
        (overrides, "task"),
    ] {
        merge(
            &mut settings,
            source.as_object().expect("validated object"),
            origin,
            &mut origins,
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
    Ok(json!({
        "settings": settings,
        "origins": origins,
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
    }))
}

#[cfg(test)]
mod tests;
