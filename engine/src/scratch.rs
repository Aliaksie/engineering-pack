//! Private task scratch and intentionally retained evidence: names, ownership metadata,
//! workspace identity and the retention rules every workspace action applies.
use crate::values::{lower_hex, safe_integer};
use crate::{digest, hash};
use serde_json::{Value, json};

type Result<T = ()> = std::result::Result<T, String>;

const HOUR_MS: i64 = 3_600_000;
const DAY_MS: i64 = 86_400_000;
/// Operations and evidence records one ownership manifest may hold.
pub const MANIFEST_ENTRIES: usize = 64;
pub const MANIFEST_BYTES: usize = 32768;

/// Effective `workspace` settings; validated by settings, so every field is present.
#[derive(Clone, Debug)]
pub struct Policy {
    pub enabled: bool,
    pub max_sessions: i64,
    pub stale_ms: i64,
    pub scratch_bytes: i64,
    pub scratch_files: i64,
    pub evidence_enabled: bool,
    pub retention_ms: i64,
    pub evidence_artifacts: i64,
    pub evidence_bytes: i64,
}

impl Policy {
    pub fn from_settings(settings: &Value) -> Policy {
        let policy = &settings["workspace"];
        let number = |pointer: &str| policy.pointer(pointer).and_then(safe_integer).unwrap_or(0);
        Policy {
            enabled: policy["enabled"] == true,
            max_sessions: number("/maxSessions"),
            stale_ms: number("/scratch/staleAfterHours").saturating_mul(HOUR_MS),
            scratch_bytes: number("/scratch/maxBytes"),
            scratch_files: number("/scratch/maxFiles"),
            evidence_enabled: policy["evidence"]["enabled"] == true,
            retention_ms: number("/evidence/retentionDays").saturating_mul(DAY_MS),
            evidence_artifacts: number("/evidence/maxArtifacts"),
            evidence_bytes: number("/evidence/maxBytes"),
        }
    }
}

/// Whether the process that recorded an owner still runs. Platforms that cannot tell
/// report `Unknown`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Liveness {
    Live,
    Dead,
    Unknown,
}

/// Open operations hold the workspace until it goes stale. An operation's owner is its
/// session/task/operation identity, not a process: every `craft` call exits at once, so a
/// recorded pid that still answers can only be a reused one.
pub fn active(manifest: &Value, now: i64, policy: &Policy) -> bool {
    !open_operations(manifest).is_empty() && !stale(manifest, now, policy)
}

fn age(manifest: &Value, now: i64) -> i64 {
    now - manifest["updated"].as_i64().unwrap_or(0)
}

fn stale(manifest: &Value, now: i64, policy: &Policy) -> bool {
    age(manifest, now) >= policy.stale_ms
}

/// Scratch of a closed or stale workspace without a live owner is disposable.
pub fn scratch_expired(manifest: &Value, now: i64, policy: &Policy) -> bool {
    manifest["closed"] == true || stale(manifest, now, policy)
}

pub fn evidence_expired(manifest: &Value, now: i64, policy: &Policy) -> bool {
    age(manifest, now) >= policy.retention_ms
}

/// A safe single path segment: `[A-Za-z0-9][A-Za-z0-9._-]{0,127}`.
pub fn is_name(value: &Value) -> bool {
    value.as_str().is_some_and(|name| {
        let bytes = name.as_bytes();
        (1..=128).contains(&bytes.len())
            && bytes[0].is_ascii_alphanumeric()
            && bytes
                .iter()
                .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(b))
    })
}

/// Hashed directory identity of one session/task/operation scope. Repository selection
/// participates in the task digest, so a rebound task never reuses earlier scratch.
#[derive(Clone, Debug)]
pub struct Identity {
    pub session: String,
    pub task: String,
    pub operation: String,
}

