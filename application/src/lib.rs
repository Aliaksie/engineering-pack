use craft_engine::PROTOCOL_VERSION;
use serde_json::{Value, json};
mod budget;
pub mod cli;
mod evidence;
mod project;
mod telemetry;
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
        // One migrated CLI invocation; the JS forwarder writes the streams and posts `export`.
        Some("cli") => match request["input"]["argv"].as_array().and_then(|argv| {
            argv.iter()
                .map(|a| a.as_str().map(str::to_string))
                .collect::<Option<Vec<_>>>()
        }) {
            Some(argv) => {
                let outcome = cli::run_in_context(&argv, &request["input"]["telemetry"]);
                Ok(
                    json!({"exitCode":outcome.exit_code,"stdout":outcome.stdout,"stderr":outcome.stderr,"export":outcome.export,"errorCode":outcome.error_code}),
                )
            }
            None => Err("cli requires an argv string array".into()),
        },
        Some("locate") => {
            let result = (|| -> Result<Value, craft_platform::search::SearchError> {
                let input = &request["input"];
                let root = input["root"].as_str().ok_or("locate requires root")?;
                let search = craft_engine::discovery::request(&input["request"])?;
                let repositories = input
                    .get("repositories")
                    .map(craft_engine::discovery::repository_scopes)
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
