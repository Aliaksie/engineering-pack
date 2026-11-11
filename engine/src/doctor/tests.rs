use super::*;

fn bundle() -> Value {
    json!({"path": "/p", "launcher": true, "runtime": "present", "packVersion": "0.0.5",
        "target": "aarch64-apple-darwin", "executable": "ok", "sha256": "ok"})
}

fn facts() -> Value {
    json!({
        "project": "/repo", "craftVersion": "0.0.5",
        "telemetry": {"local": true, "window": {"first": "2026-11-01T00:00:00Z"}, "lastSeen": [
            {"host": "claude", "package": "craft-delivery", "lifecycle": "before-tool", "packVersion": "0.0.5", "at": "2026-11-09T10:00:00Z"},
            {"host": "codex", "package": "craft-delivery", "lifecycle": "session-start", "packVersion": "0.0.5", "at": "2026-11-09T09:00:00Z"}
        ]},
        "claude": {"installed": true, "packages": [
            {"id": "craft-delivery@craft-capabilities", "scope": "local", "version": "0.0.5", "installedAt": "2026-11-08T00:00:00Z", "bundle": bundle()}
        ], "settings": [
            {"layer": "user", "enabledPlugins": {}, "commands": []},
            {"layer": "local", "enabledPlugins": {"craft-delivery@craft-capabilities": true}, "commands": []}
        ]},
        "codex": {"installed": true, "trust": "trusted", "hooksFeature": null, "commands": [], "packages": [
            {"id": "craft-delivery@engineering-pack", "version": "0.0.5", "enabled": "true", "hooks": 8, "unapproved": 0, "bundle": bundle()}
        ]},
        "overlays": [{"host": "cursor", "present": false, "commands": [], "bundles": []}],
    })
}

fn host<'a>(report: &'a Value, name: &str) -> &'a Value {
    report["hosts"]
        .as_array()
        .unwrap()
        .iter()
        .find(|h| h["host"] == name)
        .unwrap()
}

#[test]
fn a_working_install_reports_ok_and_does_not_block() {
    let report = report(&facts());
    assert_eq!(report["readOnly"], true);
    assert_eq!(report["blocking"], false);
    assert_eq!(host(&report, "claude")["status"], "ok");
    assert_eq!(host(&report, "codex")["status"], "ok");
    assert_eq!(host(&report, "cursor")["status"], "not-installed");
    assert!(report["next"].as_array().unwrap().is_empty());
}

#[test]
fn a_missing_or_altered_runtime_in_an_enabled_package_blocks() {
    for (field, value) in [
        ("executable", json!("missing")),
        ("sha256", json!("mismatch")),
        ("launcher", json!(false)),
        ("executable", json!("unsupported-platform")),
    ] {
        let mut facts = facts();
        facts["claude"]["packages"][0]["bundle"][field] = value.clone();
        let report = report(&facts);
        assert_eq!(report["blocking"], true, "{field} {value}");
        assert_eq!(host(&report, "claude")["status"], "problem");
    }
    // A disabled package's runtime does not matter.
    let mut facts = facts();
    facts["claude"]["packages"][0]["bundle"]["executable"] = json!("missing");
    facts["claude"]["settings"][1]["enabledPlugins"]["craft-delivery@craft-capabilities"] =
        json!(false);
    let report = report(&facts);
    assert_eq!(report["blocking"], false);
    assert_eq!(host(&report, "claude")["packages"][0]["status"], "off");
}

#[test]
fn a_session_older_than_the_install_is_a_problem_with_a_next_step_not_a_block() {
    let mut facts = facts();
    facts["telemetry"]["lastSeen"][0]["packVersion"] = json!("0.0.4");
    facts["telemetry"]["lastSeen"][0]["at"] = json!("2026-11-07T00:00:00Z");
    let report = report(&facts);
    assert_eq!(report["blocking"], false);
    assert_eq!(host(&report, "claude")["status"], "problem");
    assert!(
        report["next"][0]
            .as_str()
            .unwrap()
            .contains("/reload-plugins")
    );
}

