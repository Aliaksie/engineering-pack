//! Context delivery hooks: lifecycle facts → routing hints, checkpoints, service metadata and
//! workspace housekeeping, with identical routing hints suppressed under one locked step.
use crate::checkpoint;
use crate::failure::Failure;
use crate::project;
use crate::scratch::{self, Action};
use crate::session::{active_task, read_record};
use crate::telemetry::{Operation, Recorder, now_ms};
use craft_engine::delivery::{self, answer};
use craft_engine::{hash, identity, routing};
use craft_platform::{state, telemetry::uuid};
use serde_json::{Map, Value, json};
use std::path::{Component, Path, PathBuf};

const HINT_BYTES: usize = 2048;
const INTENT_BYTES: usize = 16384;

/// One delivery hook event, in host-neutral facts.
pub struct Event<'a> {
    pub root: &'a Path,
    pub cwd: &'a Path,
    pub host: &'a str,
    pub lifecycle: &'a str,
    pub session: Option<&'a str>,
    /// The tool writes a file.
    pub edit: bool,
    /// This tool hook delivers an edit's context on the host.
    pub edit_context: bool,
    pub file: Option<&'a str>,
    pub prompt: Option<&'a str>,
    pub context_supported: bool,
}

/// The project-relative form of a tool path, resolved against the hook's working
/// directory; `None` outside the project, `Some("")` for no file.
fn project_file(root: &Path, cwd: &Path, file: Option<&str>) -> Option<String> {
    let Some(file) = file.filter(|f| !f.is_empty()) else {
        return Some(String::new());
    };
    let mut resolved = PathBuf::new();
    for component in cwd.join(file).components() {
        match component {
            Component::ParentDir => {
                resolved.pop();
            }
            Component::CurDir => {}
            other => resolved.push(other),
        }
    }
    let relative = resolved.strip_prefix(root).ok()?;
    let parts: Vec<_> = relative
        .components()
        .map(|c| c.as_os_str().to_string_lossy())
        .collect();
    Some(parts.join("/"))
}

