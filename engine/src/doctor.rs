//! `craft doctor`: whether each host would run Craft for this project, decided from
//! observations of its install records, settings and Craft's own hook telemetry. Every check
//! is `ok`, `problem`, `unknown` or `off`; only a fact that stops Craft from running where it
//! is enabled (a bundled runtime that is missing, unrunnable or altered, or a registered hook
//! command whose package is gone) is blocking.

use serde_json::{Value, json};

fn check(name: &str, status: &str, detail: impl Into<String>) -> Value {
    json!({"check": name, "status": status, "detail": detail.into()})
}

/// The package name of a host plugin id: `craft-delivery@market` → `craft-delivery`.
fn package(id: &str) -> &str {
    id.split('@').next().unwrap_or(id)
}

/// A bundle's launcher and runtime for this platform: the check and whether it blocks.
fn runtime(bundle: &Value) -> (Value, bool) {
    let executable = bundle["executable"].as_str().unwrap_or("missing");
    let detail = match (bundle["runtime"].as_str(), executable) {
        (Some("missing"), _) => "bundle has no native runtime manifest".to_owned(),
        (Some("unreadable"), _) => "native runtime manifest is unreadable".to_owned(),
        (_, "unsupported-platform") => "no bundled runtime for this platform".to_owned(),
        (_, "ok") if bundle["sha256"] != "ok" => {
            "bundled runtime does not match its manifest digest".to_owned()
        }
        (_, "ok") if bundle["launcher"] != true => "launcher bin/craft is missing".to_owned(),
        (_, "ok") => {
            return (
                check(
                    "runtime",
                    "ok",
                    format!(
                        "{} runtime present, digest matches",
                        bundle["target"].as_str().unwrap_or("bundled")
                    ),
                ),
                false,
            );
        }
        (_, other) => format!("bundled runtime is {other}"),
    };
    (check("runtime", "problem", detail), true)
}

/// The latest hook event of `host` for `name`, from the retained telemetry rows.
fn last_seen<'a>(seen: &'a [Value], host: &str, name: &str) -> Option<&'a Value> {
    seen.iter()
        .filter(|e| e["host"] == host && e["package"] == name)
        .max_by(|a, b| a["at"].as_str().cmp(&b["at"].as_str()))
}

/// Whether the host ran this package's hooks, and with which version.
fn firing(
    seen: &[Value],
    telemetry: &Value,
    host: &str,
    name: &str,
    version: &Value,
    installed_at: &Value,
) -> (Value, Option<String>) {
    if telemetry["local"] == false {
        return (
            check("hooks", "unknown", "telemetry is off for this project"),
            None,
        );
    }
    let Some(event) = last_seen(seen, host, name) else {
        let window = telemetry["window"]["first"]
            .as_str()
            .map_or(String::new(), |f| format!(" since {f}"));
        return (
            check(
                "hooks",
                "unknown",
                format!("no hook event recorded in this project{window}"),
            ),
            None,
        );
    };
    let at = event["at"].as_str().unwrap_or_default();
    let ran = event["packVersion"].as_str().unwrap_or("unknown");
    if Some(ran) != version.as_str() && installed_at.as_str().is_some_and(|i| at < i) {
        let restart = if host == "claude" {
            "run /reload-plugins or restart Claude Code"
        } else {
            "restart the Codex session"
        };
        return (
            check(
                "hooks",
                "problem",
                format!(
                    "last hook {} {at} ran {ran}, installed {}: the session predates the install",
                    event["lifecycle"].as_str().unwrap_or("event"),
                    version.as_str().unwrap_or("?")
                ),
            ),
            Some(restart.into()),
        );
    }
    // An install with no recorded time (Codex): an older version's event says nothing about
    // whether the installed one runs.
    if let Some(installed) = version.as_str().filter(|installed| *installed != ran) {
        return (
            check(
                "hooks",
                "unknown",
                format!(
                    "last hook {} at {at} ran {ran}, not the installed {installed}: no hook of the installed version is known to have run in this project",
                    event["lifecycle"].as_str().unwrap_or("event")
                ),
            ),
            None,
        );
    }
    (
        check(
            "hooks",
            "ok",
            format!(
                "last hook {} at {at} ran {ran}",
                event["lifecycle"].as_str().unwrap_or("event")
            ),
        ),
        None,
    )
}