impl Identity {
    pub fn new(
        session: &str,
        task: &str,
        repositories: Option<&Value>,
        operation: Option<&str>,
    ) -> Identity {
        let task = match repositories {
            Some(repositories) => digest(&json!([task, repositories])),
            None => digest(&json!(task)),
        };
        Identity {
            session: digest(&json!(session)),
            task,
            operation: digest(&json!(operation.unwrap_or("main"))),
        }
    }

    /// The workspace directory name shared by every operation of the task.
    pub fn key(&self) -> String {
        key(&self.session, &self.task)
    }
}

fn key(session: &str, task: &str) -> String {
    hash(format!("{session}:{task}").as_bytes())
}

/// A new ownership manifest for `identity` in the project `project`.
pub fn manifest(project: &str, identity: &Identity, now: i64) -> Value {
    json!({"owner": "craft", "formatVersion": 1, "project": project, "session": identity.session,
        "task": identity.task, "updated": now, "closed": false, "operations": {}, "evidence": []})
}

/// Validate a manifest read from the workspace directory `entry` of `project`.
pub fn validate(value: &Value, project: &str, entry: &str) -> Result {
    let invalid = || "workspace ownership metadata is invalid".to_string();
    let digest_field = |name: &str| value[name].as_str().filter(|v| lower_hex(v, 64));
    let (Some(session), Some(task)) = (digest_field("session"), digest_field("task")) else {
        return Err(invalid());
    };
    let natural = |value: &Value, minimum: i64| safe_integer(value).is_some_and(|n| n >= minimum);
    let operations = value["operations"].as_object();
    let evidence = value["evidence"].as_array();
    let valid = value["owner"] == "craft"
        && safe_integer(&value["formatVersion"]) == Some(1)
        && value["project"] == project
        && key(session, task) == entry
        && natural(&value["updated"], 0)
        && value["closed"].is_boolean()
        && evidence.is_some_and(|items| {
            items.len() <= MANIFEST_ENTRIES
                && items.iter().all(|item| {
                    is_name(&item["file"])
                        && safe_integer(&item["at"]).is_some()
                        && natural(&item["bytes"], 0)
                })
        })
        && operations.is_some_and(|operations| {
            operations.len() <= MANIFEST_ENTRIES
                && operations.iter().all(|(id, operation)| {
                    lower_hex(id, 64)
                        && natural(&operation["pid"], 1)
                        && operation["closed"].is_boolean()
                })
        });
    if valid { Ok(()) } else { Err(invalid()) }
}

/// Operations not yet closed.
pub fn open_operations(manifest: &Value) -> Vec<String> {
    manifest["operations"]
        .as_object()
        .into_iter()
        .flatten()
        .filter(|(_, operation)| operation["closed"] == false)
        .map(|(id, _)| id.clone())
        .collect()
}

/// A managed scratch write: `{name, content}` with a name of at most 100 characters.
pub fn artifact(input: &Value) -> Result<(&str, &str)> {
    match (input["name"].as_str(), input["content"].as_str()) {
        (Some(name), Some(content)) if is_name(&input["name"]) && name.len() <= 100 => {
            Ok((name, content))
        }
        _ => Err(
            "workspace write requires a safe filename of at most 100 characters and text".into(),
        ),
    }
}

/// An evidence promotion `{name, purpose}` while the manifest has room for one more record.
pub fn promotion<'a>(input: &'a Value, manifest: &Value, policy: &Policy) -> Result<&'a str> {
    let registered = manifest["evidence"].as_array().map_or(0, Vec::len) as i64;
    match input["name"].as_str() {
        Some(name)
            if is_name(&input["name"])
                && name.len() <= 100
                && is_name(&input["purpose"])
                && registered < policy.evidence_artifacts =>
        {
            Ok(name)
        }
        _ => Err("invalid promotion or evidence count exceeded".into()),
    }
}

/// Retained evidence is named after its operation scope so scopes never collide.
pub fn evidence_name(operation: &str, name: &str) -> String {
    format!("{}-{name}", &operation[..16])
}

#[cfg(test)]
mod tests;