/// The hook decision `{decision, context?, warning?}` for one delivery event.
pub fn run(event: &Event, recorder: &mut Recorder, hook: &Operation) -> Result<Value, Failure> {
    let root = event.root;
    let settings = project::loaded(recorder, hook, root, None)?;
    let session = event.session.filter(|s| !s.is_empty());
    let task = active_task(root, session).unwrap_or_else(|_| {
        recorder.report("state", "STATE_UNAVAILABLE", Map::new(), hook);
        None
    });
    let source = if task.is_some() {
        "session-binding"
    } else {
        "unavailable"
    };
    let task_identity = json!({"session": event.session, "task": task, "source": source});
    let housekeeping = match event.lifecycle {
        "session-start" => Some(Action::Maintain),
        "before-prompt" if task.is_some() => Some(Action::Touch),
        _ => None,
    };
    if let Some(action) = housekeeping {
        let request = scratch::Request {
            root,
            session,
            task: task.as_deref(),
            action,
            input: &json!({}),
            settings: &settings,
        };
        // Optional housekeeping never prevents delivery.
        if scratch::run(request, recorder, hook).is_err() {
            recorder.report("state", "STATE_UNAVAILABLE", Map::new(), hook);
        }
    }
    let prompt = (event.lifecycle == "before-prompt").then(|| event.prompt.unwrap_or(""));
    // An edit outside the project has no project route; it is not a delivery failure.
    let file = project_file(root, event.cwd, event.file);
    let intent_file = match session {
        Some(session) if prompt.is_some() || event.edit || event.lifecycle == "turn-end" => {
            Some(intent_file(root, session, true)?)
        }
        _ => None,
    };
    let record = intent_file.as_deref().map_or(Value::Null, |file| {
        read_intent(file).unwrap_or_else(|_| {
            recorder.report("state", "STATE_UNAVAILABLE", Map::new(), hook);
            Value::Null
        })
    });
    let facts = delivery::Facts {
        session: session.is_some(),
        context_supported: event.context_supported,
        edit: event.edit && event.edit_context,
        established: record["intent"].as_str(),
        file_routed: file
            .as_deref()
            .is_some_and(|file| delivery::file_routed(&record, file)),
        prompt,
        started: event.lifecycle == "session-start",
        context_changing: event.lifecycle == "before-compact",
        context_restored: event.lifecycle == "after-compact",
        turn_finished: event.lifecycle == "turn-end",
    };
    let plan = delivery::plan(&facts, &settings);
    if let (true, Some(session)) = (plan.invalidate_hint, session) {
        invalidate_hint(root, session, event.host, recorder, hook);
    }
    let services = if plan.services {
        Some(crate::integration::prepared(
            &settings, None, None, recorder, hook,
        )?)
    } else {
        None
    };
    let routing = match (plan.intent, file.as_deref()) {
        (Some(intent), Some(file)) => {
            let files = if file.is_empty() {
                vec![]
            } else {
                vec![json!(file)]
            };
            let request = routing::Request {
                intent: Some(intent),
                files: &files,
                risks: &[],
                host: event.host,
                delegate: false,
                task: None,
                verification_mode: None,
                artifact: plan.artifact,
            };
            let mut plan = crate::guidance::route(&settings, &request, recorder, hook)?;
            plan["taskIdentity"] = task_identity;
            // This hook's own executable is the runtime the lookup runs.
            let runtime = std::env::current_exe().ok();
            let runtime = runtime.as_deref().and_then(Path::to_str);
            if let Some(lookup) = delivery::lookup(intent, runtime, &root.to_string_lossy()) {
                plan["lookup"] = lookup;
                // A ready lookup explains itself: naming its skill sent models to read the
                // skill, then the package instructions and setup, before one command.
                if let Some(plan) = plan.as_object_mut() {
                    plan.remove("skills");
                }
                if let Some(answer) = prompt.and_then(|p| prompt_answer(root, p, recorder, hook)) {
                    plan["answer"] = answer;
                }
            }
            Some(plan)
        }
        _ => None,
    };
    // A request establishes its intent; an edit only once it is routed.
    let routed_edit = event.edit && routing.is_some();
    if let (Some(intent), Some(target)) = (plan.establish, intent_file.as_deref())
        && (prompt.is_some() || routed_edit)
    {
        let edited = file.as_deref().filter(|_| routed_edit);
        if write_intent(target, |record| {
            delivery::established(record, intent, edited, now_ms() as u64)
        })
        .is_err()
        {
            recorder.report("state", "STATE_UNAVAILABLE", Map::new(), hook);
        }
    }
    // A file written while the session plans is its planning evidence, by content hash.
    if let (true, "after-tool", Some(target), Some(path)) = (
        event.edit && record["intent"] == "plan",
        event.lifecycle,
        intent_file.as_deref(),
        file.as_deref().filter(|f| !f.is_empty()),
    ) {
        let recorded = content_hash(&root.join(path)).and_then(|hash| match hash {
            Some(hash) => write_intent(target, |record| {
                delivery::planning_recorded(record, path, &hash)
            }),
            None => Ok(()),
        });
        if recorded.is_err() {
            recorder.report("state", "STATE_UNAVAILABLE", Map::new(), hook);
        }
    }
    let mut saved = None;
    if let (true, Some(session)) = (plan.save_checkpoint, session) {
        // Only state and explicit metadata: never prompts, transcripts or tool output.
        let metadata = task.as_ref().map_or(json!({}), |t| json!({"taskId": t}));
        let checkpoint = checkpoint::save(root, session, &metadata, None, recorder, hook)?;
        // Whatever tool wrote it (an edit, a shell redirect, a script), a file the turn
        // changed after the plan was requested is the session's planning evidence.
        if let (true, Some(target), Some(since)) = (
            event.lifecycle == "turn-end" && record["intent"] == "plan",
            intent_file.as_deref(),
            record["since"].as_u64(),
        ) && planning_written(root, target, &checkpoint["state"]["status"], since).is_err()
        {
            recorder.report("state", "STATE_UNAVAILABLE", Map::new(), hook);
        }
    }
    if let Some(session) = session {
        if plan.recover_checkpoint {
            saved = Some(checkpoint::recover(root, session, recorder, hook)?);
        } else if plan.read_checkpoint {
            saved = Some(json!({"saved": checkpoint::read(root, session, recorder, hook)?}));
        }
    }
    let mut decision = delivery::decision(
        routing.as_ref(),
        saved.as_ref(),
        services.as_ref(),
        event.lifecycle == "before-compact",
    );
    // The session identity a started or restored session needs for task commands.
    if let (true, Some(session)) = (facts.started || facts.context_restored, session) {
        let mut identity = json!({"id": session, "task": task});
        if plan.offer_handoff && task.is_none() {
            match crate::handoff::offer(root, session) {
                Ok(offered) if !offered.is_empty() => identity["handoff"] = json!(offered),
                Ok(_) => {}
                Err(_) => recorder.report("state", "STATE_UNAVAILABLE", Map::new(), hook),
            }
        }
        if plan.offer_handoff && task.is_none() {
            match earlier_planning(root, session) {
                Ok(found) if !found.is_empty() => identity["planning"] = json!(found),
                Ok(_) => {}
                Err(_) => recorder.report("state", "STATE_UNAVAILABLE", Map::new(), hook),
            }
        }
        if plan.offer_handoff {
            match crate::plan::listing(root, session) {
                Ok(plans) if !plans.is_empty() => identity["plans"] = json!(plans),
                Ok(_) => {}
                Err(_) => recorder.report("state", "STATE_UNAVAILABLE", Map::new(), hook),
            }
        }
        decision["session"] = identity;
    }
    let eligible = routing.is_some()
        && settings["routing"]["dedupeHints"] == true
        && event.context_supported
        && decision.get("context").is_some();
    if let (true, Some(session)) = (eligible, session)
        && let Err(message) = reuse_hint(
            root,
            session,
            event.host,
            &settings,
            &mut decision,
            recorder,
            hook,
        )
    {
        recorder.report("cache", "CACHE_UNAVAILABLE", Map::new(), hook);
        decision["warning"] = json!(format!("Routing hint reuse unavailable: {message}"));
    }
    Ok(decision)
}