/// A host with no install found: `not-installed`, still naming hook events this project
/// recorded for it, which came from an install these files do not show.
fn absent(host: &str, seen: &[Value]) -> Value {
    let checks: Vec<Value> = seen
        .iter()
        .filter(|event| event["host"] == host)
        .max_by(|a, b| a["at"].as_str().cmp(&b["at"].as_str()))
        .map(|event| {
            check(
                "hooks",
                "unknown",
                format!(
                    "last hook {} at {} ran {} {}, but no install was found: installed elsewhere (another user, scope or settings) or removed since",
                    event["lifecycle"].as_str().unwrap_or("event"),
                    event["at"].as_str().unwrap_or("?"),
                    event["package"].as_str().unwrap_or("?"),
                    event["packVersion"].as_str().unwrap_or("?")
                ),
            )
        })
        .into_iter()
        .collect();
    json!({"host": host, "status": "not-installed", "packages": [], "checks": checks})
}

/// A host's report from its checks: `problem` beats `unknown` beats `ok`.
fn host_status(checks: &[Value]) -> &'static str {
    let has = |s: &str| checks.iter().any(|c| c["status"] == s);
    if has("problem") {
        "problem"
    } else if has("unknown") {
        "unknown"
    } else if has("ok") {
        "ok"
    } else {
        "off"
    }
}

/// Dotted version numbers compared as numbers: `0.0.10` is newer than `0.0.9`. A segment
/// that is not a number compares as text after every number.
fn version_order(a: &str, b: &str) -> std::cmp::Ordering {
    let key = |v: &str| -> Vec<(u64, String)> {
        v.split(['.', '-', '+'])
            .map(|part| {
                part.parse()
                    .map_or((u64::MAX, part.to_owned()), |n| (n, String::new()))
            })
            .collect()
    };
    key(a).cmp(&key(b))
}

/// Each cached package once, at its newest version, with the older versions the cache keeps:
/// a version Codex no longer loads cannot block a package that runs.
fn newest(found: &[Value]) -> Vec<(&Value, Vec<&str>)> {
    let mut kept: Vec<(&Value, Vec<&str>)> = Vec::new();
    for entry in found {
        let version = |e: &Value| e["version"].as_str().unwrap_or_default().to_owned();
        match kept.iter_mut().find(|(k, _)| k["id"] == entry["id"]) {
            Some((current, older)) => {
                if version_order(&version(entry), &version(current)).is_gt() {
                    older.push(current["version"].as_str().unwrap_or_default());
                    *current = entry;
                } else {
                    older.push(entry["version"].as_str().unwrap_or_default());
                }
            }
            None => kept.push((entry, Vec::new())),
        }
    }
    kept
}

struct Report {
    blocking: bool,
    next: Vec<String>,
}