#[test]
fn no_telemetry_is_unknown_never_a_failure() {
    let mut facts = facts();
    facts["telemetry"]["lastSeen"] = json!([]);
    let report = report(&facts);
    assert_eq!(host(&report, "claude")["status"], "unknown");
    facts["telemetry"]["local"] = json!(false);
    let report = super::report(&facts);
    let hooks = &host(&report, "codex")["packages"][0]["checks"];
    assert!(hooks.to_string().contains("telemetry is off"));
}

#[test]
fn codex_trust_approvals_and_duplicates_are_reported() {
    let mut facts = facts();
    facts["codex"]["trust"] = json!("untrusted");
    facts["codex"]["packages"][0]["unapproved"] = json!(3);
    facts["codex"]["commands"] = json!([{"path": "/repo/.codex/hooks.json", "command": "/x/bin/craft hook codex before-tool --package craft-delivery"}]);
    let report = report(&facts);
    let codex = host(&report, "codex").to_string();
    assert!(codex.contains("project untrusted"), "{codex}");
    assert!(
        codex.contains("3 of 8 hooks have no recorded approval"),
        "{codex}"
    );
    assert!(codex.contains("Codex runs every matching hook"), "{codex}");
    assert_eq!(report["blocking"], false);
}

#[test]
fn claude_settings_hooks_and_two_marketplaces_are_duplicates() {
    let mut facts = facts();
    facts["claude"]["settings"][0]["commands"] =
        json!(["/x/bin/craft hook claude before-tool --package craft-delivery"]);
    let mut second = facts["claude"]["packages"][0].clone();
    second["id"] = json!("craft-delivery@other");
    facts["claude"]["packages"]
        .as_array_mut()
        .unwrap()
        .push(second);
    facts["claude"]["settings"][1]["enabledPlugins"]["craft-delivery@other"] = json!(true);
    let claude = host(&report(&facts), "claude").to_string();
    assert!(
        claude.contains("also registered in user settings"),
        "{claude}"
    );
    assert!(claude.contains("enabled from two marketplaces"), "{claude}");
}

#[test]
fn an_overlay_whose_package_has_no_runtime_blocks() {
    let mut facts = facts();
    let mut missing = bundle();
    missing["executable"] = json!("missing");
    facts["overlays"] = json!([{"host": "cursor", "present": true, "path": "/repo/.cursor/hooks.json", "commands": ["r=\"./packages/craft-delivery\"; …"], "bundles": [missing]}]);
    let report = report(&facts);
    assert_eq!(report["blocking"], true);
    assert_eq!(host(&report, "cursor")["status"], "problem");
}

#[test]
fn one_claude_package_at_two_scopes_is_not_two_marketplaces() {
    let mut facts = facts();
    let mut user = facts["claude"]["packages"][0].clone();
    user["scope"] = json!("user");
    facts["claude"]["packages"]
        .as_array_mut()
        .unwrap()
        .push(user);
    facts["claude"]["settings"][0]["enabledPlugins"]["craft-delivery@craft-capabilities"] =
        json!(true);
    let claude = host(&report(&facts), "claude").to_string();
    assert!(!claude.contains("two marketplaces"), "{claude}");
}

