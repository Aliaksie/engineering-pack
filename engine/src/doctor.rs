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

/// The version an installed package runs as: the one its bundle states, which hook events
/// record, else the host's own name for the install (a cache folder can be named otherwise).
fn installed_version(found: &Value) -> &Value {
    match &found["bundle"]["packVersion"] {
        Value::String(_) => &found["bundle"]["packVersion"],
        _ => &found["version"],
    }
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
    // An event of another version says nothing about whether the installed one runs. An
    // event that names no version is not held against it.
    if let Some(installed) = version
        .as_str()
        .filter(|installed| *installed != ran && ran != "unknown")
    {
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

/// A shell word in single quotes, as `host-setup` quotes the launcher.
fn quoted(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\"'\"'"))
}

/// Claude's status line is how Craft observes context usage there; without it Craft stays
/// silent about compaction. Doctor offers the setup command and never writes settings.
fn status_line(
    facts: &Value,
    project: &Value,
    offer: Option<(&str, &str, Option<&str>)>,
    report: &mut Report,
) -> Option<Value> {
    let (id, bundle, installed) = offer?;
    let found = &facts["statusLine"];
    if found.is_null() {
        return None;
    }
    if let Some(error) = found["error"].as_str() {
        return Some(check(
            "status-line",
            "unknown",
            format!("Claude settings not readable: {error}"),
        ));
    }
    let launcher = if facts["windows"] == true {
        format!("{bundle}/bin/craft.cmd")
    } else {
        format!("{bundle}/bin/craft")
    };
    let name = package(id);
    if found["configured"] != true {
        let setup = format!(
            "{} host-setup --host claude --package {name} --root {}",
            quoted(&launcher),
            quoted(project.as_str().unwrap_or(".")),
        );
        report.next.push(if facts["windows"] == true {
            format!("Optional, so Craft sees context usage and can advise compaction: follow the status-line guidance of {setup}")
        } else {
            format!("Optional, so Craft sees context usage and can advise compaction: {setup} --apply (adds Craft's status line to .claude/settings.local.json; run it without --apply to preview)")
        });
        return Some(check(
            "status-line",
            "off",
            "no status line: Craft sees no context usage in Claude and says nothing about compaction",
        ));
    }
    let source = found["source"].as_str().unwrap_or("Claude settings");
    let Some(current) = found["launcher"].as_str() else {
        return Some(check(
            "status-line",
            "off",
            format!(
                "a custom status line ({source}) is set: Craft sees no context usage unless it calls `craft statusline claude --native-stdin`"
            ),
        ));
    };
    let replace = format!(
        "Set statusLine.command in {source} to: {} statusline claude --package {} --native-stdin",
        quoted(&launcher),
        quoted(name),
    );
    if found["present"] != true {
        report.next.push(replace);
        return Some(check(
            "status-line",
            "problem",
            format!(
                "the status line runs {current}, which no longer exists: Claude shows no status line and Craft sees no context usage"
            ),
        ));
    }
    // Any copy of the installed version is fine (a marketplace directory, the host's cache).
    let ran = found["packVersion"].as_str();
    match ran {
        Some(version) if Some(version) == installed => Some(check(
            "status-line",
            "ok",
            format!("Craft's status line ({source}) reports context usage"),
        )),
        Some(version) => {
            report.next.push(replace);
            Some(check(
                "status-line",
                "problem",
                format!(
                    "the status line runs craft {version} from {current}, not the installed {}",
                    installed.unwrap_or("version")
                ),
            ))
        }
        None => Some(check(
            "status-line",
            "unknown",
            format!("the status line runs {current}, whose version cannot be read"),
        )),
    }
}