fn claude(facts: &Value, seen: &[Value], telemetry: &Value, report: &mut Report) -> Value {
    if facts["installed"] != true || facts["packages"].as_array().is_none_or(Vec::is_empty) {
        return absent("claude", seen);
    }
    let layers = facts["settings"].as_array().cloned().unwrap_or_default();
    let mut packages = Vec::new();
    let mut enabled_ids: Vec<&str> = Vec::new();
    for found in facts["packages"].as_array().into_iter().flatten() {
        let id = found["id"].as_str().unwrap_or_default();
        // Settings merge key by key; the last layer naming the plugin decides.
        let decided = layers.iter().rev().find_map(|layer| {
            layer["enabledPlugins"]
                .get(id)
                .map(|v| (v.clone(), layer["layer"].clone()))
        });
        let mut checks = Vec::new();
        let enabled = match &decided {
            Some((Value::Bool(true), layer)) => {
                checks.push(check(
                    "enabled",
                    "ok",
                    format!("enabled in {} settings", layer.as_str().unwrap_or("?")),
                ));
                true
            }
            Some((_, layer)) => {
                checks.push(check(
                    "enabled",
                    "off",
                    format!("disabled in {} settings", layer.as_str().unwrap_or("?")),
                ));
                false
            }
            None => {
                checks.push(check("enabled", "unknown", "no enabledPlugins entry in user, project or local settings (managed settings and --settings not inspected)"));
                true
            }
        };
        if enabled {
            enabled_ids.push(id);
            let (runtime, blocks) = runtime(&found["bundle"]);
            if blocks {
                report.blocking = true;
                report.next.push(format!(
                    "Reinstall {id}: Claude Code refuses actions whose hook command fails"
                ));
            }
            checks.push(runtime);
            let (hooks, next) = firing(
                seen,
                telemetry,
                "claude",
                package(id),
                &found["version"],
                &found["installedAt"],
            );
            checks.push(hooks);
            report.next.extend(next);
        }
        let status = host_status(&checks);
        packages.push(json!({"id": id, "scope": found["scope"], "version": found["version"], "status": status, "checks": checks}));
    }
    let mut checks = Vec::new();
    let registered: Vec<String> = layers
        .iter()
        .filter(|layer| {
            layer["commands"].as_array().into_iter().flatten().any(|c| {
                c.as_str()
                    .is_some_and(|c| c.contains(" hook claude ") && c.contains("craft"))
            })
        })
        .filter_map(|layer| layer["layer"].as_str().map(str::to_owned))
        .collect();
    if registered.is_empty() {
        checks.push(check(
            "duplicates",
            "ok",
            "no Craft hook command in Claude settings files",
        ));
    } else {
        checks.push(check(
            "duplicates",
            "problem",
            format!(
                "Craft hook commands also registered in {} settings: those events run twice",
                registered.join(", ")
            ),
        ));
    }
    // One package id enabled at several scopes is one plugin; only distinct ids that name the
    // same package come from two marketplaces.
    enabled_ids.sort_unstable();
    enabled_ids.dedup();
    let mut names: Vec<&str> = enabled_ids.iter().map(|id| package(id)).collect();
    names.sort_unstable();
    names.dedup();
    if names.len() < enabled_ids.len() {
        checks.push(check(
            "duplicates",
            "problem",
            "the same Craft package is enabled from two marketplaces; one copy's hooks do not run",
        ));
    }
    let mut all: Vec<Value> = checks.clone();
    for p in &packages {
        all.extend(p["checks"].as_array().cloned().unwrap_or_default());
    }
    json!({"host": "claude", "status": host_status(&all), "packages": packages, "checks": checks})
}

/// Whether Codex would run a package's hooks: each needs a recorded approval, and Codex ties
/// an approval to the plugin's files, so one given to other content (before an update) no
/// longer counts and its hook is skipped without a message.
fn approval(found: &Value, id: &str, hooks: u64, report: &mut Report) -> Value {
    let missing = found["unapproved"].as_u64().unwrap_or(0);
    if missing > 0 {
        report
            .next
            .push(format!("Review and approve {id} hooks in Codex `/hooks`"));
        return check(
            "approval",
            "problem",
            format!("{missing} of {hooks} hooks have no recorded approval (observed layout)"),
        );
    }
    let stale = found["staleApprovals"].as_u64().unwrap_or(0);
    if stale > 0 {
        report.next.push(format!(
            "Approve {id} hooks again in Codex `/hooks`: an approval covers the plugin content it was given to, so each update needs a new one"
        ));
        return check(
            "approval",
            "problem",
            format!(
                "{stale} of {hooks} hook approvals were given to other plugin content (the plugin changed since): Codex skips those hooks until they are approved again"
            ),
        );
    }
    let content = &found["approvedContent"];
    match (content.as_str(), content["error"].as_str()) {
        (Some("match"), _) => check(
            "approval",
            "ok",
            format!("{hooks} hook approvals recorded, given to the installed plugin content"),
        ),
        (_, Some(error)) => check(
            "approval",
            "unknown",
            format!(
                "{hooks} hook approvals recorded; the plugin content could not be compared ({error})"
            ),
        ),
        _ => check(
            "approval",
            "ok",
            format!(
                "{hooks} hook approvals recorded (this Codex recorded no plugin content with them; observed layout)"
            ),
        ),
    }
}

