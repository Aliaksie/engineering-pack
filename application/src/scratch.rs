//! Private task scratch and retained evidence (`workspace-*`): the verified task binding,
//! the hashed workspace identity and each action under the shared workspace lock.
use crate::failure::Failure;
use crate::session;
use crate::telemetry::{Operation, Recorder, now_ms};
use craft_engine::scratch::{self, Identity, MANIFEST_BYTES, MANIFEST_ENTRIES, Policy};
use craft_platform::repository::snapshot;
use craft_platform::scratch::{self as files, Usage};
use craft_platform::state;
use serde_json::{Map, Value, json};
use std::io;
use std::path::{Path, PathBuf};

const SESSION_ENTRIES: usize = 64;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Open,
    Write,
    Promote,
    Release,
    Maintain,
    /// Read-only usage of the task's workspace for session status.
    Status,
    /// A task step: refresh the retention clock of an open workspace.
    Touch,
    /// Task completion: close the workspace and its main operation; scratch is cleaned once no
    /// other operation of the task is still open.
    Complete,
}

impl Action {
    fn name(self) -> &'static str {
        match self {
            Action::Open => "open",
            Action::Write => "write",
            Action::Promote => "promote",
            Action::Release => "release",
            Action::Maintain => "maintain",
            Action::Status => "status",
            Action::Touch => "touch",
            Action::Complete => "complete",
        }
    }

    fn creates(self) -> bool {
        matches!(self, Action::Open | Action::Write)
    }
}

pub struct Request<'a> {
    pub root: &'a Path,
    pub session: Option<&'a str>,
    pub task: Option<&'a str>,
    pub action: Action,
    pub input: &'a Value,
    pub settings: &'a Value,
}

/// Run one workspace action for the explicit or bound task. Maintenance needs no task.
pub fn run(
    request: Request,
    recorder: &mut Recorder,
    parent: &Operation,
) -> Result<Value, Failure> {
    let Request {
        root,
        session,
        task,
        action,
        input,
        settings,
    } = request;
    let policy = Policy::from_settings(settings);
    if !policy.enabled {
        return Ok(json!({"status": "disabled"}));
    }
    let session = session.filter(|s| !s.is_empty());
    let selected = match task {
        Some(task) => Some(task.to_string()),
        None => session::active_task(root, session)?,
    }
    .filter(|task| !task.is_empty());
    if action != Action::Maintain && (session.is_none() || selected.is_none()) {
        return Ok(
            json!({"status": "unavailable", "reason": "A verified session and task binding are required."}),
        );
    }
    let bound = session.zip(selected.as_deref());
    if let Some((session, task)) = bound {
        craft_engine::session::identity(&json!(session), &json!(task))?;
    }
    if matches!(
        action,
        Action::Open | Action::Write | Action::Promote | Action::Release
    ) && session::active_task(root, session)? != selected
    {
        return Err("workspace writes require the active task binding".into());
    }
    let operation = input.get("operation");
    if let Some(operation) = operation {
        craft_engine::session::identity(&session.map_or(Value::Null, |s| json!(s)), operation)?;
    }
    let repositories = |recorder: &mut Recorder| -> Result<Option<Value>, Failure> {
        let (session, task) = bound.expect("bound task");
        let state = session::read_state(root, session, task, recorder, parent)?;
        Ok(state["taskRecord"]
            .get("repositories")
            .filter(|r| !r.is_null())
            .cloned())
    };
    let identity = match bound {
        Some((session, task)) => Some(Identity::new(
            session,
            task,
            repositories(recorder)?.as_ref(),
            operation.and_then(Value::as_str),
        )),
        None => None,
    };
    // Promotion records the task source it saw; creation-time source is never inferred.
    let source = if action == Action::Promote {
        let repositories = repositories(recorder)?;
        let repository = snapshot::workspace(root, repositories.as_ref(), false)?;
        craft_engine::session::source(&repository)
    } else {
        Value::Null
    };
    let started = now_ms();
    let result = operate(
        root,
        identity.as_ref(),
        &policy,
        action,
        input,
        source,
        started,
    )?;
    let cleaned = result["cleaned"].as_array().map_or(&[][..], Vec::as_slice);
    let retained = result["retained"].as_array().map_or(&[][..], Vec::as_slice);
    if action != Action::Status
        && (result["status"] == "available" || !cleaned.is_empty() || !retained.is_empty())
    {
        let mut entries: Vec<_> = cleaned.iter().map(|item| &item["entry"]).collect();
        entries.sort_by_key(|entry| entry.as_str());
        entries.dedup();
        let count = |value: &Value| value.as_u64().unwrap_or(0);
        let attributes = Map::from_iter([
            ("workspaceAction".to_string(), json!(action.name())),
            (
                "scratchBytes".to_string(),
                json!(count(&result["scratchUsage"]["bytes"])),
            ),
            (
                "evidenceBytes".to_string(),
                json!(count(&result["evidence"]["bytes"])),
            ),
            (
                "evidenceCount".to_string(),
                json!(count(&result["evidence"]["artifacts"])),
            ),
            ("workspacesCleaned".to_string(), json!(entries.len())),
            ("complete".to_string(), json!(retained.is_empty())),
        ]);
        let outcome = if retained.is_empty() {
            "success"
        } else {
            "deferred"
        };
        recorder.observed(
            "workspace.observed",
            attributes,
            parent,
            outcome,
            now_ms() - started,
        );
    }
    Ok(result)
}

