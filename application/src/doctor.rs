//! `craft doctor`: observe how each host installed Craft for this project and report, from
//! files only, whether its hooks would run. Read-only: nothing is written to host or Craft
//! state, no host is started and nothing is fetched.
use crate::failure::Failure;
use craft_platform::{host_install, state, telemetry};
use serde_json::{Value, json};
use std::path::Path;

/// Repository overlays the exports install, by host.
const OVERLAYS: [(&str, &str); 3] = [
    ("cursor", ".cursor/hooks.json"),
    ("copilot", ".github/hooks/craft-capabilities.json"),
    ("gemini", ".gemini/settings.json"),
];

/// The latest `host.hook` event per host and package in the retained telemetry, with the
/// retained window; `local: false` when the project turned telemetry off.
fn hook_events(root: &Path) -> Value {
    let effective = crate::telemetry::effective(root);
    let Ok((records, _)) = telemetry::snapshot(root) else {
        return json!({"local": effective["local"], "lastSeen": [], "readable": false});
    };
    let mut latest: Vec<Value> = Vec::new();
    let mut first: Option<&str> = None;
    for record in &records {
        let at = record["timestamp"].as_str().unwrap_or_default();
        if first.is_none_or(|f| at < f) && !at.is_empty() {
            first = Some(at);
        }
        if record["name"] != "host.hook" {
            continue;
        }
        let row = json!({
            "host": record["host"], "package": record["attributes"]["package"],
            "lifecycle": record["attributes"]["lifecycle"], "packVersion": record["packVersion"],
            "at": at, "outcome": record["outcome"],
        });
        match latest
            .iter_mut()
            .find(|seen| seen["host"] == row["host"] && seen["package"] == row["package"])
        {
            Some(seen) if seen["at"].as_str().unwrap_or_default() < at => *seen = row,
            Some(_) => {}
            None => latest.push(row),
        }
    }
    json!({"local": effective["local"], "window": {"first": first}, "lastSeen": latest})
}

/// The doctor report for the project at `root`.
pub fn run(root: &Path) -> Result<Value, Failure> {
    let root = state::canonical_root(root)?;
    let overlays: Vec<Value> = OVERLAYS
        .iter()
        .map(|(host, relative)| host_install::overlay(host, &root, relative))
        .collect();
    let facts = json!({
        "project": root.to_string_lossy(),
        "craftVersion": crate::telemetry::pack_version(),
        "telemetry": hook_events(&root),
        "claude": host_install::claude(&root),
        "codex": host_install::codex(&root),
        "overlays": overlays,
    });
    Ok(craft_engine::doctor::report(&facts))
}

/// One line per host and package, then what to do and what stays unknown.
pub fn text(report: &Value) -> String {
    let mut out = format!(
        "craft doctor · {} · craft {}\n",
        report["project"].as_str().unwrap_or("?"),
        report["craftVersion"].as_str().unwrap_or("dev")
    );
    let line = |check: &Value| {
        format!(
            "{} {}: {}",
            match check["status"].as_str() {
                Some("ok") => "ok     ",
                Some("problem") => "PROBLEM",
                Some("off") => "off    ",
                _ => "unknown",
            },
            check["check"].as_str().unwrap_or("?"),
            check["detail"].as_str().unwrap_or_default()
        )
    };
    for host in report["hosts"].as_array().into_iter().flatten() {
        let name = host["host"].as_str().unwrap_or("?");
        if host["status"] == "not-installed" {
            out.push_str(&format!("{name:<9} not installed\n"));
        } else {
            out.push_str(&format!(
                "{name:<9} {}\n",
                host["status"].as_str().unwrap_or("?")
            ));
        }
        for package in host["packages"].as_array().into_iter().flatten() {
            out.push_str(&format!(
                "  {} {}\n",
                package["id"].as_str().unwrap_or("?"),
                package["version"].as_str().unwrap_or("?")
            ));
            for check in package["checks"].as_array().into_iter().flatten() {
                out.push_str(&format!("    {}\n", line(check)));
            }
        }
        for check in host["checks"].as_array().into_iter().flatten() {
            out.push_str(&format!("  {}\n", line(check)));
        }
    }
    for next in report["next"].as_array().into_iter().flatten() {
        out.push_str(&format!("next: {}\n", next.as_str().unwrap_or_default()));
    }
    let unknown: Vec<&str> = report["unknown"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .collect();
    out.push_str(&format!("not checked: {}\n", unknown.join("; ")));
    if report["blocking"] == true {
        out.push_str("blocking: an enabled package cannot run here\n");
    }
    out
}