fn codex(facts: &Value, seen: &[Value], telemetry: &Value, report: &mut Report) -> Value {
    let cached = facts["packages"].as_array().map_or(&[][..], Vec::as_slice);
    // Enabled in the configuration, absent from the plugin cache: Codex loads nothing of it
    // and says nothing, so Craft is silent without any failure to see.
    let uncached: Vec<&str> = facts["configured"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|plugin| plugin["enabled"] == "true")
        .filter_map(|plugin| plugin["id"].as_str())
        .filter(|id| !cached.iter().any(|package| package["id"] == *id))
        .collect();
    if facts["installed"] != true || (cached.is_empty() && uncached.is_empty()) {
        return absent("codex", seen);
    }
    let mut checks = Vec::new();
    if !uncached.is_empty() {
        checks.push(check(
            "installed",
            "problem",
            format!(
                "{} enabled in config.toml but not in the plugin cache: Codex loads none of its skills or hooks",
                uncached.join(", ")
            ),
        ));
        report.next.push(format!(
            "Install what config.toml enables, then restart Codex: {}",
            uncached
                .iter()
                .map(|id| format!("`codex plugin add {id}`"))
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    match facts["trust"].as_str() {
        Some("trusted") => checks.push(check("trust", "ok", "project trusted")),
        Some(level) => checks.push(check(
            "trust",
            "problem",
            format!("project {level}: Codex skips the project's .codex layers and AGENTS.md"),
        )),
        None => checks.push(check(
            "trust",
            "unknown",
            "no trust entry for this exact path (a parent entry may apply)",
        )),
    }
    if facts["hooksFeature"] == "false" {
        checks.push(check(
            "hooks-feature",
            "problem",
            "[features] hooks = false: no plugin hook runs",
        ));
        report
            .next
            .push("Remove `hooks = false` from [features] in the Codex config".into());
    }
    let mut packages = Vec::new();
    for (found, older) in newest(cached) {
        let id = found["id"].as_str().unwrap_or_default();
        let mut pchecks = Vec::new();
        let enabled = match found["enabled"].as_str() {
            Some("true") => {
                pchecks.push(check("enabled", "ok", "enabled in config.toml"));
                true
            }
            Some(_) => {
                pchecks.push(check("enabled", "off", "disabled in config.toml"));
                false
            }
            None => {
                pchecks.push(check(
                    "enabled",
                    "off",
                    "in the plugin cache, not configured in config.toml",
                ));
                false
            }
        };
        if enabled {
            let (runtime, blocks) = runtime(&found["bundle"]);
            if blocks {
                report.blocking = true;
                report.next.push(format!(
                    "Reinstall {id}: its bundled runtime cannot run here"
                ));
            }
            pchecks.push(runtime);
            let hooks = found["hooks"].as_u64().unwrap_or(0);
            pchecks.push(approval(found, id, hooks, report));
            let (firing, next) = firing(
                seen,
                telemetry,
                "codex",
                package(id),
                &found["version"],
                &Value::Null,
            );
            pchecks.push(firing);
            report.next.extend(next);
        }
        let mut entry = json!({"id": id, "version": found["version"], "status": host_status(&pchecks), "checks": pchecks});
        if !older.is_empty() {
            entry["olderCachedVersions"] = json!(older);
        }
        packages.push(entry);
    }
    let duplicated: Vec<&str> = facts["commands"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|c| {
            c["command"]
                .as_str()
                .is_some_and(|c| c.contains(" hook codex ") && c.contains("craft"))
        })
        .filter_map(|c| c["path"].as_str())
        .collect();
    if duplicated.is_empty() {
        checks.push(check(
            "duplicates",
            "ok",
            "no Craft hook command in Codex hook files",
        ));
    } else {
        checks.push(check(
            "duplicates",
            "problem",
            format!(
                "Craft hook commands also in {}: Codex runs every matching hook",
                duplicated.join(", ")
            ),
        ));
    }
    let mut all = checks.clone();
    for p in &packages {
        all.extend(p["checks"].as_array().cloned().unwrap_or_default());
    }
    json!({"host": "codex", "status": host_status(&all), "packages": packages, "checks": checks})
}

/// A repository overlay (Cursor, Copilot CLI, Gemini CLI): its bundles and whether it fired.
fn overlay(facts: &Value, seen: &[Value], telemetry: &Value, report: &mut Report) -> Value {
    let host = facts["host"].as_str().unwrap_or_default();
    if facts["present"] != true {
        let mut report = absent(host, seen);
        report
            .as_object_mut()
            .expect("host report")
            .remove("packages");
        return report;
    }
    let mut checks = Vec::new();
    if let Some(error) = facts["error"].as_str() {
        checks.push(check("overlay", "problem", error));
    }
    for bundle in facts["bundles"].as_array().into_iter().flatten() {
        let (mut runtime, blocks) = runtime(bundle);
        runtime["detail"] = json!(format!(
            "{}: {}",
            bundle["path"].as_str().unwrap_or("?"),
            runtime["detail"].as_str().unwrap_or_default()
        ));
        if blocks {
            report.blocking = true;
            report.next.push(format!("Rebuild or reinstall the {host} overlay: a hook command's package has no runnable runtime"));
        }
        checks.push(runtime);
    }
    let fired = seen
        .iter()
        .filter(|e| e["host"] == host)
        .max_by(|a, b| a["at"].as_str().cmp(&b["at"].as_str()));
    checks.push(match (telemetry["local"] == false, fired) {
        (true, _) => check("hooks", "unknown", "telemetry is off for this project"),
        (false, Some(event)) => check(
            "hooks",
            "ok",
            format!(
                "last hook {} at {} ran {}",
                event["lifecycle"].as_str().unwrap_or("event"),
                event["at"].as_str().unwrap_or("?"),
                event["packVersion"].as_str().unwrap_or("?")
            ),
        ),
        (false, None) => check("hooks", "unknown", "no hook event recorded in this project"),
    });
    json!({"host": host, "status": host_status(&checks), "path": facts["path"], "checks": checks})
}

/// What the configuration layers say about this project: a file that cannot be used, fields
/// of an organization or user file this build left out, and a safety policy no host hook has
/// applied here yet.
fn configuration(facts: &Value, hosts: &[Value], report: &mut Report) -> Vec<Value> {
    let found = &facts["configuration"];
    let mut checks = Vec::new();
    if let Some(error) = found["error"].as_str() {
        checks.push(check("layers", "problem", error));
        report.next.push(
            "Fix the configuration file named above: until it loads, hooks deliver nothing and say so".into(),
        );
        return checks;
    }
    for layer in found["layers"].as_array().into_iter().flatten() {
        let ignored: Vec<&str> = layer["ignored"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .collect();
        if !ignored.is_empty() {
            checks.push(check(
                "layers",
                "unknown",
                format!(
                    "{} layer {}: {} not known to craft {} and ignored (a newer build may read it; otherwise misspelled)",
                    layer["name"].as_str().unwrap_or("?"),
                    layer["file"].as_str().unwrap_or("?"),
                    ignored.join(", "),
                    facts["craftVersion"].as_str().unwrap_or("dev"),
                ),
            ));
        }
    }
    if found["safety"] == true {
        for host in hosts {
            if matches!(host["status"].as_str(), Some("not-installed" | "off")) {
                continue;
            }
            let own = host["checks"].as_array().into_iter().flatten();
            let packaged = host["packages"]
                .as_array()
                .into_iter()
                .flatten()
                .flat_map(|package| package["checks"].as_array().into_iter().flatten());
            let mut hooks = own
                .chain(packaged)
                .filter(|c| c["check"] == "hooks")
                .peekable();
            if hooks.peek().is_some() && !hooks.any(|c| c["status"] == "ok") {
                checks.push(check(
                    "safety",
                    "unknown",
                    format!(
                        "the safety policy is on, but no hook of the installed {} package is known to have run in this project: until one does, {} commands are not checked against it",
                        host["host"].as_str().unwrap_or("host"),
                        host["host"].as_str().unwrap_or("its"),
                    ),
                ));
            }
        }
    }
    checks
}

/// The doctor report over observed facts: `{formatVersion, readOnly, project, craftVersion,
/// hosts, configuration, blocking, next, unknown}`.
pub fn report(facts: &Value) -> Value {
    let seen = facts["telemetry"]["lastSeen"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    let telemetry = &facts["telemetry"];
    let mut state = Report {
        blocking: false,
        next: Vec::new(),
    };
    let mut hosts = vec![
        claude(&facts["claude"], &seen, telemetry, &mut state),
        codex(&facts["codex"], &seen, telemetry, &mut state),
    ];
    for found in facts["overlays"].as_array().into_iter().flatten() {
        hosts.push(overlay(found, &seen, telemetry, &mut state));
    }
    let configuration = configuration(facts, &hosts, &mut state);
    state.next.dedup();
    json!({
        "formatVersion": 1, "readOnly": true, "project": facts["project"], "craftVersion": facts["craftVersion"],
        "hosts": hosts, "configuration": configuration, "blocking": state.blocking, "next": state.next,
        "unknown": [
            "whether a running host session loaded the current install (hosts keep no record on disk)",
            "whether a Codex hook's declaration still matches its approval (only the plugin content is compared)",
            "managed and command-line settings",
        ],
    })
}

#[cfg(test)]
mod tests;
