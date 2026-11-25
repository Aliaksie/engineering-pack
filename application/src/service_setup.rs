//! Service setup discovery over facts on this machine: the Git remotes, the tracker identities
//! the project's prompts and plans named, the setup CLIs on `PATH` and MCP tools a host reports.
//! The proposal is a preview; only `--apply` adds the bindings a project lacks to
//! `.craft/project.json`, and a configured binding is never replaced.
use crate::failure::Failure;
use craft_engine::services::discovery;
use craft_platform::{process, repository, state};
use serde_json::{Value, json};
use std::path::Path;

/// Tracker identities named in prompts, kept per project.
const TRACKERS: &str = "notices/trackers.json";
/// The proposal a session start last said.
const SHOWN: &str = "notices/service-setup.json";
const RECORD_BYTES: usize = 8192;

/// The CLIs the setups run.
const COMMANDS: [&str; 3] = ["gh", "glab", "acli"];

fn unbound(settings: &Value) -> bool {
    ["codehost", "tasks"]
        .iter()
        .any(|service| settings.pointer(&format!("/services/{service}")).is_none())
}

/// Keep the tracker identities the URLs in `prompt` name, never the prompt itself. Nothing is
/// read or written when both services are bound or the prompt names no tracker.
pub(crate) fn remember(root: &Path, settings: &Value, prompt: &str) -> Result<(), Failure> {
    if !unbound(settings) {
        return Ok(());
    }
    let found = discovery::trackers(prompt);
    if found.is_empty() {
        return Ok(());
    }
    let file = state::file(root, TRACKERS, true, None)?;
    let Some(_lock) = state::lock(&file)? else {
        return Ok(());
    };
    // A record that cannot be read is left as it is, not replaced.
    let kept = crate::session::read_record(&file, RECORD_BYTES)?;
    if let Some(record) = discovery::keep(&kept, &found) {
        state::write(&file, &record, RECORD_BYTES, false)?;
    }
    Ok(())
}

/// The tracker identities named so far: kept from prompts, then with `plans` the ticket
/// links of work plans.
fn trackers(root: &Path, plans: bool) -> Vec<Value> {
    let record = state::file(root, TRACKERS, false, None)
        .ok()
        .and_then(|file| crate::session::read_record(&file, RECORD_BYTES).ok())
        .unwrap_or(Value::Null);
    let mut found = discovery::kept(&record);
    if plans {
        let linked = crate::plan::summaries(root)
            .unwrap_or_default()
            .iter()
            .flat_map(|plan| plan["references"].as_array().cloned().unwrap_or_default())
            .filter_map(|reference| discovery::tracker(reference.as_str()?.strip_prefix("link:")?))
            .collect::<Vec<_>>();
        for identity in linked {
            if !found.contains(&identity) {
                found.push(identity);
            }
        }
    }
    found
}

fn facts(root: &Path, tools: Option<&Value>, plans: bool) -> Value {
    let remotes: Vec<Value> = repository::remotes(root)
        .into_iter()
        .map(|(name, url)| json!({"name": name, "url": url}))
        .collect();
    let path = std::env::var_os("PATH");
    let commands: Vec<&str> = COMMANDS
        .into_iter()
        .filter(|command| process::on_path(command, path.as_deref()))
        .collect();
    let tools = tools
        .map(|tools| match tools {
            Value::Array(_) => tools.clone(),
            other => json!([other]),
        })
        .unwrap_or_else(|| json!([]));
    json!({"remotes": remotes, "trackers": trackers(root, plans), "commands": commands, "tools": tools})
}

/// The proposal for the project at `root` under `settings`, plan links included.
pub(crate) fn proposal(root: &Path, settings: &Value, tools: Option<&Value>) -> Value {
    discovery::propose(settings, &facts(root, tools, true))
}

