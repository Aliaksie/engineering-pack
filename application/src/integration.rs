//! Configured service integrations as an observed use-case step.
use crate::failure::Failure;
use crate::telemetry::{Operation, Recorder, observe};
use craft_engine::services;
use serde_json::{Map, Value, json};

/// Configured service metadata under `integration.prepared`, reporting operations whose
/// command or tool was not observed.
pub(crate) fn prepared(
    settings: &Value,
    commands: Option<&Value>,
    tools: Option<&Value>,
    recorder: &mut Recorder,
    parent: &Operation,
) -> Result<Value, Failure> {
    observe(
        recorder,
        "integration.prepared",
        parent,
        "INTEGRATION_UNAVAILABLE",
        Map::new(),
        |recorder, operation| {
            let result = services::prepare(settings, commands, tools)?;
            let unavailable = result["services"]
                .as_array()
                .into_iter()
                .flatten()
                .flat_map(|service| service["operations"].as_array().into_iter().flatten())
                .filter(|operation| operation["availability"] == "not-observed")
                .count();
            if unavailable > 0 {
                let attributes =
                    Map::from_iter([("unavailableCapabilities".to_string(), json!(unavailable))]);
                recorder.report(
                    "integration",
                    "INTEGRATION_UNAVAILABLE",
                    attributes,
                    operation,
                );
            }
            Ok(result)
        },
    )
}