/// Index refresh all prompt-time lookups of one prompt may spend together; the prompt waits
/// for it, and sources it defers are searched literally.
const PROMPT_REFRESH_MS: u64 = 1_000;

/// The cited answer to the structural lookup a prompt asks, or `None` when it asks none or
/// the answer is not certain. The lookup is recorded as a discovery under the hook; one that
/// cannot run leaves the prompt with its runnable lookup alone.
fn prompt_answer(
    root: &Path,
    prompt: &str,
    recorder: &mut Recorder,
    hook: &Operation,
) -> Option<Value> {
    let (ask, value) = answer::question(prompt)?;
    let operation = recorder.start("discovery.completed", Some(hook));
    let started = std::time::Instant::now();
    let lookup = |ask: &str, recorder: &mut Recorder| {
        let input = json!({"ask": ask, "value": value, "paths": [], "limit": answer::ROWS});
        let spent = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
        // A later lookup queries the index this prompt just refreshed, so it needs no budget of
        // its own; one millisecond keeps the refresh bounded rather than unlimited.
        let refresh = PROMPT_REFRESH_MS.saturating_sub(spent).max(1);
        crate::question::answer(root, &input, refresh, |request| {
            let context = crate::discovery::Context {
                root,
                session: None,
                task: None,
                preferences: None,
                observed: &json!({}),
            };
            crate::discovery::run(request, context, recorder, &operation)
        })
        .ok()
    };
    let result = lookup(ask, recorder);
    // A cold or partial index cannot answer callers, so the declaration lookup would only
    // add latency to a prompt that gets no answer.
    let declarations = match (ask, &result) {
        ("callers", Some(result)) if answer::answerable(result) => lookup("declaration", recorder),
        _ => None,
    };
    let answered = result
        .as_ref()
        .and_then(|result| answer::answered(ask, &value, result, declarations.as_ref()));
    let rows = answered
        .as_ref()
        .and_then(|a| a["rows"].as_array())
        .map_or(0, Vec::len);
    let attributes = Map::from_iter([
        ("evidenceCount".to_string(), json!(rows)),
        ("complete".to_string(), json!(answered.is_some())),
        (
            "reason".to_string(),
            json!(match (&result, &answered) {
                (None, _) => "unavailable",
                (_, None) => "incomplete",
                _ => "observed",
            }),
        ),
    ]);
    recorder.complete(operation, attributes, None);
    answered
}