#[test]
fn codex_judges_the_newest_cached_version_and_names_older_ones() {
    let mut facts = facts();
    let mut old = facts["codex"]["packages"][0].clone();
    old["version"] = json!("0.0.10");
    old["bundle"]["executable"] = json!("unsupported-platform");
    let mut older = old.clone();
    older["version"] = json!("0.0.9");
    let packages = facts["codex"]["packages"].as_array_mut().unwrap();
    packages.insert(0, older);
    packages.push(old);
    // 0.0.10 is newest and cannot run: it blocks.
    let report = report(&facts);
    assert_eq!(report["blocking"], true);
    let codex = host(&report, "codex");
    assert_eq!(codex["packages"].as_array().unwrap().len(), 1);
    assert_eq!(codex["packages"][0]["version"], "0.0.10");
    // An older version that cannot run does not block the newest that can.
    let mut facts = super::tests::facts();
    let mut stale = facts["codex"]["packages"][0].clone();
    stale["version"] = json!("0.0.4");
    stale["bundle"]["executable"] = json!("unsupported-platform");
    facts["codex"]["packages"]
        .as_array_mut()
        .unwrap()
        .push(stale);
    let report = super::report(&facts);
    assert_eq!(report["blocking"], false);
    let codex = host(&report, "codex");
    assert_eq!(codex["packages"][0]["version"], "0.0.5");
    assert_eq!(
        codex["packages"][0]["olderCachedVersions"],
        json!(["0.0.4"])
    );
}

#[test]
fn codex_plugins_enabled_but_not_in_the_cache_are_a_problem_with_the_install_command() {
    let mut facts = facts();
    facts["codex"]["packages"] = json!([]);
    facts["codex"]["configured"] = json!([
        {"id": "craft-delivery@engineering-pack", "enabled": "true"},
        {"id": "craft-review@engineering-pack", "enabled": "false"}
    ]);
    let report = report(&facts);
    let codex = host(&report, "codex");
    assert_eq!(codex["status"], "problem");
    let installed = &codex["checks"][0];
    assert_eq!(installed["check"], "installed");
    assert!(installed["detail"].as_str().unwrap().starts_with(
        "craft-delivery@engineering-pack enabled in config.toml but not in the plugin cache"
    ));
    assert!(report["next"].as_array().unwrap().iter().any(|next| {
        next.as_str()
            .unwrap()
            .contains("`codex plugin add craft-delivery@engineering-pack`")
    }));
    // Nothing fails in the host: Craft is absent, not broken.
    assert_eq!(report["blocking"], false);
    // One enabled plugin missing beside a cached one is named the same way.
    let mut partly = self::facts();
    partly["codex"]["configured"] = json!([
        {"id": "craft-delivery@engineering-pack", "enabled": "true"},
        {"id": "craft-stories@engineering-pack", "enabled": "true"}
    ]);
    let report = self::report(&partly);
    let detail = host(&report, "codex")["checks"][0]["detail"]
        .as_str()
        .unwrap();
    assert!(
        detail.starts_with("craft-stories@engineering-pack enabled"),
        "{detail}"
    );
    // Nothing configured and nothing cached stays not installed.
    let mut none = self::facts();
    none["codex"]["packages"] = json!([]);
    assert_eq!(
        host(&self::report(&none), "codex")["status"],
        "not-installed"
    );
}

#[test]
fn a_host_without_an_install_still_names_the_hook_events_recorded_for_it() {
    let mut facts = facts();
    facts["claude"]["packages"] = json!([]);
    let report = report(&facts);
    let claude = host(&report, "claude");
    assert_eq!(claude["status"], "not-installed");
    let detail = claude["checks"][0]["detail"].as_str().unwrap();
    assert!(detail.starts_with("last hook before-tool at 2026-11-09T10:00:00Z ran craft-delivery 0.0.5, but no install was found"), "{detail}");
    // A host that never ran a hook here has nothing to add.
    assert!(
        host(&report, "cursor")["checks"]
            .as_array()
            .unwrap()
            .is_empty()
    );
}

