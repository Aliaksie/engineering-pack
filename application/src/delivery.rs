//! Context delivery hooks: lifecycle facts → routing hints, checkpoints, service metadata and
//! workspace housekeeping, with identical routing hints suppressed under one locked step.
use crate::checkpoint;
use crate::failure::Failure;
use crate::project;
use crate::scratch::{self, Action};
use crate::session::{active_task, read_record};
use crate::telemetry::{Operation, Recorder, now_ms};
use craft_engine::delivery::{self, answer};
use craft_engine::{hash, identity, routing, standards};
use craft_platform::{state, telemetry::uuid};
use serde_json::{Map, Value, json};
use std::path::{Component, Path, PathBuf};

const HINT_BYTES: usize = 2048;
const INTENT_BYTES: usize = 16384;
const RESTORE_BYTES: usize = 256;

/// One delivery hook event, in host-neutral facts.
pub struct Event<'a> {
    pub root: &'a Path,
    pub cwd: &'a Path,
    pub host: &'a str,
    /// The package whose hook this is.
    pub package: Option<&'a str>,
    pub lifecycle: &'a str,
    pub session: Option<&'a str>,
    /// The tool writes a file.
    pub edit: bool,
    /// This tool hook delivers an edit's context on the host.
    pub edit_context: bool,
    pub file: Option<&'a str>,
    /// Every file the edit writes, when it writes several (a patch); includes `file`.
    pub files: &'a [&'a str],
    pub prompt: Option<&'a str>,
    /// A search tool's pattern and a shell tool's command, for a search the model ran.
    pub pattern: Option<&'a str>,
    pub command: Option<&'a str>,
    pub context_supported: bool,
    /// The host is continuing a stop a hook already held.
    pub stop_active: bool,
    /// The runner's verdict on a test command the host just ran.
    pub test_run: Option<&'a Value>,
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
    let settings = project::hook_loaded(recorder, hook, root)?;
    if event.lifecycle == "agent-start" {
        return Ok(delegation(root, &settings, event, recorder, hook));
    }
    let session = event.session.filter(|s| !s.is_empty());
    // A test command the host just ran: its verdict, kept with the tree it ran on.
    let observed = match (
        event.test_run.filter(|_| delivery::test_runs(&settings)),
        session,
    ) {
        (Some(verdict), Some(session)) => observe_run(root, session, event, verdict)
            .map_err(|_| recorder.report("state", "STATE_UNAVAILABLE", Map::new(), hook))
            .ok(),
        _ => None,
    };
    if event.lifecycle == "after-tool-failure" {
        return Ok(json!({"decision": "allow"}));
    }
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
    if event.lifecycle == "session-start" {
        warm_evidence(root);
        if let Some(session) = session
            && crate::stop_review::baseline(root, session, &settings).is_err()
        {
            recorder.report("state", "STATE_UNAVAILABLE", Map::new(), hook);
        }
    }
    let prompt = (event.lifecycle == "before-prompt").then(|| event.prompt.unwrap_or(""));
    // Every project file the edit writes: one, or each file of a patch.
    let mut edited: Vec<String> = Vec::new();
    if event.edit {
        let named = event.file.into_iter().chain(event.files.iter().copied());
        for path in named.filter_map(|f| project_file(root, event.cwd, Some(f))) {
            if !path.is_empty() && !edited.contains(&path) {
                edited.push(path);
            }
        }
    }
    // An edit outside the project has no project route; it is not a delivery failure. A
    // patch is routed when any file it writes is the project's, not only its first.
    let file = match edited.first() {
        Some(first) => Some(first.clone()),
        None => project_file(root, event.cwd, event.file),
    };
    // Without session records an existing record is still read, never created or changed.
    let records = delivery::session_records(&settings);
    let intent_file = match session {
        Some(session) if prompt.is_some() || event.edit || event.lifecycle == "turn-end" => {
            Some(intent_file(root, session, records)?)
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
        file_routed: !edited.is_empty()
            && edited
                .iter()
                .all(|file| delivery::file_routed(&record, file)),
        prompt,
        started: event.lifecycle == "session-start",
        context_changing: event.lifecycle == "before-compact",
        context_restored: event.lifecycle == "after-compact",
        turn_finished: event.lifecycle == "turn-end",
        // Only a bound task has records to resume from; prompts and tool results carry it.
        restore_pending: (prompt.is_some() || event.lifecycle == "after-tool")
            && task.is_some()
            && session.is_some_and(|session| restore_pending(root, session)),
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
    // Hook answers, on prompts and on searches, follow the managed-lookup switch.
    let answers = settings["efficiency"]["lookup"]["enabled"] == true;
    let routing = match (plan.intent, file.as_deref()) {
        (Some(intent), Some(file)) => {
            let files: Vec<Value> = if event.edit {
                edited.iter().map(|f| json!(f)).collect()
            } else if file.is_empty() {
                vec![]
            } else {
                vec![json!(file)]
            };
            // What the host installed beside this hook's package; unknown outside a bundle.
            let packages = event.package.and_then(|package| {
                let bundle = std::env::var_os(crate::statusline::PACKAGE_ROOT)?;
                let bundle = std::fs::canonicalize(bundle).ok()?;
                craft_platform::host_install::packages(event.host, root, &bundle, package)
            });
            let request = routing::Request {
                packages: packages.as_deref(),
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
            }
            // A verified answer serves any route: a question about decision records reads as
            // planning, one about a configuration key may route nowhere else.
            if let Some(answer) = prompt
                .filter(|_| answers)
                .and_then(|p| prompt_answer(root, p, recorder, hook))
            {
                plan["answer"] = answer;
            }
            // The route still establishes the session's intent below.
            Some(plan).filter(|plan| delivery::speaks(plan, prompt.is_some()))
        }
        // A prompt no intent claims still gets the answer to the lookup it asks, alone.
        (None, Some(_)) if event.context_supported => prompt
            .filter(|_| answers)
            .and_then(|p| prompt_answer(root, p, recorder, hook))
            .map(|answer| json!({"answer": answer})),
        _ => None,
    };
    // A request establishes its intent; an edit only once it is routed.
    let routed_edit = event.edit && routing.is_some();
    if let (Some(intent), Some(target)) = (plan.establish, intent_file.as_deref())
        && records
        && (prompt.is_some() || routed_edit)
    {
        let now = now_ms() as u64;
        if write_intent(target, |record| {
            if !routed_edit || edited.is_empty() {
                return delivery::established(record, intent, None, now);
            }
            // Each written file joins the record; unchanged when all were there.
            let mut next: Option<Value> = None;
            for file in &edited {
                let current = next.as_ref().unwrap_or(record);
                if let Some(updated) = delivery::established(current, intent, Some(file), now) {
                    next = Some(updated);
                }
            }
            next
        })
        .is_err()
        {
            recorder.report("state", "STATE_UNAVAILABLE", Map::new(), hook);
        }
    }
    // A file written while the session plans is its planning evidence, by content hash.
    if let (true, "after-tool", Some(target)) = (
        records && event.edit && record["intent"] == "plan",
        event.lifecycle,
        intent_file.as_deref(),
    ) {
        for path in &edited {
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
    }
    let mut saved = None;
    if let (true, Some(session)) = (plan.save_checkpoint, session) {
        // Only state and explicit metadata: never prompts, transcripts or tool output.
        let metadata = task.as_ref().map_or(json!({}), |t| json!({"taskId": t}));
        let checkpoint = checkpoint::save(root, session, &metadata, None, recorder, hook)?;
        // Whatever tool wrote it (an edit, a shell redirect, a script), a file the turn
        // changed after the plan was requested is the session's planning evidence.
        if let (true, Some(target), Some(since)) = (
            records && event.lifecycle == "turn-end" && record["intent"] == "plan",
            intent_file.as_deref(),
            record["since"].as_u64(),
        ) && planning_written(root, target, &checkpoint["state"]["status"], since).is_err()
        {
            recorder.report("state", "STATE_UNAVAILABLE", Map::new(), hook);
        }
    }
    if let (true, Some(session), Some(_)) = (plan.mark_restore && records, session, &task)
        && mark_restore(root, session, true).is_err()
    {
        recorder.report("state", "STATE_UNAVAILABLE", Map::new(), hook);
    }
    if let Some(session) = session {
        if plan.recover_checkpoint {
            saved = Some(checkpoint::recover(root, session, recorder, hook)?);
            // Delivered here, so the next prompt does not repeat it.
            if event.context_supported
                && restore_pending(root, session)
                && mark_restore(root, session, false).is_err()
            {
                recorder.report("state", "STATE_UNAVAILABLE", Map::new(), hook);
            }
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
    // A resumed session restates the checked and reviewed standards of its scope: rules a
    // compaction dropped from context come back with the work they govern, whether or not
    // a task was bound to it.
    let resuming = match decision["context"]["kind"].as_str() {
        Some("resume") => true,
        Some("checkpoint") => plan.recover_checkpoint,
        _ => false,
    };
    if let (true, Some(session)) = (resuming, session) {
        let (record, task) = session_scope(root, session, recorder, hook);
        let files = record["files"].as_array().cloned().unwrap_or_default();
        let pinned = standards::pinned(&settings, record["intent"].as_str(), &files, task.as_ref());
        if !pinned.is_empty() {
            decision["context"]["data"]["standards"] = json!(pinned);
        }
        // Whether the tests the session ran still speak for the tree it resumes on.
        let repositories = crate::session::active_binding(root, session)
            .ok()
            .flatten()
            .and_then(|binding| binding.get("repositories").cloned());
        let status = if delivery::test_runs(&settings) {
            crate::test_runs::status(root, session, repositories.as_ref())
        } else {
            Ok(None)
        };
        match status {
            Ok(Some(tests)) => decision["context"]["data"]["tests"] = tests,
            Ok(None) => {}
            Err(_) => recorder.report("state", "STATE_UNAVAILABLE", Map::new(), hook),
        }
    }
    // A search the model just ran gets its verified answer in the same tool result: no turn.
    let searching = event.lifecycle == "after-tool"
        && event.context_supported
        && decision.get("context").is_none()
        && answers;
    if let Some(answer) = searching
        .then(|| search_answer(root, event.pattern, event.command, recorder, hook))
        .flatten()
    {
        decision["context"] = json!({"kind": "answer", "data": answer});
    }
    // The session identity a started or restored session needs for task commands.
    if let (true, Some(session)) = (plan.identify_session, session) {
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
    // A run that exited cleanly but ran no test verifies nothing; the model hears it at once.
    if let Some(run) = observed.filter(|run| {
        event.context_supported
            && matches!(run["reason"].as_str(), Some("zero-tests" | "all-skipped"))
    }) {
        decision["tests"] = run;
    }
    // A file or commit just written in a declared standard's scope gets its anchor check in
    // the same tool result. Findings are reported, never enforced; passing checks stay quiet.
    if event.lifecycle == "after-tool" && event.context_supported {
        let found = standard_findings(root, &settings, event, &edited);
        if !found.is_empty() {
            decision["standards"] = json!(found);
        }
    }
    // A turn that changed files ends with one summary of what is new, where the host can
    // hold the stop and continue with it.
    if let (true, Some(session)) = (
        event.lifecycle == "turn-end" && crate::hook::continues_turn(event.host),
        session,
    ) {
        let intent = record["intent"].as_str();
        match crate::stop_review::stop(root, session, &settings, intent, event.stop_active) {
            Ok(Some(summary)) => decision["stop"] = summary,
            Ok(None) => {}
            Err(_) => recorder.report("state", "STATE_UNAVAILABLE", Map::new(), hook),
        }
    }
    let eligible = decision["context"]["kind"] == "routing"
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

/// The decision when the host starts a worker of its own: the standards of the delegating
/// session's scope as the worker's context. The host passes no delegated prompt, so the scope
/// is what the session established and its bound plan task owns. Nothing to carry stays quiet.
fn delegation(
    root: &Path,
    settings: &Value,
    event: &Event,
    recorder: &mut Recorder,
    hook: &Operation,
) -> Value {
    let mut decision = json!({"decision": "allow"});
    let Some(session) = event.session.filter(|s| {
        !s.is_empty() && event.context_supported && standards::worker_standards(settings)
    }) else {
        return decision;
    };
    let (record, task) = session_scope(root, session, recorder, hook);
    let files = record["files"].as_array().cloned().unwrap_or_default();
    let intent = record["intent"].as_str();
    let found = standards::delegated(settings, intent, &files, task.as_ref());
    if !found.is_empty() {
        decision["delegation"] = json!({"intent": intent, "standards": found});
    }
    decision
}

/// What a session established (its intent record) and the scope of the task bound to it.
/// Unreadable state reads as nothing established and is reported.
fn session_scope(
    root: &Path,
    session: &str,
    recorder: &mut Recorder,
    hook: &Operation,
) -> (Value, Option<Value>) {
    let record = intent_file(root, session, false)
        .and_then(|file| read_intent(&file))
        .unwrap_or_else(|_| {
            recorder.report("state", "STATE_UNAVAILABLE", Map::new(), hook);
            Value::Null
        });
    let task = match crate::session::active_binding(root, session) {
        Ok(binding) => binding.map(|binding| bound_task(root, session, &binding, recorder, hook)),
        Err(_) => {
            recorder.report("state", "STATE_UNAVAILABLE", Map::new(), hook);
            None
        }
    };
    (record, task)
}

/// Record a test run the session's host just ran, over the bound task's repositories.
fn observe_run(
    root: &Path,
    session: &str,
    event: &Event,
    verdict: &Value,
) -> Result<Value, Failure> {
    let repositories = crate::session::active_binding(root, session)?
        .and_then(|binding| binding.get("repositories").cloned());
    crate::test_runs::record(
        root,
        session,
        event.host,
        event.command.unwrap_or_default(),
        verdict,
        repositories.as_ref(),
    )
}

/// The scope of the task bound to a session: the files its task record names and, for a
/// plan task, the plan's owned paths, in the shape [`standards::for_task`] reads.
fn bound_task(
    root: &Path,
    session: &str,
    binding: &Value,
    recorder: &mut Recorder,
    hook: &Operation,
) -> Value {
    let mut files: Vec<Value> = binding["task"]
        .as_str()
        .and_then(|task| crate::session::read_state(root, session, task, recorder, hook).ok())
        .and_then(|state| state["taskRecord"]["files"].as_array().cloned())
        .unwrap_or_default();
    let planned = binding["plan"]
        .as_str()
        .and_then(|plan| crate::plan::read(root, plan).ok().flatten())
        .and_then(|saved| {
            saved["tasks"]
                .as_array()?
                .iter()
                .find(|task| task["id"] == binding["task"])
                .cloned()
        });
    let scope = planned
        .as_ref()
        .map_or(Value::Null, |task| task["scope"].clone());
    for file in planned
        .iter()
        .flat_map(|task| task["files"].as_array().cloned().unwrap_or_default())
    {
        if !files.contains(&file) {
            files.push(file);
        }
    }
    json!({"files": files, "scope": scope})
}

/// How large a written file an anchor check reads; larger files are left unchecked.
const STANDARD_BYTES: u64 = 512 * 1024;
/// How recent `HEAD` must be to be the commit the shell command just made.
const COMMIT_FRESH_SECONDS: u64 = 120;

/// Anchor check results for the files an edit wrote or the commit a shell command made.
fn standard_findings(
    root: &Path,
    settings: &Value,
    event: &Event,
    edited: &[String],
) -> Vec<Value> {
    if event.edit {
        // A patch writes several files: each finding then names the one it is about.
        let several = edited.len() > 1;
        return edited
            .iter()
            .flat_map(|file| {
                file_findings(root, settings, file)
                    .into_iter()
                    .map(move |mut found| {
                        if several {
                            found["file"] = json!(file);
                        }
                        found
                    })
            })
            .collect();
    }
    let checks = standards::commit_checks(settings);
    if checks.is_empty() || !event.command.is_some_and(standards::commits) {
        return Vec::new();
    }
    // A failed commit leaves an older HEAD; only a commit made just now is the one written.
    let Ok(Some(output)) =
        craft_platform::process::git(root, &["log", "-1", "--format=%ct%n%B"], 64 * 1024)
    else {
        return Vec::new();
    };
    let output = String::from_utf8_lossy(&output);
    let (time, message) = output.split_once('\n').unwrap_or((&output, ""));
    let now = now_ms() as u64 / 1000;
    match time.trim().parse::<u64>() {
        Ok(time) if now.saturating_sub(time) <= COMMIT_FRESH_SECONDS => {
            standards::verdicts(&checks, message.trim_end(), None)
        }
        _ => Vec::new(),
    }
}

/// Anchor and module dependency check results for one written project file.
fn file_findings(root: &Path, settings: &Value, file: &str) -> Vec<Value> {
    let checks = standards::file_checks(settings, file);
    if checks.is_empty() && standards::import_checks(settings, file).is_empty() {
        return Vec::new();
    }
    let path = root.join(file);
    let bytes = match std::fs::metadata(&path) {
        Ok(meta) if meta.is_file() && meta.len() <= STANDARD_BYTES => std::fs::read(&path),
        _ => return Vec::new(),
    };
    let Ok(bytes) = bytes else {
        return Vec::new();
    };
    let mut found = if checks.is_empty() {
        Vec::new()
    } else {
        standards::verdicts(&checks, &String::from_utf8_lossy(&bytes), Some(file))
    };
    found.extend(crate::dependencies::verdicts(root, settings, file, &bytes));
    found
}

/// Start indexing the project in the background, so the session's first lookup finds a warm
/// index. Only for a Git worktree with evidence enabled, unless `CRAFT_EVIDENCE_WARM=0` (for
/// short-lived checkouts); where the platform cannot detach a process, the first lookup
/// indexes within its own budget as before.
fn warm_evidence(root: &Path) {
    if std::env::var_os("CRAFT_EVIDENCE_WARM").is_some_and(|value| value == "0") {
        return;
    }
    let enabled = crate::project::configuration(root)
        .ok()
        .and_then(|config| craft_engine::contracts::evidence_enabled(&config).ok())
        .unwrap_or(false);
    if !enabled || !root.ancestors().any(|dir| dir.join(".git").exists()) {
        return;
    }
    // Only the `craft` executable starts itself; a library caller such as a test harness
    // would start a copy of itself with arguments it does not understand.
    let program = std::env::current_exe()
        .ok()
        .filter(|exe| exe.file_stem().is_some_and(|stem| stem == "craft"));
    if let (Some(program), Some(root_arg)) = (program, root.to_str()) {
        let args = ["project-evidence", "warm", "--enable", "--root", root_arg];
        let _ = craft_platform::process::spawn_detached(&program, &args, root);
    }
}

/// Index refresh all prompt-time lookups of one prompt may spend together; the prompt waits
/// for it, and sources it defers are searched literally.
const PROMPT_REFRESH_MS: u64 = 1_000;

/// Index refresh a search answer may spend: enough for the few sources an edit changed
/// since the index was warmed; a larger refresh leaves the search unanswered.
const SEARCH_REFRESH_MS: u64 = 200;

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
    evidence_answer(
        root,
        ask,
        &value,
        PROMPT_REFRESH_MS,
        recorder,
        hook,
        |result, declarations| answer::answered(ask, &value, result, declarations),
    )
}

/// The verified declaration and callers of the one name a search the model just ran looks
/// for, or `None`: the model keeps its own search result either way.
fn search_answer(
    root: &Path,
    pattern: Option<&str>,
    command: Option<&str>,
    recorder: &mut Recorder,
    hook: &Operation,
) -> Option<Value> {
    let value = answer::searched(pattern, command)?;
    evidence_answer(
        root,
        "callers",
        &value,
        SEARCH_REFRESH_MS,
        recorder,
        hook,
        |result, declarations| answer::usages(&value, result, declarations?),
    )
}

/// Ask Project Intelligence `ask` about `value` within one `refresh_ms` budget, then the
/// declaration when callers can answer at all, and certify the pair with `certify`.
fn evidence_answer(
    root: &Path,
    ask: &str,
    value: &str,
    refresh_ms: u64,
    recorder: &mut Recorder,
    hook: &Operation,
    certify: impl FnOnce(&Value, Option<&Value>) -> Option<Value>,
) -> Option<Value> {
    let operation = recorder.start("discovery.completed", Some(hook));
    let started = std::time::Instant::now();
    let lookup = |ask: &str, recorder: &mut Recorder| {
        let input = json!({"ask": ask, "value": value, "paths": [], "limit": answer::rows(ask)});
        let spent = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
        // A later lookup queries the index the first one just refreshed, so it needs no
        // budget of its own; one millisecond keeps the refresh bounded rather than unlimited.
        let refresh = refresh_ms.saturating_sub(spent).max(1);
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
    // add latency to an event that gets no answer.
    let declarations = match (ask, &result) {
        ("callers", Some(result)) if answer::answerable(result) => lookup("declaration", recorder),
        _ => None,
    };
    let answered = result
        .as_ref()
        .and_then(|result| certify(result, declarations.as_ref()));
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

/// Whether context changed and the bound task has not been resumed since. Unreadable reads
/// as not pending: a missed resume costs what it did before, a repeated one costs bytes.
fn restore_pending(root: &Path, session: &str) -> bool {
    state::file(root, "restore.json", false, Some(session))
        .ok()
        .and_then(|file| read_record(&file, RESTORE_BYTES).ok())
        .is_some_and(|record| record["pending"] == true)
}

fn mark_restore(root: &Path, session: &str, pending: bool) -> Result<(), Failure> {
    let file = state::file(root, "restore.json", pending, Some(session))?;
    if !pending && !file.exists() {
        return Ok(());
    }
    let record = json!({"formatVersion": 1, "pending": pending});
    Ok(state::write(&file, &record, RESTORE_BYTES, false)?)
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