fn hint_file(root: &Path, session: &str, host: &str) -> Result<PathBuf, Failure> {
    let name = format!("routing-{}.json", &identity(&[host, session])[..24]);
    Ok(state::file(root, &name, true, Some(session))?)
}

fn intent_file(root: &Path, session: &str, create: bool) -> Result<PathBuf, Failure> {
    Ok(state::file(root, "intent.json", create, Some(session))?)
}

/// Bytes hashed for planning evidence; a larger file is not recorded.
const ARTIFACT_BYTES: u64 = 2 * 1024 * 1024;

/// The content hash of a regular file, `None` when it is absent or too large.
fn content_hash(file: &Path) -> Result<Option<String>, Failure> {
    match std::fs::symlink_metadata(file) {
        Ok(meta) if meta.is_file() && meta.len() <= ARTIFACT_BYTES => {
            let bytes = std::fs::read(file).map_err(|e| e.to_string())?;
            Ok(Some(hash(&bytes)))
        }
        Ok(_) => Ok(None),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e.to_string().into()),
    }
}

/// Record as planning evidence each file in a snapshot `status` modified at or after `since`
/// (Unix milliseconds). Paths are relative to the repository root, so a project below it
/// finds none.
fn planning_written(root: &Path, target: &Path, status: &Value, since: u64) -> Result<(), Failure> {
    let mut written = Vec::new();
    for path in delivery::written_paths(status.as_array().map_or(&[], Vec::as_slice)) {
        let file = root.join(&path);
        let modified = std::fs::symlink_metadata(&file)
            .and_then(|meta| meta.modified())
            .ok()
            .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|elapsed| elapsed.as_millis());
        if modified.is_some_and(|ms| ms >= u128::from(since))
            && let Some(hash) = content_hash(&file)?
        {
            written.push((path, hash));
        }
    }
    if written.is_empty() {
        return Ok(());
    }
    write_intent(target, |record| {
        let mut next: Option<Value> = None;
        for (path, hash) in &written {
            if let Some(updated) =
                delivery::planning_recorded(next.as_ref().unwrap_or(record), path, hash)
            {
                next = Some(updated);
            }
        }
        next
    })
}

/// This session's planning evidence, each artifact with whether its file still has the
/// recorded content.
pub fn planning(root: &Path, session: &str) -> Result<Vec<Value>, Failure> {
    let record = read_intent(&intent_file(root, session, false)?)?;
    planning_of(root, &record)
}

fn planning_of(root: &Path, record: &Value) -> Result<Vec<Value>, Failure> {
    let entries = record["planning"].as_array().cloned().unwrap_or_default();
    entries
        .into_iter()
        .map(|mut entry| {
            let path = entry["path"].as_str().unwrap_or_default();
            let current = content_hash(&root.join(path))?;
            entry["current"] = json!(current.as_deref() == entry["contentHash"].as_str());
            Ok(entry)
        })
        .collect()
}

/// Planning evidence of earlier sessions in this worktree whose files still have the recorded
/// content, newest first: what a new session may continue from. No tasks are implied.
pub fn earlier_planning(root: &Path, session: &str) -> Result<Vec<Value>, Failure> {
    let own = state::location(root, Some(session))?.session_id;
    let mut found: Vec<(i64, Value)> = Vec::new();
    for directory in state::sweep::sessions(root, craft_engine::session::handoff::SCANNED)? {
        let Some(entry) = directory
            .entries
            .iter()
            .find(|e| e.file && e.name == "intent.json")
        else {
            continue;
        };
        if directory.id == own {
            continue;
        }
        let Ok(record) = read_intent(&directory.path.join("intent.json")) else {
            continue;
        };
        for mut artifact in planning_of(root, &record)? {
            if artifact["current"] == true {
                artifact.as_object_mut().expect("entry").remove("current");
                artifact["from"] = json!(directory.id);
                found.push((entry.modified, artifact));
            }
        }
    }
    found.sort_by_key(|(modified, _)| std::cmp::Reverse(*modified));
    Ok(found
        .into_iter()
        .take(delivery::PLANNING_ARTIFACTS)
        .map(|(_, artifact)| artifact)
        .collect())
}

