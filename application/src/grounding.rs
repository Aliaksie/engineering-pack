use crate::{
    cli::Failure,
    project,
    telemetry::{Operation, Recorder, now_ms},
};
use craft_engine::{digest, grounding, session, timestamp};
use craft_platform::{grounding::observe, repository::snapshot};
use serde_json::{Value, json};
use std::path::Path;
pub fn run(
    root: &Path,
    input: &Value,
    preferences: Option<&Path>,
    _recorder: &mut Recorder,
    _parent: &Operation,
) -> Result<Value, Failure> {
    let request = grounding::request(input)?;
    let files = request["files"].as_array().unwrap();
    let before = snapshot::workspace(root, None, false)?;
    let observations = observe(root, files)?;
    let project = project::settings(root, preferences)?;
    let after = observe(root, files)?;
    let current = snapshot::workspace(root, None, false)?;
    let mut value = input.clone();
    let mut questions = input.get("questions").cloned().unwrap_or_else(|| json!([]));
    let questions = questions.as_array_mut().ok_or("acceptance:capacity")?;
    let stable = observations == after;
    if !stable {
        questions.push(json!(
            "Sources changed during composition; repeat grounding."
        ));
    }
    let mut binding = json!({"kind":"repository","identity":session::source(&before)});
    if !stable || digest(&before) != digest(&current) {
        binding["identity"] = Value::Null;
        questions.push(json!(
            "Repository changed during composition; freshness is unknown."
        ));
    }
    value["scope"] = request["scope"].clone();
    value["project"] = project;
    value["observations"] = after;
    value["observedAt"] = json!(timestamp::format(now_ms()));
    value["binding"] = binding.clone();
    value["questions"] = json!(questions);
    Ok(
        json!({"context":grounding::compose(&value)?,"binding":binding,"discovery":input["discovery"],"authorization":"Citations and declared checks only; no source text or check command was executed."}),
    )
}