/// `craft service-setup`: the proposal, and with `apply` the bindings it adds to
/// `.craft/project.json`. The written file must load again, or it is restored.
pub(crate) fn run(
    root: &Path,
    settings: &Value,
    tools: Option<&Value>,
    apply: bool,
) -> Result<Value, Failure> {
    let root = state::canonical_root(root)?;
    let mut report = proposal(&root, settings, tools);
    let file = root.join(".craft").join("project.json");
    report["file"] = json!(file);
    report["mode"] = json!(if apply { "apply" } else { "preview" });
    if !apply {
        return Ok(report);
    }
    let project = crate::project::configuration(&root)?;
    let Some(written) = discovery::apply(&project, &report) else {
        report["written"] = json!([]);
        return Ok(report);
    };
    let before = std::fs::read(&file).ok();
    replace(&file, &state::serialize(&written, true))?;
    if let Err(failure) = crate::project::settings(&root, None) {
        match before {
            Some(bytes) => replace(&file, &bytes),
            None => std::fs::remove_file(&file)
                .map_err(|error| format!("cannot restore {}: {error}", file.display()).into()),
        }?;
        return Err(failure);
    }
    let added: Vec<&String> = report["merge"]["services"]
        .as_object()
        .into_iter()
        .flat_map(|services| services.keys())
        .filter(|service| project.pointer(&format!("/services/{service}")).is_none())
        .collect();
    report["written"] = json!(added);
    Ok(report)
}

/// Replace `file` whole: written beside it, then renamed over it, so a failure midway leaves
/// the old file. A new file is readable by others, as a committed project file is.
fn replace(file: &Path, bytes: &[u8]) -> Result<(), Failure> {
    let folder = file.parent().expect("project file folder");
    std::fs::create_dir_all(folder)
        .map_err(|error| format!("cannot create {}: {error}", folder.display()))?;
    let staged = folder.join(format!(".project.json.{}.tmp", std::process::id()));
    let result = std::fs::write(&staged, bytes)
        .and_then(|()| {
            if let Ok(meta) = std::fs::metadata(file) {
                std::fs::set_permissions(&staged, meta.permissions())?;
            }
            std::fs::rename(&staged, file)
        })
        .map_err(|error| format!("cannot write {}: {error}", file.display()).into());
    let _ = std::fs::remove_file(&staged);
    result
}

/// What a session start says about an unconfigured service setup: the proposal once per
/// distinct proposal, recorded as said. `None` when the services are bound, nothing is
/// supported, it was said already or another hook is saying it.
pub(crate) fn offer(root: &Path, settings: &Value) -> Result<Option<Value>, Failure> {
    if !unbound(settings) {
        return Ok(None);
    }
    let file = state::file(root, SHOWN, true, None)?;
    let Some(_lock) = state::lock(&file)? else {
        return Ok(None);
    };
    // A session start reads no plans: the trackers prompts named are already kept.
    let proposal = discovery::propose(settings, &facts(root, None, false));
    let shown = crate::session::read_record(&file, RECORD_BYTES)?;
    let (Some(record), Some(line)) = (
        discovery::offer(&proposal, &shown),
        discovery::line(&proposal),
    ) else {
        return Ok(None);
    };
    state::write(&file, &record, RECORD_BYTES, false)?;
    Ok(Some(json!({"line": line, "merge": proposal["merge"]})))
}

/// The receipt: each service's choice with its facts, then what was written.
pub(crate) fn text(report: &Value) -> String {
    let mut out = String::new();
    for (service, entry) in report["services"].as_object().into_iter().flatten() {
        match entry["status"].as_str() {
            Some("configured") => {
                out.push_str(&format!("{service}: configured; kept as it is\n"));
            }
            Some("proposed") => {
                out.push_str(&format!("{service}: {}\n", entry["binding"]));
                for fact in entry["facts"].as_array().into_iter().flatten() {
                    out.push_str(&format!("  because {}\n", fact.as_str().unwrap_or("?")));
                }
                if let Some(missing) = entry["missing"].as_str() {
                    out.push_str(&format!("  but {missing}\n"));
                }
            }
            _ => out.push_str(&format!(
                "{service}: nothing proposed: {}\n",
                entry["reason"].as_str().unwrap_or("?")
            )),
        }
    }
    let file = report["file"].as_str().unwrap_or(".craft/project.json");
    match (report["mode"].as_str(), report["written"].as_array()) {
        (Some("apply"), Some(written)) if written.is_empty() => {
            out.push_str(&format!("wrote nothing to {file}\n"));
        }
        (Some("apply"), Some(written)) => {
            let names: Vec<&str> = written.iter().filter_map(Value::as_str).collect();
            out.push_str(&format!("wrote services {} to {file}\n", names.join(", ")));
        }
        _ if report.get("merge").is_some() => {
            out.push_str(&format!(
                "preview only: run with --apply to add it to {file}\n"
            ));
        }
        _ => {}
    }
    out
}
