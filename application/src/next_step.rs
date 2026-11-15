//! The facts a turn-end notice's next step is chosen from, read from the session's own
//! records and the files the turn changed: the session's check runs, the host's last context
//! observation and its checkpoint. A record that cannot be read is a fact Craft does not
//! hold, so it names nothing from it.
use crate::telemetry::{Operation, Recorder, now_ms};
use craft_engine::session::next::{self, Checkpoint, Facts};
use craft_engine::{checks, routing};
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
    let facts = Facts {
        unrun_checks: unrun_checks(root, settings, session, changed),
        context: context(root, settings, session, recorder, hook),
    };
    next::line(&facts)
}

/// Declared checks of the commit stage (and the step stage before it) that cover a changed
/// file, when the session ran none of them since the newest change.
fn unrun_checks(root: &Path, settings: &Value, session: &str, changed: &[String]) -> usize {
    if changed.is_empty() || !checks::enabled(settings) {
        return 0;
    }
    let files: Vec<Value> = changed.iter().map(|file| json!(file)).collect();
    let Ok(scope) = routing::scope(settings, &files) else {
        return 0;
    };
    let Ok(plan) = checks::plan(settings, &scope, checks::DEFAULT_STAGE, false) else {
        return 0;
    };
    let covering = plan["run"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|check| {
            !checks::takes_files(check)
                || checks::files_for(settings, check, &files).is_ok_and(|f| !f.is_empty())
        })
        .count();
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
            let head = crate::stop_review::head(root);
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
