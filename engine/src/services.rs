//! External service operations: invocation plans and session-level availability metadata
//! over validated effective settings. Plans describe what a host may run; nothing here
//! executes, discovers tools or grants authorization.
use crate::{
    contracts::{READ_OPERATIONS, WRITE_OPERATIONS},
    values::safe_integer,
};
use serde_json::{Map, Value, json};
use std::collections::BTreeSet;

type Result<T> = std::result::Result<T, String>;

const SERVICES: [(&str, &str); 3] = [("git", "git."), ("codehost", "change."), ("tasks", "task.")];

fn operations() -> impl Iterator<Item = &'static str> {
    READ_OPERATIONS.iter().chain(WRITE_OPERATIONS).copied()
}

fn service_of(operation: &str) -> &'static str {
    SERVICES
        .iter()
        .find(|(_, prefix)| operation.starts_with(prefix))
        .map_or("codehost", |(service, _)| service)
}

/// The project binding, or the local Git CLI when Git is not configured.
fn binding(settings: &Value, service: &str) -> Option<(Value, bool)> {
    match settings.pointer(&format!("/services/{service}")) {
        Some(binding) => Some((binding.clone(), true)),
        None if service == "git" => Some((
            json!({"provider": "git", "transport": "cli", "command": "git"}),
            false,
        )),
        None => None,
    }
}

fn write_enabled(settings: &Value, operation: &str) -> bool {
    settings
        .pointer("/permissions/writeOperations")
        .and_then(Value::as_array)
        .is_some_and(|enabled| enabled.iter().any(|item| item == operation))
}

enum Mapping<'a> {
    Configured(&'a Value),
    Builtin {
        required: &'static [&'static str],
        optional: &'static [&'static str],
    },
    Missing(String),
}

/// Built-in CLI mappings belong to the service whose binding supplies them: Git
/// operations to a `git` Git binding, change operations to a GitHub or GitLab code host.
fn mapping<'a>(service: &str, binding: &'a Value, operation: &str) -> Mapping<'a> {
    if let Some(mapping) = binding.pointer(&format!("/operations/{operation}")) {
        return Mapping::Configured(mapping);
    }
    let provider = binding["provider"].as_str().unwrap_or_default();
    let builtin = binding["transport"] == "cli"
        && match (service, operation) {
            ("git", "git.status" | "git.diff") => provider == "git",
            ("codehost", "change.read" | "change.diff") => ["github", "gitlab"].contains(&provider),
            _ => false,
        };
    if !builtin {
        return Mapping::Missing(format!("binding has no mapping for {operation}"));
    }
    match operation {
        "git.status" => Mapping::Builtin {
            required: &[],
            optional: &[],
        },
        "git.diff" => Mapping::Builtin {
            required: &[],
            optional: &["base", "head"],
        },
        _ if binding.get("repository").is_none() => {
            Mapping::Missing("codehost repository must be explicit".into())
        }
        _ => Mapping::Builtin {
            required: &["id"],
            optional: &[],
        },
    }
}

/// `String(number)` in JavaScript, for parameters substituted into text and CLI arguments.
fn number_text(number: &serde_json::Number) -> String {
    if number.is_i64() || number.is_u64() {
        return number.to_string();
    }
    let float = number.as_f64().unwrap_or_default();
    let scientific = format!("{float:e}");
    let (mantissa, exponent) = scientific.split_once('e').expect("exponent form");
    let exponent: i32 = exponent.parse().expect("integer exponent");
    if (-6..21).contains(&exponent) {
        format!("{float}")
    } else {
        format!(
            "{mantissa}e{}{exponent}",
            if exponent > 0 { "+" } else { "" }
        )
    }
}

fn scalar_text(value: &Value) -> Option<String> {
    match value {
        Value::String(text) => Some(text.clone()),
        Value::Number(number) => Some(number_text(number)),
        Value::Bool(flag) => Some(flag.to_string()),
        _ => None,
    }
}

/// `{name}` placeholders: `[A-Za-z]` then word characters.
fn placeholders(text: &str) -> Vec<(usize, usize, &str)> {
    let bytes = text.as_bytes();
    let mut found = Vec::new();
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'{' && bytes.get(index + 1).is_some_and(u8::is_ascii_alphabetic) {
            let end = (index + 1..bytes.len())
                .find(|&at| !(bytes[at].is_ascii_alphanumeric() || bytes[at] == b'_'))
                .unwrap_or(bytes.len());
            if bytes.get(end) == Some(&b'}') {
                found.push((index, end + 1, &text[index + 1..end]));
                index = end + 1;
                continue;
            }
        }
        index += 1;
    }
    found
}

fn parameter<'a>(params: &'a Map<String, Value>, name: &str) -> Result<&'a Value> {
    params
        .get(name)
        .ok_or_else(|| format!("missing parameter {name}"))
}

