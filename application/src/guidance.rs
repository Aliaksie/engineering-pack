//! Guidance selection as observed use-case steps: the route plan and its measured context.
use crate::failure::Failure;
use crate::telemetry::{Operation, Recorder};
use craft_engine::{context, routing};
use serde_json::{Map, Value, json};

/// The route with its `routing.completed` operation and `guidance.selected` observations.
pub(crate) fn route(
    settings: &Value,
    request: &routing::Request,
    recorder: &mut Recorder,
    parent: &Operation,
) -> Result<Value, Failure> {
    let operation = recorder.start("routing.completed", Some(parent));
    let result = routing::route(settings, request);
    let (attributes, failure) = match &result {
        Ok(plan) => {
            let catalog = routing::catalog::catalog()?;
            let known = routing::catalog::skills(catalog);
            // Route skills are already unique.
            let skills = plan["skills"].as_array().map_or(&[][..], Vec::as_slice);
            for skill in skills.iter().take(32) {
                let attributes = Map::from_iter([("skill".to_string(), skill.clone())]);
                recorder.observe("guidance.selected", attributes, &operation, &known);
            }
            let attributes = Map::from_iter([
                ("intent".to_string(), plan["intent"].clone()),
                (
                    "capabilityClass".to_string(),
                    plan["capability"]["class"].clone(),
                ),
                ("selectedSkills".to_string(), json!(skills.len())),
            ]);
            (attributes, None)
        }
        Err(_) => (Map::new(), Some("OPERATION_FAILED")),
    };
    recorder.complete(operation, attributes, failure);
    Ok(result?)
}

/// Selected guidance files for `plan` in the verified `bundles`, observed as `context.observed`.
pub(crate) fn report(
    plan: &Value,
    bundles: &[String],
    budget: Option<&Value>,
    recorder: &mut Recorder,
    parent: &Operation,
) -> Result<Value, Failure> {
    let operation = recorder.start("context.observed", Some(parent));
    let result = (|| -> Result<Value, Failure> {
        let requests = context::requests(plan)?;
        let (entries, missing) = craft_platform::guidance::measure(bundles, &requests)?;
        let budget = budget.map(context::budget).transpose()?;
        Ok(context::summary(entries, missing, budget))
    })();
    let (attributes, failure) = match &result {
        Ok(report) => {
            let totals = &report["totals"];
            let attributes = Map::from_iter([
                ("guidanceFiles".to_string(), totals["files"].clone()),
                ("guidanceBytes".to_string(), totals["bytes"].clone()),
                (
                    "estimatedTokens".to_string(),
                    totals["estimatedTokens"].clone(),
                ),
            ]);
            (attributes, None)
        }
        Err(error) => (Map::new(), Some(error.code("CONTEXT_UNAVAILABLE"))),
    };
    recorder.complete(operation, attributes, failure);
    result
}
