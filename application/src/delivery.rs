//! Context delivery hooks: lifecycle facts → routing hints, checkpoints, service metadata and
//! workspace housekeeping, with identical routing hints suppressed under one locked step.
use crate::checkpoint;
use crate::cli::{prepared_services, routed};
use crate::failure::Failure;
use crate::project;
use crate::scratch::{self, Action};
use crate::session::{active_task, read_record};
use crate::telemetry::observe;
use crate::telemetry::{Operation, Recorder, now_ms};
use craft_engine::{delivery, hash, identity, routing};
use craft_platform::{state, telemetry::uuid};
use serde_json::{Map, Value, json};
use std::path::{Component, Path, PathBuf};

const HINT_BYTES: usize = 2048;

/// One delivery hook event, in host-neutral facts.
pub struct Event<'a> {
    pub root: &'a Path,
    pub cwd: &'a Path,
    pub host: &'a str,
    pub lifecycle: &'a str,
    pub session: Option<&'a str>,
    pub edit: bool,
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
    let settings = observe(
        recorder,
        "project.loaded",
        hook,
        "CONFIG_INVALID",
        Map::new(),
        |_, _| project::settings(root, None),
    )?;
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
    let facts = delivery::Facts {
        session: session.is_some(),
        edit: event.edit,
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
        Some(prepared_services(&settings, None, None, recorder, hook)?)
    } else {
        None
    };
    // An edit outside the project has no project route; it is not a delivery failure.
    let file = project_file(root, event.cwd, event.file);
    let routing = match (plan.intent, file) {
        (Some(intent), Some(file)) => {
            if !event.context_supported {
                recorder.report("host", "HOST_CAPABILITY_UNAVAILABLE", Map::new(), hook);
            }
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
            };
            let mut plan = routed(&settings, &request, recorder, hook)?;
            plan["taskIdentity"] = task_identity;
            Some(plan)
        }
        _ => None,
    };
    let mut saved = None;
    if let (true, Some(session)) = (plan.save_checkpoint, session) {
        // Only state and explicit metadata: never prompts, transcripts or tool output.
        let metadata = task.as_ref().map_or(json!({}), |t| json!({"taskId": t}));
        checkpoint::save(root, session, &metadata, None, recorder, hook)?;
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

fn hint_file(root: &Path, session: &str, host: &str) -> Result<PathBuf, String> {
    let name = format!("routing-{}.json", &identity(&[host, session])[..24]);
    state::file(root, &name, true, Some(session))
}

fn generation_file(file: &Path) -> PathBuf {
    let mut name = file.as_os_str().to_owned();
    name.push(".generation");
    PathBuf::from(name)
}

fn read_hint(file: &Path) -> Result<Value, String> {
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
    let result = (|| -> Result<(), String> {
        let file = generation_file(&hint_file(root, session, host)?);
        read_hint(&file)?;
        let key = hash(uuid()?.as_bytes());
        let generation = json!({"formatVersion": 1, "key": key, "emittedAt": now_ms()});
        state::write(&file, &generation, HINT_BYTES, false)
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
) -> Result<(), String> {
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
    state::write(&file, &hint, HINT_BYTES, false)
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
            )?;
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