fn io(error: io::Error) -> Failure {
    Failure::from(error)
}

fn read_manifest(file: &Path, project: &str, entry: &str) -> Result<Value, Failure> {
    let value = session::read_record(file, MANIFEST_BYTES)?;
    scratch::validate(&value, project, entry)?;
    Ok(value)
}

fn usage_json(usage: Usage) -> Value {
    json!({"bytes": usage.bytes, "files": usage.files})
}

struct Owned {
    entry: String,
    folder: PathBuf,
    manifest: Value,
}

/// All cooperating writers and cleanup serialize on one project-private lock. Calls with the
/// same session/task/operation identity are one owner. Routine cleanup keeps workspaces with
/// open operations until stale; capacity may evict the oldest open one of another task.
fn operate(
    root: &Path,
    identity: Option<&Identity>,
    policy: &Policy,
    action: Action,
    input: &Value,
    source: Value,
    now: i64,
) -> Result<Value, Failure> {
    let anchor = state::file(root, "workspace-guard.json", action.creates(), None)?;
    let sessions = anchor.with_file_name("sessions");
    if !files::directory(&sessions, action.creates()).map_err(io)? {
        return Ok(
            json!({"status": "absent", "scratch": "absent", "evidence": {"artifacts": 0, "bytes": 0}}),
        );
    }
    let Some(_lock) = files::lock(&anchor).map_err(io)? else {
        return Ok(json!({"status": "retained", "reason": "workspace is in use"}));
    };
    let project = craft_engine::hash(state::canonical_root(root)?.to_string_lossy().as_bytes());
    let mut entries = files::names(&sessions, SESSION_ENTRIES).map_err(io)?;
    entries.sort_by(|a, b| a.encode_utf16().cmp(b.encode_utf16()));
    let (mut cleaned, mut retained) = (Vec::new(), Vec::new());
    let mut owned = Vec::new();
    for entry in &entries {
        let folder = sessions.join(entry);
        let checked = if craft_engine::values::lower_hex(entry, 64) {
            files::directory(&folder, false)
                .map_err(io)
                .and_then(|_| read_manifest(&folder.join("owner.json"), &project, entry))
        } else {
            Err("unknown workspace entry".into())
        };
        match checked {
            Ok(manifest) => owned.push(Owned {
                entry: entry.clone(),
                folder,
                manifest,
            }),
            Err(failure) => retained.push(json!({"entry": entry, "reason": failure.message})),
        }
    }
    let key = identity.map(Identity::key);
    let removed = |cleaned: &[Value]| {
        cleaned
            .iter()
            .filter(|item| item["removed"] == true)
            .count() as i64
    };
    let total = entries.len() as i64;
    if matches!(action, Action::Maintain | Action::Open | Action::Write) {
        // A new workspace needs one free slot; unverified entries occupy capacity.
        let room =
            i64::from(action.creates() && key.as_ref().is_some_and(|key| !entries.contains(key)));
        owned.sort_by_key(|item| item.manifest["updated"].as_i64().unwrap_or(0));
        let over = |cleaned: &[Value]| total - removed(cleaned) + room > policy.max_sessions;
        // Inactive workspaces first; an open one of another task only when capacity requires.
        for evict_open in [false, true] {
            for item in &owned {
                if key.as_deref() == Some(item.entry.as_str())
                    || scratch::active(&item.manifest, now, policy) != evict_open
                    || (evict_open && !over(&cleaned))
                {
                    continue;
                }
                if let Err(error) = sweep(item, now, policy, &mut cleaned, over) {
                    retained.push(json!({"entry": item.entry, "reason": error.to_string()}));
                }
            }
        }
    }
    let Some(identity) = identity else {
        return Ok(json!({"status": "inspected", "cleaned": cleaned, "retained": retained}));
    };
    let key = identity.key();
    let folder = sessions.join(&key);
    let owner = folder.join("owner.json");
    if !files::directory(&folder, false).map_err(io)? {
        if !action.creates() {
            return Ok(json!({"status": "absent", "cleaned": cleaned, "retained": retained}));
        }
        if total - removed(&cleaned) >= policy.max_sessions {
            return Err(
                "workspace session capacity reached; active or unverified workspaces were retained"
                    .into(),
            );
        }
        files::directory(&folder, true).map_err(io)?;
        state::write(
            &owner,
            &scratch::manifest(&project, identity, now),
            MANIFEST_BYTES,
            true,
        )?;
    }
    let mut value = read_manifest(&owner, &project, &key)?;
    if value["session"] != identity.session.as_str() || value["task"] != identity.task.as_str() {
        return Err("workspace identity mismatch".into());
    }
    let scratch_dir = folder.join("scratch");
    let evidence = folder.join("evidence");
    let scope = scratch_dir.join(&identity.operation);
    let current = value["operations"].get(&identity.operation).cloned();
    if action.creates() {
        let operations = value["operations"]
            .as_object_mut()
            .expect("validated manifest");
        if !operations.contains_key(&identity.operation) && operations.len() >= MANIFEST_ENTRIES {
            return Err("workspace operation capacity reached".into());
        }
        files::directory(&scratch_dir, true).map_err(io)?;
        files::directory(&scope, true).map_err(io)?;
        // Format 1 requires a pid; it records the writer and decides nothing.
        operations.insert(
            identity.operation.clone(),
            json!({"pid": std::process::id(), "closed": false}),
        );
        value["closed"] = json!(false);
    }
    if action == Action::Write {
        let (name, content) = scratch::artifact(input)?;
        let used = files::usage(&scratch_dir).map_err(io)?;
        let bytes = content.len() as u64;
        if used.files as i64 >= policy.scratch_files
            || (used.bytes + bytes) as i64 > policy.scratch_bytes
        {
            return Err("workspace scratch budget exceeded".into());
        }
        files::create(&scope.join(name), content.as_bytes()).map_err(io)?;
    }
    if action == Action::Promote {
        if !policy.evidence_enabled {
            return Ok(json!({"status": "disabled", "reason": "evidence retention is disabled"}));
        }
        let name = scratch::promotion(input, &value, policy)?;
        files::directory(&scratch_dir, false).map_err(io)?;
        files::directory(&scope, false).map_err(io)?;
        files::directory(&evidence, true).map_err(io)?;
        let target = scratch::evidence_name(&identity.operation, name);
        let admit = |size: u64| {
            let used = files::usage(&evidence)?;
            if used.files as i64 >= policy.evidence_artifacts
                || (used.bytes + size) as i64 > policy.evidence_bytes
            {
                return Err(io::Error::other("unsafe or oversized evidence"));
            }
            Ok(())
        };
        let bytes = files::copy(&scope.join(name), &evidence.join(&target), admit).map_err(io)?;
        value["evidence"]
            .as_array_mut()
            .expect("validated manifest")
            .push(json!({
                "file": target, "at": now, "bytes": bytes, "purpose": input["purpose"],
                "operation": identity.operation, "sourceAtPromotion": source,
            }));
    }
    let mut cleanup = "retained";
    if matches!(action, Action::Release | Action::Complete) {
        if action == Action::Complete {
            value["closed"] = json!(true);
        }
        if current.is_some() {
            value["operations"][&identity.operation]["closed"] = json!(true);
        }
        if scratch::open_operations(&value).is_empty() {
            match files::remove(&scratch_dir) {
                Ok(()) => cleanup = "cleaned",
                Err(error) => retained.push(json!({"entry": key, "reason": error.to_string()})),
            }
        }
    }
    if action != Action::Status {
        // Retention counts from completion: a step never refreshes a closed workspace.
        if action != Action::Touch || value["closed"] != true {
            value["updated"] = json!(now);
        }
        state::write(&owner, &value, MANIFEST_BYTES, true)?;
    }
    let scratch_usage = files::usage(&scratch_dir).map_err(io)?;
    let evidence_usage = files::usage(&evidence).map_err(io)?;
    let scratch_state = if files::directory(&scratch_dir, false).map_err(io)? {
        cleanup
    } else {
        "cleaned"
    };
    let manifest_bytes = std::fs::metadata(&owner).map_err(io)?.len();
    Ok(json!({
        "status": "available",
        "directory": folder.to_string_lossy(),
        "scratch": scratch_state,
        "scratchDirectory": scope.to_string_lossy(),
        "scratchUsage": usage_json(scratch_usage),
        "evidence": {
            "artifacts": evidence_usage.files,
            "registeredArtifacts": value["evidence"].as_array().map_or(0, Vec::len),
            "bytes": evidence_usage.bytes,
            "files": evidence_usage.files,
        },
        "state": "existing checkpoint/session stores",
        "diskBytes": scratch_usage.bytes + evidence_usage.bytes + manifest_bytes,
        "active": scratch::active(&value, now, policy),
        "cleaned": cleaned,
        "retained": retained,
    }))
}

/// Reclaim one inactive workspace: disposable scratch, then the whole directory when its
/// evidence expired or capacity is exceeded (`over`).
fn sweep(
    item: &Owned,
    now: i64,
    policy: &Policy,
    cleaned: &mut Vec<Value>,
    over: impl Fn(&[Value]) -> bool,
) -> io::Result<()> {
    let scratch_dir = item.folder.join("scratch");
    if scratch::scratch_expired(&item.manifest, now, policy)
        && files::directory(&scratch_dir, false)?
    {
        files::remove(&scratch_dir)?;
        cleaned.push(json!({"entry": item.entry, "scratch": "cleaned"}));
    }
    if scratch::evidence_expired(&item.manifest, now, policy) || over(cleaned) {
        let known = ["owner.json", "scratch", "evidence"];
        if files::names(&item.folder, usize::MAX)?
            .iter()
            .any(|name| !known.contains(&name.as_str()))
        {
            return Err(io::Error::other("unknown workspace files; cleanup refused"));
        }
        files::remove(&item.folder)?;
        cleaned.push(json!({"entry": item.entry, "removed": true}));
    }
    Ok(())
}