fn read_intent(file: &Path) -> Result<Value, Failure> {
    let record = read_record(file, INTENT_BYTES).map_err(|f| f.message)?;
    delivery::validate_intent(&record)?;
    Ok(record)
}

/// Apply `update` to the current record under its lock; a busy record keeps its state.
fn write_intent(file: &Path, update: impl FnOnce(&Value) -> Option<Value>) -> Result<(), Failure> {
    let Some(_lock) = state::lock(file)? else {
        return Ok(());
    };
    match update(&read_intent(file)?) {
        Some(next) => Ok(state::write(file, &next, INTENT_BYTES, false)?),
        None => Ok(()),
    }
}

fn generation_file(file: &Path) -> PathBuf {
    let mut name = file.as_os_str().to_owned();
    name.push(".generation");
    PathBuf::from(name)
}

fn read_hint(file: &Path) -> Result<Value, Failure> {
    let hint = read_record(file, HINT_BYTES).map_err(|f| f.message)?;
    delivery::validate_hint(&hint)?;
    Ok(hint)
}

/// A new generation key, independent of the hint lock, so an invalidation during an
/// in-flight delivery still takes effect.
fn invalidate_hint(
    root: &Path,
    session: &str,
    host: &str,
    recorder: &mut Recorder,
    hook: &Operation,
) {
    let operation = recorder.start("cache.invalidated", Some(hook));
    let result = (|| -> Result<(), Failure> {
        let file = generation_file(&hint_file(root, session, host)?);
        read_hint(&file)?;
        let key = hash(uuid()?.as_bytes());
        let generation = json!({"formatVersion": 1, "key": key, "emittedAt": now_ms()});
        Ok(state::write(&file, &generation, HINT_BYTES, false)?)
    })();
    recorder.complete(
        operation,
        Map::new(),
        result.is_err().then_some("CACHE_UNAVAILABLE"),
    );
    if result.is_err() {
        recorder.report("cache", "CACHE_UNAVAILABLE", Map::new(), hook);
    }
}

