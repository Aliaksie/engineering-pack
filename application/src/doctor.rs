//! `craft doctor`: observe how each host installed Craft for this project and report, from
//! files only, whether its hooks would run. Read-only unless asked: only
//! `--deny-secret-files` writes, merging Claude's secret-file deny rules into the project's
//! local Claude settings. No host is started and nothing is fetched.
use crate::failure::Failure;
use craft_platform::{host_install, host_settings, state, telemetry};
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
    // Hook runs that gave no answer, per host and package: input not read, or a failure.
    let mut unanswered: Vec<Value> = Vec::new();
    let mut first: Option<&str> = None;
    for record in &records {
        let at = record["timestamp"].as_str().unwrap_or_default();
        if first.is_none_or(|f| at < f) && !at.is_empty() {
            first = Some(at);
        }
        if record["name"] != "host.hook" {
            continue;
        }
        let status = record["attributes"]["inputStatus"]
            .as_str()
            .unwrap_or("complete");
        let unread = status != "complete";
        if unread || record["outcome"] == "failure" {
            let key = (&record["host"], &record["attributes"]["package"]);
            let index = unanswered
                .iter()
                .position(|row| (&row["host"], &row["package"]) == key)
                .unwrap_or_else(|| {
                    unanswered
                        .push(json!({"host": key.0, "package": key.1, "unread": 0, "failed": 0}));
                    unanswered.len() - 1
                });
            let row = &mut unanswered[index];
            let field = if unread { "unread" } else { "failed" };
            row[field] = json!(row[field].as_u64().unwrap_or(0) + 1);
            if row["at"].as_str().is_none_or(|seen| seen < at) {
                row["at"] = json!(at);
                row["lifecycle"] = record["attributes"]["lifecycle"].clone();
            }
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
    // Every installed package's hook sees the same event: a host's count is its package's
    // that saw the most.
    let mut per_host: Vec<Value> = Vec::new();
    for row in unanswered {
        let total =
            |row: &Value| row["unread"].as_u64().unwrap_or(0) + row["failed"].as_u64().unwrap_or(0);
        match per_host.iter_mut().find(|seen| seen["host"] == row["host"]) {
            Some(seen) if total(seen) < total(&row) => *seen = row,
            Some(_) => {}
            None => per_host.push(row),
        }
    }
    json!({"local": effective["local"], "window": {"first": first}, "lastSeen": latest, "unanswered": per_host})
}

/// The launcher a status-line command runs when it is Craft's own (`'<launcher>' statusline
/// claude …`, as `host-setup` writes it); `None` for any other command.
fn craft_launcher(command: &str) -> Option<&str> {
    if !command.contains(" statusline claude ") {
        return None;
    }
    match command.strip_prefix('\'') {
        Some(rest) => rest.split('\'').next(),
        None => command.split_whitespace().next(),
    }
}

/// The effective Claude status line: whether one is set, where, and when it runs Craft,
/// whether that launcher exists and which Craft version its bundle carries.
fn status_line(root: &Path) -> Value {
    let found = match host_settings::inspect(root) {
        Ok(found) => found,
        Err(error) => return json!({"error": error.to_string()}),
    };
    let mut out = json!({"configured": !found["statusLine"].is_null(), "source": found["source"]});
    let Some(launcher) = found["statusLine"]["command"]
        .as_str()
        .and_then(craft_launcher)
    else {
        return out;
    };
    // `<bundle>/bin/craft` or `craft.cmd`: the bundle's runtime manifest names its version.
    let bundle = state::physical(Path::new(launcher))
        .ok()
        .and_then(|path| path.parent()?.parent().map(Path::to_path_buf));
    out["launcher"] = json!(launcher);
    out["present"] = json!(bundle.is_some());
    if let Some(bundle) = bundle {
        out["packVersion"] = host_install::bundle(&bundle)["packVersion"].clone();
    }
    out
}

/// The doctor report for the project at `root`. With `deny_secret_files`, the consumer's
/// consent, Claude's secret-file deny rules are merged into `.claude/settings.local.json`
/// first, so the report reads the settings as written.
pub fn run(root: &Path, deny_secret_files: bool) -> Result<Value, Failure> {
    let root = state::canonical_root(root)?;
    let written = deny_secret_files
        .then(|| host_settings::deny(&root, &craft_engine::doctor::CLAUDE_SECRET_DENY))
        .transpose()?;
    let overlays: Vec<Value> = OVERLAYS
        .iter()
        .map(|(host, relative)| {
            let mut found = host_install::overlay(host, &root, relative);
            if *host == "cursor" {
                found["reincluded"] = json!(host_install::cursor_reincluded(&root));
            }
            found
        })
        .collect();
    // The layers as every command reads them; a file that cannot be used is reported, not raised.
    let configuration =
        match crate::project::layers(&root, crate::project::Sources::default(), false, None) {
            Ok(report) => {
                let settings = crate::project::settings(&root, None).ok();
                let on = |rule: fn(&Value) -> bool| settings.as_ref().is_some_and(rule);
                json!({
                    "layers": report["layers"],
                    "safety": on(craft_engine::safety::enabled),
                    "secretFiles": on(craft_engine::safety::guards_secret_files),
                })
            }
            Err(failure) => json!({"error": failure.message}),
        };
    let mut codex = host_install::codex(&root);
    // The newest install: sessions before it are not this install's gap.
    let installed = codex["packages"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|package| package["enabled"] == "true")
        .filter_map(|package| package["installedMs"].as_i64())
        .max();
    let safety = configuration["safety"] == true;
    if let Some(gap) = crate::hook_gap::found(&root, "codex", None, installed, safety) {
        codex["gap"] = gap;
    }
    let mut claude = host_install::claude(&root);
    claude["statusLine"] = status_line(&root);
    claude["windows"] = json!(cfg!(windows));
    let facts = json!({
        "configuration": configuration,
        "project": root.to_string_lossy(),
        "craftVersion": crate::telemetry::pack_version(),
        "telemetry": hook_events(&root),
        "claude": claude,
        "codex": codex,
        "overlays": overlays,
    });
    let mut report = craft_engine::doctor::report(&facts);
    if let Some(written) = written {
        report["secretFilesWritten"] = written;
    }
    Ok(report)
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
    let configuration = report["configuration"].as_array().into_iter().flatten();
    for (index, check) in configuration.enumerate() {
        if index == 0 {
            out.push_str("configuration\n");
        }
        out.push_str(&format!("  {}\n", line(check)));
    }
    let written = &report["secretFilesWritten"];
    if let Some(file) = written["file"].as_str() {
        let added: Vec<&str> = written["added"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .collect();
        out.push_str(&if added.is_empty() {
            format!("wrote nothing: {file} already denies Claude's secret-file reads\n")
        } else {
            format!("wrote to {file}: deny {}\n", added.join(", "))
        });
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
