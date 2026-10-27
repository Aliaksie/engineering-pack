use craft_engine::PROTOCOL_VERSION;
use serde_json::{Value, json};
mod actions;
mod budget;
mod checkpoint;
pub mod cli;
mod control;
mod delivery;
mod discovery;
mod evidence;
mod failure;
mod grounding;
pub mod hook;
pub mod host;
mod project;
mod review;
mod scratch;
mod session;
mod status;
pub mod statusline;
mod task;
mod telemetry;
mod workers;
mod workspace;

/// One request read by `craft` itself. Worker and action phases post their own remote
/// telemetry export once `write` has the response, as hooks do; no caller forwards it.
pub fn serve(request: Value, write: impl FnOnce(&Value)) {
    let delivers = matches!(request["operation"].as_str(), Some("workers" | "actions"));
    let mut response = evaluate(request);
    let export = match response["result"].as_object_mut() {
        Some(result) if delivers => result.remove("export"),
        _ => None,
    };
    write(&response);
    telemetry::Recorder::deliver(export);
}

pub fn evaluate(request: Value) -> Value {
    if request.get("formatVersion").and_then(Value::as_u64) != Some(PROTOCOL_VERSION.into()) {
        return json!({"formatVersion":PROTOCOL_VERSION,"ok":false,"error":{"code":"UNSUPPORTED_VERSION","message":"expected formatVersion 1"}});
    }
    let result = match request.get("operation").and_then(Value::as_str) {
        Some(operation @ ("refresh" | "status" | "query" | "backup" | "rebuild")) => {
            if request["input"].get("members").is_some() {
                workspace::run(operation, &request["input"])
            } else {
                evidence::run(operation, &request["input"])
            }
        }
        Some("workers") => workers::run(&request["input"]),
        Some("actions") => actions::execute(&request["input"]),
        Some("control") => control::run(&request["input"]),
        Some("locate") => {
            let result = (|| -> Result<Value, craft_platform::search::SearchError> {
                let input = &request["input"];
                let root = input["root"].as_str().ok_or("locate requires root")?;
                let search = craft_engine::discovery::request(&input["request"])?;
                let repositories = input
                    .get("repositories")
                    .map(craft_engine::repository::scopes)
                    .transpose()?;
                craft_platform::search::scan(
                    std::path::Path::new(root),
                    &search,
                    repositories.as_deref(),
                )
            })();
            return match result {
                Ok(result) => json!({"formatVersion":1,"ok":true,"result":result}),
                Err(error) => {
                    json!({"formatVersion":1,"ok":false,"error":{"code":error.code(),"message":error.to_string()}})
                }
            };
        }
        Some("version") => Ok(
            json!({"binaryVersion":env!("CARGO_PKG_VERSION"),"schemaVersion":craft_engine::SCHEMA_VERSION,"provider":craft_platform::javascript::PROVIDER,"providers":craft_platform::providers::ACTIVE}),
        ),
        Some("extract") => {
            let input = &request["input"];
            match (input["path"].as_str(), input["source"].as_str()) {
                (Some(path), Some(source)) => {
                    craft_platform::providers::extract(path, source.as_bytes())
                        .and_then(|v| serde_json::to_value(v).map_err(|e| e.to_string()))
                }
                _ => Err("extract requires path and source strings".into()),
            }
        }
        _ => {
            return json!({"formatVersion":PROTOCOL_VERSION,"ok":false,"error":{"code":"UNKNOWN_OPERATION","message":"unknown native operation"}});
        }
    };
    match result {
        Ok(result) => json!({"formatVersion":PROTOCOL_VERSION,"ok":true,"result":result}),
        Err(message) => {
            json!({"formatVersion":PROTOCOL_VERSION,"ok":false,"error":{"code":"INVALID_INPUT","message":message}})
        }
    }
}