/// A whole-value placeholder keeps the parameter's JSON type; placeholders inside text
/// require scalar parameters rather than stringifying objects, arrays or null.
fn substitute(value: &Value, params: &Map<String, Value>) -> Result<Value> {
    Ok(match value {
        Value::Array(items) => Value::Array(
            items
                .iter()
                .map(|item| substitute(item, params))
                .collect::<Result<_>>()?,
        ),
        Value::Object(map) => Value::Object(
            map.iter()
                .map(|(key, item)| Ok((key.clone(), substitute(item, params)?)))
                .collect::<Result<_>>()?,
        ),
        Value::String(text) => {
            let found = placeholders(text);
            if let [(0, end, name)] = found[..]
                && end == text.len()
            {
                return parameter(params, name).cloned();
            }
            let mut result = String::new();
            let mut last = 0;
            for (start, end, name) in found {
                let value = parameter(params, name)?;
                let text_value = scalar_text(value).ok_or_else(|| {
                    format!("parameter {name} must be a string, number or boolean")
                })?;
                result.push_str(&text[last..start]);
                result.push_str(&text_value);
                last = end;
            }
            result.push_str(&text[last..]);
            Value::String(result)
        }
        other => other.clone(),
    })
}

fn revision(value: &Value) -> Result<&str> {
    value
        .as_str()
        .filter(|text| {
            !text.is_empty()
                && !text.starts_with('-')
                && text
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"_./~^@{}+-".contains(&b))
        })
        .ok_or_else(|| "invalid Git revision".into())
}

/// A positive decimal change number, as a string or a safe integer.
fn change_id(value: Option<&Value>) -> Result<String> {
    let id = match value {
        Some(Value::String(text)) => Some(text.clone()),
        Some(number @ Value::Number(_)) => safe_integer(number).map(|n| n.to_string()),
        _ => None,
    };
    id.filter(|id| {
        id.as_bytes()
            .first()
            .is_some_and(|b| (b'1'..=b'9').contains(b))
            && id.bytes().all(|b| b.is_ascii_digit())
    })
    .ok_or_else(|| "change id must be a positive integer".into())
}

/// Built-in arguments are final: user values are never re-read as placeholders.
fn builtin_args(
    binding: &Value,
    operation: &str,
    params: &Map<String, Value>,
) -> Result<Vec<String>> {
    let text = |items: &[&str]| {
        items
            .iter()
            .map(|item| item.to_string())
            .collect::<Vec<_>>()
    };
    match operation {
        "git.status" => Ok(text(&["status", "--short"])),
        "git.diff" => {
            let mut args = text(&["diff", "--no-ext-diff", "--no-textconv", "--no-color"]);
            if params.contains_key("base") || params.contains_key("head") {
                let base = revision(params.get("base").unwrap_or(&Value::Null))?;
                let head = revision(params.get("head").unwrap_or(&Value::Null))?;
                args.push(format!("{base}...{head}"));
            }
            args.push("--".into());
            Ok(args)
        }
        _ => {
            let id = change_id(params.get("id"))?;
            let github = binding["provider"] == "github";
            let repository = binding["repository"].as_str().unwrap_or_default();
            let repository = match binding["host"].as_str() {
                Some(host) if github => format!("{host}/{repository}"),
                Some(host) => format!("https://{host}/{repository}"),
                None => repository.to_string(),
            };
            let read = operation == "change.read";
            let mut args = text(&[
                if github { "pr" } else { "mr" },
                if read { "view" } else { "diff" },
            ]);
            args.extend([id, "--repo".into(), repository]);
            if read {
                args.extend(text(if github {
                    &["--json", "number,title,body,baseRefOid,headRefOid"]
                } else {
                    &["--output", "json"]
                }));
            }
            Ok(args)
        }
    }
}

/// An invocation plan for one operation; the host runs it, if at all, under its own permissions.
pub fn plan(settings: &Value, operation: &str, params: &Value) -> Result<Value> {
    if !operations().any(|known| known == operation) {
        return Err(format!("unknown operation: {operation}"));
    }
    let params = params
        .as_object()
        .ok_or("operation parameters must be an object")?;
    let service = service_of(operation);
    let (binding, _) = binding(settings, service)
        .ok_or_else(|| format!("missing {service} binding for {operation}"))?;
    let write = WRITE_OPERATIONS.contains(&operation);
    if write && !write_enabled(settings, operation) {
        return Err(format!(
            "write operation not enabled by project policy: {operation}"
        ));
    }
    let resolved = mapping(service, &binding, operation);
    let mut result = json!({
        "operation": operation,
        "service": service,
        "provider": binding["provider"],
        "transport": binding["transport"],
        "effect": if write { "write" } else { "read" },
        "authorization": if write { "explicit-user-or-job-approval-required" } else { "host-permissions-apply" },
        "availability": "not-verified",
        "output": if operation.ends_with(".diff") { "save-to-artifact" } else { "bounded-result" },
    });
    let args = match resolved {
        Mapping::Missing(reason) => return Err(reason),
        Mapping::Configured(mapping) if binding["transport"] == "mcp" => {
            result["tool"] = mapping["tool"].clone();
            result["arguments"] = substitute(&mapping["arguments"], params)?;
            return Ok(result);
        }
        Mapping::Configured(mapping) => match substitute(&mapping["args"], params)? {
            Value::Array(args) => args
                .iter()
                .map(scalar_text)
                .collect::<Option<Vec<_>>>()
                .ok_or("CLI arguments must be scalar values")?,
            _ => return Err("CLI arguments must be scalar values".into()),
        },
        Mapping::Builtin { .. } => builtin_args(&binding, operation, params)?,
    };
    result["command"] = binding["command"].clone();
    result["args"] = json!(args);
    result["shell"] = json!(false);
    Ok(result)
}