/// Codex ties a hook approval to the plugin's files: after an update it skips every hook
/// without a message, so the report has to say it.
#[test]
fn codex_approvals_given_to_other_plugin_content_are_a_problem() {
    let mut facts = facts();
    facts["codex"]["packages"][0]["staleApprovals"] = json!(8);
    facts["codex"]["packages"][0]["approvedContent"] = json!("changed");
    let report = report(&facts);
    let codex = host(&report, "codex");
    assert_eq!(codex["status"], "problem");
    let text = codex.to_string();
    assert!(
        text.contains("8 of 8 hook approvals were given to other plugin content"),
        "{text}"
    );
    assert_eq!(report["blocking"], false);
    let next = report["next"].to_string();
    assert!(
        next.contains("Approve craft-delivery@engineering-pack hooks again in Codex `/hooks`"),
        "{next}"
    );

    let approval = |content: Value| {
        let mut facts = self::facts();
        facts["codex"]["packages"][0]["approvedContent"] = content;
        let report = self::report(&facts);
        let checks = host(&report, "codex")["packages"][0]["checks"].clone();
        let found = checks
            .as_array()
            .unwrap()
            .iter()
            .find(|c| c["check"] == "approval")
            .unwrap()
            .clone();
        (
            found["status"].as_str().unwrap().to_owned(),
            found["detail"].as_str().unwrap().to_owned(),
        )
    };
    let (status, detail) = approval(json!("match"));
    assert_eq!(status, "ok");
    assert!(detail.contains("given to the installed plugin content"));
    let (status, detail) = approval(json!({"error": "bin/link is neither a file nor a folder"}));
    assert_eq!(status, "unknown");
    assert!(detail.contains("neither a file nor a folder"), "{detail}");
    assert_eq!(approval(json!("not-recorded")).0, "ok");
}

#[test]
fn a_hook_event_of_another_version_does_not_show_the_installed_one_runs() {
    let mut facts = facts();
    facts["codex"]["packages"][0]["version"] = json!("0.0.6");
    let report = report(&facts);
    let codex = host(&report, "codex");
    let hooks = codex["packages"][0]["checks"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["check"] == "hooks")
        .unwrap();
    assert_eq!(hooks["status"], "unknown");
    assert!(
        hooks["detail"]
            .as_str()
            .unwrap()
            .contains("ran 0.0.5, not the installed 0.0.6"),
        "{hooks}"
    );
}

#[test]
fn configuration_names_ignored_fields_an_unusable_file_and_an_unapplied_safety_policy() {
    let mut facts = facts();
    facts["configuration"] = json!({"safety": true, "layers": [
        {"name": "bundled", "values": 30},
        {"name": "user", "file": "/home/u/.config/craft/preferences.json", "state": "loaded",
         "values": 1, "ignored": ["routing.futureSwitch", "futureSection"]}]});
    let report = self::report(&facts);
    let found = report["configuration"].as_array().unwrap();
    assert_eq!(found.len(), 1, "hooks ran on both hosts: {found:?}");
    assert_eq!(found[0]["status"], "unknown");
    let detail = found[0]["detail"].as_str().unwrap();
    assert!(
        detail.starts_with("user layer /home/u/.config/craft/preferences.json: routing.futureSwitch, futureSection not known to craft"),
        "{detail}"
    );

    // No hook of an installed host has run here: the policy has refused nothing for it.
    facts["telemetry"]["lastSeen"] = json!([]);
    let report = self::report(&facts);
    let safety: Vec<&str> = report["configuration"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|c| c["check"] == "safety")
        .filter_map(|c| c["detail"].as_str())
        .collect();
    assert_eq!(safety.len(), 2, "{safety:?}");
    assert!(safety[1].contains("codex commands are not checked against it"));
    facts["configuration"]["safety"] = json!(false);
    assert!(
        !self::report(&facts)["configuration"]
            .to_string()
            .contains("safety")
    );

    facts["configuration"] =
        json!({"error": "user layer /p.json: routing.debtMarkers must be boolean"});
    let report = self::report(&facts);
    assert_eq!(report["configuration"][0]["status"], "problem");
    assert!(
        report["next"]
            .to_string()
            .contains("Fix the configuration file")
    );
    assert_eq!(report["blocking"], false);
}