/// Suppress a routing hint identical to one emitted within the reuse window; otherwise
/// record the hint the host is about to receive.
fn reuse_hint(
    root: &Path,
    session: &str,
    host: &str,
    settings: &Value,
    decision: &mut Value,
    recorder: &mut Recorder,
    hook: &Operation,
) -> Result<(), Failure> {
    let file = hint_file(root, session, host)?;
    let Some(_lock) = state::lock(&file)? else {
        recorder.report("cache", "STATE_BUSY", Map::new(), hook);
        return Ok(());
    };
    let previous = read_hint(&file)?;
    let generation = read_hint(&generation_file(&file))?;
    let key = delivery::hint_key(generation["key"].as_str(), settings, &decision["context"]);
    let now = now_ms();
    let reused = delivery::hint_valid(&previous, &key, now as f64);
    let (hits, misses) = if reused { (1, 0) } else { (0, 1) };
    let attributes = Map::from_iter([
        ("reused".to_string(), json!(reused)),
        ("cacheHits".to_string(), json!(hits)),
        ("cacheMisses".to_string(), json!(misses)),
    ]);
    recorder.observe("cache.observed", attributes, hook, &[]);
    if reused {
        decision
            .as_object_mut()
            .expect("decision object")
            .remove("context");
        return Ok(());
    }
    // Eligible hints are delivered: the host supports context and the decision carries it.
    let hint = json!({"formatVersion": 1, "key": key, "emittedAt": now});
    Ok(state::write(&file, &hint, HINT_BYTES, false)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tool_paths_resolve_against_the_hook_directory_inside_the_project() {
        let root = Path::new("/work/project");
        let cwd = root.join("src");
        let file = |f| project_file(root, &cwd, f);
        assert_eq!(file(Some("a.py")).as_deref(), Some("src/a.py"));
        assert_eq!(
            file(Some("/work/project/src/../b.py")).as_deref(),
            Some("b.py")
        );
        assert_eq!(file(Some("../../other/c.md")), None);
        assert_eq!(file(Some("/work/projectx/c.md")), None);
        assert_eq!(file(Some("/elsewhere/notes.md")), None);
        assert_eq!(file(None).as_deref(), Some(""));
        assert_eq!(file(Some("")).as_deref(), Some(""));
    }

    struct Fixture {
        root: PathBuf,
        recorder: Recorder,
        hook: Operation,
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.root);
        }
    }

    fn fixture(name: &str) -> Fixture {
        let root = std::env::temp_dir().join(format!("craft-hint-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let root = std::fs::canonicalize(&root).unwrap();
        let recorder = Recorder::open(&root, Some("s1"), None);
        let hook = recorder.start("host.hook", None);
        Fixture {
            root,
            recorder,
            hook,
        }
    }

    fn routed(text: &str) -> Value {
        json!({"decision": "allow", "context": {"kind": "routing", "data": {"intent": text}}})
    }

    impl Fixture {
        /// Whether the hint survived reuse, or the reuse error.
        fn deliver(&mut self, settings: &Value, decision: &Value) -> Result<bool, String> {
            let mut decision = decision.clone();
            reuse_hint(
                &self.root,
                "s1",
                "codex",
                settings,
                &mut decision,
                &mut self.recorder,
                &self.hook,
            )
            .map_err(|failure| failure.message)?;
            Ok(decision.get("context").is_some())
        }
    }

    #[test]
    fn identical_hints_stay_quiet_until_scope_settings_or_generation_change() {
        let mut f = fixture("reuse");
        let settings = json!({"routing": {"dedupeHints": true}});
        assert_eq!(f.deliver(&settings, &routed("implement")), Ok(true));
        assert_eq!(f.deliver(&settings, &routed("implement")), Ok(false));
        let file = hint_file(&f.root, "s1", "codex").unwrap();
        let stored = std::fs::read_to_string(&file).unwrap();
        assert!(
            !stored.contains("implement"),
            "hint records keep no context text"
        );
        assert_eq!(f.deliver(&settings, &routed("review")), Ok(true));
        let changed = json!({"routing": {"dedupeHints": true}, "languages": ["python"]});
        assert_eq!(f.deliver(&changed, &routed("review")), Ok(true));
        assert_eq!(f.deliver(&changed, &routed("review")), Ok(false));
        let (root, hook) = (f.root.clone(), f.recorder.start("host.hook", None));
        invalidate_hint(&root, "s1", "codex", &mut f.recorder, &hook);
        assert_eq!(f.deliver(&changed, &routed("review")), Ok(true));
    }

    #[test]
    fn busy_or_unreadable_records_never_suppress_a_hint() {
        let mut f = fixture("busy");
        let settings = json!({"routing": {"dedupeHints": true}});
        assert_eq!(f.deliver(&settings, &routed("implement")), Ok(true));
        let file = hint_file(&f.root, "s1", "codex").unwrap();
        let held = state::lock(&file).unwrap().expect("free lock");
        assert_eq!(f.deliver(&settings, &routed("implement")), Ok(true));
        drop(held);
        std::fs::write(&file, "invalid").unwrap();
        assert!(f.deliver(&settings, &routed("implement")).is_err());
        let mut lock = file.as_os_str().to_owned();
        lock.push(".lock");
        assert!(!Path::new(&lock).exists(), "the reuse lock is released");
        std::fs::remove_file(&file).unwrap();
        #[cfg(unix)]
        {
            let missing = f.root.join("absent.json");
            std::os::unix::fs::symlink(&missing, &file).unwrap();
            assert!(f.deliver(&settings, &routed("implement")).is_err());
            assert!(!missing.exists());
        }
    }
}
