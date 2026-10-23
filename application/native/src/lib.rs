use craft_engine::PROTOCOL_VERSION;
use serde_json::{Value, json};
mod budget;
mod evidence;
mod workspace;

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
        Some("version") => Ok(
            json!({"binaryVersion":env!("CARGO_PKG_VERSION"),"schemaVersion":craft_engine::SCHEMA_VERSION,"provider":craft_platform::javascript::PROVIDER}),
        ),
        Some("extract") => {
            let input = &request["input"];
            match (input["path"].as_str(), input["source"].as_str()) {
                (Some(path), Some(source)) => {
                    craft_platform::javascript::extract(path, source.as_bytes())
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
