//! The facts a turn-end notice's next step is chosen from, read from the session's own
//! records and the files the turn changed: the session's check runs, the task bound to it with
//! its acceptance contract and Craft work plan, the host's last context observation and its
//! checkpoint. A consumer's own planning files are never read. A record that cannot be read is a fact Craft does not
//! hold, so it names nothing from it.
use crate::telemetry::{Operation, Recorder, now_ms};
use craft_engine::checks;
use craft_engine::session::next::{self, Checkpoint, Facts};
use serde_json::{Value, json};
use std::path::Path;
use std::time::UNIX_EPOCH;

/// The next step for the turn that changed `changed` (named from the project root), or
/// `None` when Craft holds no fact that calls for one.
pub(crate) fn line(
    root: &Path,
    settings: &Value,
    session: &str,
    changed: &[String],
    recorder: &mut Recorder,
    hook: &Operation,
) -> Option<String> {
    if !next::enabled(settings) {
        return None;
    }
    let mut facts = Facts {
        unrun_checks: unrun_checks(root, settings, session, changed),
        ..Facts::default()
    };
    if facts.unrun_checks == 0 {
        task(root, session, &mut facts, recorder, hook);
    }
    if facts == Facts::default() {
        facts.context = context(root, settings, session, recorder, hook);
    }
    next::line(&facts)
}

/// The facts of the task bound to the session: a required acceptance criterion without
/// evidence while it is open, and the plan's next ready task once it is completed.
fn task(root: &Path, session: &str, facts: &mut Facts, recorder: &mut Recorder, hook: &Operation) {
    let Ok(Some(binding)) = crate::session::active_binding(root, session) else {
        return;
    };
    let Some(task) = binding["task"].as_str() else {
        return;
    };
    if let Ok(state) = crate::session::read_state(root, session, task, recorder, hook) {
        facts.unevidenced =
            next::unevidenced(&state["taskRecord"]).map(|criterion| (criterion, task.to_owned()));
    }
    if facts.unevidenced.is_some() {
        return;
    }
    let Some(plan) = binding["plan"].as_str() else {
        return;
    };
    // The queue reads every holder's session; it is taken only once the task is completed.
    let Ok(Some(saved)) = crate::plan::read(root, plan) else {
        return;
    };
    let completed = saved["tasks"]
        .as_array()
        .into_iter()
        .flatten()
        .any(|entry| entry["id"] == task && entry["state"] == "completed");
    if completed && let Ok(queue) = crate::plan::queue(root, plan, Some(session)) {
        facts.ready = next::ready(&saved, task, &queue);
    }
}

/// Declared checks of the commit stage (and the step stage before it) that cover a changed
/// file, when the session ran none of them since the newest change.
fn unrun_checks(root: &Path, settings: &Value, session: &str, changed: &[String]) -> usize {
    if changed.is_empty() || !checks::enabled(settings) {
        return 0;
    }
    let files: Vec<Value> = changed.iter().map(|file| json!(file)).collect();
    let covering = checks::covering(settings, &files).map_or(0, |found| found.len());
    if covering == 0 {
        return 0;
    }
    let newest = changed
        .iter()
        .filter_map(|file| root.join(file).metadata().ok()?.modified().ok())
        .filter_map(|time| time.duration_since(UNIX_EPOCH).ok())
        .filter_map(|since| i64::try_from(since.as_millis()).ok())
        .max();
    let Ok(history) = crate::checks::history(root, session) else {
        return 0;
    };
    if next::unrun(&history, checks::DEFAULT_STAGE, newest) {
        covering
    } else {
        0
    }
}

/// The observed occupancy past the compaction threshold, with where the session's saved
/// continuation stands against the commit the project is on.
fn context(
    root: &Path,
    settings: &Value,
    session: &str,
    recorder: &mut Recorder,
    hook: &Operation,
) -> Option<(u64, Checkpoint)> {
    let saved = crate::host_context::read(root, session).ok()?;
    let percent = next::pressure(settings, &saved["usage"], session, now_ms())?;
    // Hooks keep an automatic snapshot too; only a saved continuation says where to resume.
    let checkpoint = match crate::checkpoint::read(root, session, recorder, hook) {
        Ok(saved) if saved["continuationSource"].is_object() => {
            let head = crate::hook::stop_review::head(root);
            let saved = saved["continuationSource"]["head"].as_str();
            if head.is_some() && saved == head.as_deref() {
                Checkpoint::Current
            } else {
                Checkpoint::Behind
            }
        }
        _ => Checkpoint::Missing,
    };
    Some((percent, checkpoint))
}
