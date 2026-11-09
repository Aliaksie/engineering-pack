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