fn names(value: Option<&Value>, label: &str) -> Result<Option<BTreeSet<String>>> {
    let Some(value) = value else {
        return Ok(None);
    };
    value
        .as_array()
        .and_then(|items| {
            items
                .iter()
                .map(|item| {
                    item.as_str()
                        .filter(|name| !name.trim().is_empty())
                        .map(str::to_string)
                })
                .collect::<Option<BTreeSet<_>>>()
        })
        .map(Some)
        .ok_or_else(|| format!("{label} must be an array of tool or command names"))
}

/// Placeholder names a configured mapping requires, sorted and unique.
fn required(value: &Value, found: &mut BTreeSet<String>) {
    match value {
        Value::Array(items) => items.iter().for_each(|item| required(item, found)),
        Value::Object(map) => map.values().for_each(|item| required(item, found)),
        Value::String(text) => found.extend(
            placeholders(text)
                .into_iter()
                .map(|(_, _, name)| name.to_string()),
        ),
        _ => {}
    }
}

/// A session-level metadata summary, not a tool discovery request or authorization check.
pub fn prepare(
    settings: &Value,
    available_commands: Option<&Value>,
    available_mcp_tools: Option<&Value>,
) -> Result<Value> {
    let cli = names(available_commands, "availableCommands")?;
    let mcp = names(available_mcp_tools, "availableMcpTools")?;
    let services = SERVICES.iter().map(|&(service, prefix)| {
        let bound = binding(settings, service);
        let operations: Vec<Value> = operations()
            .filter(|operation| operation.starts_with(prefix))
            .map(|operation| {
                let write = WRITE_OPERATIONS.contains(&operation);
                let mut entry = json!({
                    "operation": operation,
                    "effect": if write { "write" } else { "read" },
                    "projectPolicy": match (write, write_enabled(settings, operation)) {
                        (false, _) => "read",
                        (true, true) => "enabled",
                        (true, false) => "disabled",
                    },
                    "authorization": if write { "explicit-user-or-job-approval-required" } else { "host-permissions-apply" },
                });
                let missing = |entry: &mut Value, reason: String| {
                    entry["mapping"] = json!("missing");
                    entry["availability"] = json!("not-applicable");
                    entry["missing"] = json!(reason);
                };
                let Some((binding, _)) = &bound else {
                    missing(&mut entry, format!("missing {service} binding"));
                    return entry;
                };
                let cli_transport = binding["transport"] == "cli";
                let (kind, name, parameters, optional) = match mapping(service, binding, operation) {
                    Mapping::Missing(reason) => {
                        missing(&mut entry, reason);
                        return entry;
                    }
                    Mapping::Configured(mapping) => {
                        let mut found = BTreeSet::new();
                        required(&mapping[if cli_transport { "args" } else { "arguments" }], &mut found);
                        let name = if cli_transport { &binding["command"] } else { &mapping["tool"] };
                        ("configured", name.clone(), json!(found), &[][..])
                    }
                    Mapping::Builtin { required, optional } => {
                        ("builtin", binding["command"].clone(), json!(required), optional)
                    }
                };
                entry["mapping"] = json!(kind);
                entry[if cli_transport { "command" } else { "tool" }] = name.clone();
                entry["requiredParameters"] = parameters;
                if !optional.is_empty() {
                    entry["optionalParameters"] = json!(optional);
                }
                let inventory = if cli_transport { &cli } else { &mcp };
                entry["availability"] = json!(match inventory {
                    None => "unverified",
                    Some(names) if name.as_str().is_some_and(|name| names.contains(name)) => "observed-available",
                    Some(_) => "not-observed",
                });
                entry
            })
            .collect();
        let mut summary = json!({ "service": service, "configured": bound.is_some() });
        if let Some((binding, explicit)) = &bound {
            let mut scope = Map::new();
            for field in ["host", "repository"] {
                if let Some(value) = binding.get(field) {
                    scope.insert(field.into(), value.clone());
                }
            }
            scope.insert("account".into(), json!("unverified"));
            summary["source"] = json!(if *explicit { "project" } else { "local-default" });
            summary["provider"] = binding["provider"].clone();
            summary["transport"] = binding["transport"].clone();
            summary["scope"] = Value::Object(scope);
        }
        summary["operations"] = Value::Array(operations);
        summary
    });
    Ok(json!({
        "services": services.collect::<Vec<_>>(),
        "reuse": {
            "prepare": "once-per-session-or-first-service-need",
            "refreshOn": ["project-binding-change", "tool-connection-change", "account-or-scope-change", "relevant-operation-failure"],
            "limits": "Reuse metadata between turns. Tool names do not establish account identity, authorization, or fresh remote state."
        }
    }))
}

#[cfg(test)]
mod tests;