fn claude(
    facts: &Value,
    project: &Value,
    seen: &[Value],
    telemetry: &Value,
    report: &mut Report,
) -> Value {
    if facts["installed"] != true || facts["packages"].as_array().is_none_or(Vec::is_empty) {
        return absent("claude", seen);
    }
    let layers = facts["settings"].as_array().cloned().unwrap_or_default();
    let mut packages = Vec::new();
    let mut enabled_ids: Vec<&str> = Vec::new();
    let mut offer: Option<(&str, &str, Option<&str>)> = None;
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
            if let (None, Some(path)) = (offer, found["bundle"]["path"].as_str()) {
                offer = Some((id, path, found["bundle"]["packVersion"].as_str()));
            }
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
                installed_version(found),
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
    checks.extend(status_line(facts, project, offer, report));
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
        report.next.push(format!(
            "Review and trust {id} hooks in Codex with {}",
            crate::hook_gap::trust_place("codex")
        ));
        return check(
            "approval",
            "problem",
            format!("{missing} of {hooks} hooks have no recorded approval (observed layout)"),
        );
    }
    let stale = found["staleApprovals"].as_u64().unwrap_or(0);
    if stale > 0 {
        report.next.push(format!(
            "Trust {id} hooks again in Codex with {}: an approval covers the plugin content it was given to, so each update needs a new one",
            crate::hook_gap::trust_place("codex")
        ));
        return check(
            "approval",
            "problem",
            format!(
                "{stale} of {hooks} hook approvals were given to other plugin content (the plugin changed since; compared as Codex 0.167 fingerprints a plugin): Codex skips those hooks until they are approved again"
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
                installed_version(found),
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
    // Codex's own session records against the hook telemetry: sessions since the last hook
    // that ran none show it skipping them now, whatever the cause.
    if let Some(gap) = facts.get("gap").filter(|gap| gap.is_object()) {
        let count = gap["sessions"].as_u64().unwrap_or(0);
        checks.push(check(
            "sessions",
            "problem",
            format!(
                "{count} Codex session{} in this project since {} ran no Craft hook{}",
                if count == 1 { "" } else { "s" },
                gap["first"].as_str().unwrap_or("?"),
                if gap["safety"] == true {
                    ": the safety policy checked none of their commands"
                } else {
                    ""
                }
            ),
        ));
        report.next.push(format!(
            "Trust Craft's hooks in Codex with {}, then start a new session",
            crate::hook_gap::trust_place("codex")
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
    let read: Vec<String> = found["layers"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|layer| matches!(layer["state"].as_str(), Some("loaded" | "pinned")))
        .filter_map(|layer| {
            let name = layer["name"].as_str()?;
            Some(match layer["state"].as_str() {
                Some("pinned") => format!("{name} (the project's pinned copy)"),
                _ => name.to_string(),
            })
        })
        .collect();
    if !read.is_empty() {
        checks.push(check(
            "layers",
            "ok",
            format!(
                "settings read from: {} (`craft config` explains each value)",
                read.join(", ")
            ),
        ));
    }
    for layer in found["layers"].as_array().into_iter().flatten() {
        if layer["pin"]["state"] == "differs" {
            checks.push(check(
                "layers",
                "problem",
                format!(
                    "{} differs from {}: CI and teammates without that file get the older organization layer",
                    layer["pin"]["file"].as_str().unwrap_or("?"),
                    layer["file"].as_str().unwrap_or("?"),
                ),
            ));
            report.next.push(
                "Run `craft config --sync --apply` and commit .craft/organization.json".into(),
            );
        }
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
    // With telemetry off nothing is recorded, so nothing is known either way.
    if found["safety"] == true && facts["telemetry"]["local"] != false {
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
    checks.extend(secret_files(facts, hosts, report));
    checks
}

/// Claude's own deny rules for secret files, listed for `doctor` to offer; Craft writes none.
const CLAUDE_SECRET_DENY: &str = r#"{"permissions": {"deny": ["Read(**/.env)", "Read(**/.env.local)", "Read(**/*.key)", "Read(~/.ssh/id_*)", "Read(~/.aws/credentials)"]}}"#;

/// The Codex permission profile entry that keeps `.env` files from commands, for `doctor`
/// to offer.
const CODEX_SECRET_DENY: &str =
    r#""**/*.env" = "deny" under [permissions.<profile>.filesystem.":workspace_roots"]"#;

/// Whether each installed host's own controls also keep `.env` files from the model while
/// Craft's secret-file rule is in force. The rule is a hook guardrail; a host deny rule,
/// permission profile or ignore list is the boundary, and only the consumer sets one.
fn secret_files(facts: &Value, hosts: &[Value], report: &mut Report) -> Vec<Value> {
    if facts["configuration"]["secretFiles"] != true {
        return Vec::new();
    }
    let installed = |name: &str| {
        hosts.iter().any(|host| {
            host["host"] == name
                && !matches!(host["status"].as_str(), Some("not-installed" | "off"))
        })
    };
    let mut checks = Vec::new();
    if installed("claude") {
        checks.push(claude_secret_files(facts, report));
    }
    if installed("codex") {
        checks.push(codex_secret_files(&facts["codex"], report));
    }
    let cursor = facts["overlays"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|overlay| overlay["host"] == "cursor" && overlay["present"] == true);
    if let Some(cursor) = cursor {
        checks.push(cursor_secret_files(cursor, report));
    }
    checks
}

fn claude_secret_files(facts: &Value, report: &mut Report) -> Value {
    let denying: Vec<&str> = facts["claude"]["settings"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|layer| {
            layer["deny"].as_array().into_iter().flatten().any(|rule| {
                rule.as_str()
                    .is_some_and(|rule| rule.starts_with("Read(") && rule.contains(".env"))
            })
        })
        .filter_map(|layer| layer["layer"].as_str())
        .collect();
    if !denying.is_empty() {
        return check(
            "secret-files",
            "ok",
            format!(
                "Claude's own deny rules ({} settings) refuse reading .env files; Craft's rule asks about the other secret files its hook sees",
                denying.join(", ")
            ),
        );
    }
    report.next.push(format!(
        "Optional, for a boundary on secret files in Claude: add to .claude/settings.json (or your user settings) {CLAUDE_SECRET_DENY}; Craft does not write it"
    ));
    check(
        "secret-files",
        "off",
        "only Craft's hook guards secret files in Claude, a guardrail: no Claude deny rule for .env files in user, project or local settings (managed settings not inspected), so a read the hook cannot see, such as a script, is not stopped",
    )
}

/// The profile `default_permissions` selects and the `.env` paths it denies. A profile chosen
/// for one run on the command line or set by managed requirements is not seen.
fn codex_secret_files(codex: &Value, report: &mut Report) -> Value {
    let profile = codex["defaultPermissions"].as_str().unwrap_or_default();
    let prefix = format!("permissions.{profile}.filesystem.");
    let denied: Vec<&str> = codex["denied"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .filter_map(|key| key.strip_prefix(&prefix))
        .filter(|_| !profile.is_empty())
        .map(|path| path.strip_prefix(":workspace_roots.").unwrap_or(path))
        .filter(|path| path.contains(".env"))
        .collect();
    if !denied.is_empty() {
        return check(
            "secret-files-codex",
            "ok",
            format!(
                "Codex's permission profile `{profile}` denies {}; Craft's rule refuses the other secret files its hook sees",
                denied.join(", ")
            ),
        );
    }
    report.next.push(format!(
        "Optional, for a boundary on secret files in Codex (permission profiles, beta): in config.toml set {CODEX_SECRET_DENY} for the profile `default_permissions` selects; Craft does not write it"
    ));
    check(
        "secret-files-codex",
        "off",
        "only Craft's hook guards secret files in Codex, a guardrail: the permission profile config.toml selects by default denies no .env path (a profile chosen for one run and managed requirements are not inspected), so a read the hook cannot see, such as a script, is not stopped",
    )
}

/// Cursor keeps `.env*` files from its Agent by default; a `.cursorignore` line can take
/// them back out.
fn cursor_secret_files(cursor: &Value, report: &mut Report) -> Value {
    let reincluded: Vec<&str> = cursor["reincluded"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .collect();
    if reincluded.is_empty() {
        return check(
            "secret-files-cursor",
            "ok",
            "Cursor ignores .env files for its Agent by default and .cursorignore does not take them back out (user-level ignore settings not inspected); terminal commands and MCP tools are outside Cursor's ignore list, where Craft's rule refuses the reads its hook sees",
        );
    }
    report.next.push(format!(
        "Optional, for secret files in Cursor: remove {} from .cursorignore so Cursor ignores .env files again; Craft does not write it",
        reincluded.join(", ")
    ));
    check(
        "secret-files-cursor",
        "off",
        format!(
            "only Craft's hook guards .env files in Cursor, a guardrail: .cursorignore takes them back out of what Cursor ignores ({})",
            reincluded.join(", ")
        ),
    )
}

/// A host's hook runs that gave no answer in the retained telemetry, as a `problem` check:
/// a hook that stays silent because it could not read its input or failed is counted, not
/// taken for one with nothing to say. `None` when every run answered.
fn unanswered(telemetry: &Value, host: &str) -> Option<Value> {
    let row = telemetry["unanswered"]
        .as_array()?
        .iter()
        .find(|row| row["host"] == host)?;
    let count = |field: &str| row[field].as_u64().unwrap_or(0);
    let (unread, failed) = (count("unread"), count("failed"));
    if unread + failed == 0 {
        return None;
    }
    let mut parts = Vec::new();
    if unread > 0 {
        parts.push(format!(
            "{unread} with input not read (too large, unreadable or not JSON)"
        ));
    }
    if failed > 0 {
        parts.push(format!("{failed} failed"));
    }
    let since = telemetry["window"]["first"]
        .as_str()
        .map_or(String::new(), |first| format!(" since {first}"));
    Some(check(
        "answers",
        "problem",
        format!(
            "{} hook run{} gave no answer{since}: {}; last {} at {}",
            unread + failed,
            if unread + failed == 1 { "" } else { "s" },
            parts.join(", "),
            row["lifecycle"].as_str().unwrap_or("event"),
            row["at"].as_str().unwrap_or("?"),
        ),
    ))
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
        claude(
            &facts["claude"],
            &facts["project"],
            &seen,
            telemetry,
            &mut state,
        ),
        codex(&facts["codex"], &seen, telemetry, &mut state),
    ];
    for found in facts["overlays"].as_array().into_iter().flatten() {
        hosts.push(overlay(found, &seen, telemetry, &mut state));
    }
    for host in &mut hosts {
        let name = host["host"].as_str().unwrap_or_default().to_owned();
        if matches!(host["status"].as_str(), Some("not-installed" | "off")) {
            continue;
        }
        if let (Some(found), Some(checks)) =
            (unanswered(telemetry, &name), host["checks"].as_array_mut())
        {
            checks.push(found);
            host["status"] = json!("problem");
        }
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
