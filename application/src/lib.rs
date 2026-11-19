//! The `craft` use cases over the engine and platform: one JSON request in, one response
//! out, plus the command line, host hook and status line entry points.
use craft_engine::PROTOCOL_VERSION;
use failure::Failure;
use serde_json::{Value, json};
mod actions;
mod checkpoint;
mod checks;
pub mod cli;
mod control;
mod conventions;
mod delegation;
mod dependencies;
mod discovery;
mod doctor;
mod evidence;
mod failure;
mod grounding;
mod guidance;
mod handoff;
pub mod hook;
mod hook_gap;
mod host_context;
mod integration;
mod plan;
mod project;
mod question;
mod render;
mod retention;
mod review;
mod scratch;
mod session;
mod setup;
mod status;
pub mod statusline;
mod task;
mod telemetry;
mod test_runs;
mod ticket;
mod workers;

/// Set by the package launcher: the root of the installed package this executable serves.
pub(crate) const PACKAGE_ROOT: &str = "CRAFT_PACKAGE_ROOT";

/// What one `craft` invocation writes and exits with.
pub struct Outcome {
    pub exit_code: i32,
    pub stdout: String,
    pub stderr: String,
    /// Remote-bound telemetry for the caller to post, when configured.
    pub export: Option<Value>,
}

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

/// The executable's identity; `templates` names the exact model-facing wording it embeds.
fn version() -> Value {
    json!({"binaryVersion":env!("CARGO_PKG_VERSION"),"schemaVersion":craft_engine::SCHEMA_VERSION,"provider":craft_platform::providers::javascript::PROVIDER,"providers":*craft_platform::providers::ACTIVE,
        "templates":craft_engine::text::identities(&hook::text::templates())})
}

/// One Project Intelligence operation over a single repository or explicit members.
fn project_evidence(operation: &str, input: &Value) -> Result<Value, Failure> {
    match operation {
        "version" => Ok(version()),
        _ if input.get("members").is_some() => evidence::members::run(operation, input),
        _ => evidence::run(operation, input),
    }
}

pub fn evaluate(request: Value) -> Value {
    if request.get("formatVersion").and_then(Value::as_u64) != Some(PROTOCOL_VERSION.into()) {
        return json!({"formatVersion":PROTOCOL_VERSION,"ok":false,"error":{"code":"UNSUPPORTED_VERSION","message":"expected formatVersion 1"}});
    }
    let result = match request.get("operation").and_then(Value::as_str) {
        Some(operation @ ("refresh" | "warm" | "status" | "query" | "backup" | "rebuild")) => {
            project_evidence(operation, &request["input"])
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
                let root = std::path::Path::new(root);
                let secrets = crate::project::secret_files(root);
                craft_platform::search::scan(root, &search, repositories.as_deref(), &secrets)
            })();
            return match result {
                Ok(result) => json!({"formatVersion":1,"ok":true,"result":result}),
                Err(error) => {
                    json!({"formatVersion":1,"ok":false,"error":{"code":error.code(),"message":error.to_string()}})
                }
            };
        }
        Some("version") => Ok(version()),
        Some("extract") => {
            let input = &request["input"];
            match (input["path"].as_str(), input["source"].as_str()) {
                (Some(path), Some(source)) => {
                    craft_platform::providers::extract(path, source.as_bytes())
                        .map_err(Failure::from)
                        .and_then(|v| serde_json::to_value(v).map_err(|e| e.to_string().into()))
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
        Err(failure) => {
            json!({"formatVersion":PROTOCOL_VERSION,"ok":false,"error":{"code":"INVALID_INPUT","message":failure.message}})
        }
    }
}
