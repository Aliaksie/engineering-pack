//! Read-only observations of how hosts installed Craft: their install records, settings and
//! configuration files, and each installed bundle's launcher and native runtime. Nothing is
//! written, started or fetched; values that could hold secrets are never returned, only paths,
//! identifiers, booleans, versions and counts.
use crate::{config, documents, files};
use serde_json::{Map, Value, json};
use std::{
    fs,
    io::ErrorKind,
    path::{Path, PathBuf},
};

/// How large a host record or configuration file is read.
const RECORD_BYTES: usize = 4 * 1024 * 1024;
/// How large a bundled native runtime is hashed.
const BINARY_BYTES: usize = 256 * 1024 * 1024;

fn home() -> Option<PathBuf> {
    std::env::home_dir()
}

fn variable(name: &str) -> Option<PathBuf> {
    std::env::var_os(name)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
}

/// A JSON file: `Ok(None)` when absent, an error text when unreadable or not JSON.
fn json_file(path: &Path) -> Result<Option<Value>, String> {
    match files::read_bounded(path, RECORD_BYTES) {
        Ok(bytes) => serde_json::from_slice(&bytes)
            .map(Some)
            .map_err(|_| format!("{} is not valid JSON", path.display())),
        Err(files::ReadError::Io(error)) if error.kind() == ErrorKind::NotFound => Ok(None),
        Err(_) => Err(format!("{} could not be read", path.display())),
    }
}

/// `{path, present, error?}` for a file that was looked at.
fn looked(path: &Path, present: bool, error: Option<String>) -> Value {
    let mut entry = json!({"path": path.to_string_lossy(), "present": present});
    if let Some(error) = error {
        entry["error"] = json!(error);
    }
    entry
}

/// The runtime artifact name this executable runs as: `darwin`/`linux`/`win32` and
/// `arm64`/`x64`, as bundles name them.
pub fn platform() -> (&'static str, &'static str) {
    let os = match std::env::consts::OS {
        "macos" => "darwin",
        "windows" => "win32",
        other => other,
    };
    let arch = match std::env::consts::ARCH {
        "aarch64" => "arm64",
        "x86_64" => "x64",
        other => other,
    };
    (os, arch)
}

/// Whether `path` holds an installed Craft bundle: it ships a native runtime manifest.
pub fn is_bundle(path: &Path) -> bool {
    path.join("support/native-bin/runtime.json").is_file()
}

/// The launcher and native runtime of a bundle for this platform: `{launcher, packVersion,
/// target, executable, sha256}` with `executable` and `sha256` as `ok|missing|mismatch|…`.
pub fn bundle(path: &Path) -> Value {
    let launcher = if cfg!(windows) {
        "bin/craft.cmd"
    } else {
        "bin/craft"
    };
    let mut found =
        json!({"path": path.to_string_lossy(), "launcher": path.join(launcher).is_file()});
    let runtime = match json_file(&path.join("support/native-bin/runtime.json")) {
        Ok(Some(runtime)) => runtime,
        Ok(None) => {
            found["runtime"] = json!("missing");
            return found;
        }
        Err(error) => {
            found["runtime"] = json!("unreadable");
            found["error"] = json!(error);
            return found;
        }
    };
    found["runtime"] = json!("present");
    found["packVersion"] = runtime["packVersion"].clone();
    let (os, arch) = platform();
    let artifact = runtime["artifacts"].as_array().and_then(|artifacts| {
        artifacts
            .iter()
            .find(|a| a["platform"] == os && a["arch"] == arch)
    });
    let Some(artifact) = artifact else {
        found["executable"] = json!("unsupported-platform");
        return found;
    };
    found["target"] = artifact["target"].clone();
    found["binaryVersion"] = artifact["binaryVersion"].clone();
    let Some(relative) = artifact["executable"]
        .as_str()
        .filter(|r| craft_engine::valid_path(r))
    else {
        found["executable"] = json!("invalid-manifest");
        return found;
    };
    let binary = path.join("support/native-bin").join(relative);
    match files::read_bounded(&binary, BINARY_BYTES) {
        Ok(bytes) => {
            found["executable"] = json!(if runnable(&binary) {
                "ok"
            } else {
                "not-executable"
            });
            found["sha256"] = json!(if Some(craft_engine::hash(&bytes).as_str())
                == artifact["sha256"].as_str()
            {
                "ok"
            } else {
                "mismatch"
            });
        }
        Err(files::ReadError::Io(error)) if error.kind() == ErrorKind::NotFound => {
            found["executable"] = json!("missing");
        }
        Err(_) => found["executable"] = json!("unreadable"),
    }
    found
}

#[cfg(unix)]
fn runnable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    // The launcher sets the executable bit itself when it can; an owner-writable file passes.
    fs::metadata(path)
        .is_ok_and(|m| m.permissions().mode() & 0o100 != 0 || m.permissions().mode() & 0o200 != 0)
}

#[cfg(not(unix))]
fn runnable(_: &Path) -> bool {
    true
}

/// Shell hook commands in a Claude-shaped settings object: `hooks.<Event>[].hooks[].command`.
fn settings_commands(settings: &Value) -> Vec<String> {
    settings["hooks"]
        .as_object()
        .into_iter()
        .flat_map(|events| events.values())
        .filter_map(Value::as_array)
        .flatten()
        .filter_map(|group| group["hooks"].as_array())
        .flatten()
        .filter_map(|hook| hook["command"].as_str().map(str::to_owned))
        .collect()
}

/// Claude Code: the configuration directory, Craft bundles in the install record that apply
/// to `root` (user scope, or project/local scope for this project), and the settings layers
/// in precedence order (user, project, local) with their `enabledPlugins` and hook commands.
pub fn claude(root: &Path) -> Value {
    let Some(config) = variable("CLAUDE_CONFIG_DIR").or_else(|| home().map(|h| h.join(".claude")))
    else {
        return json!({"host": "claude", "installed": false, "reason": "no home directory"});
    };
    let plugins =
        variable("CLAUDE_CODE_PLUGIN_CACHE_DIR").unwrap_or_else(|| config.join("plugins"));
    let record = plugins.join("installed_plugins.json");
    let mut sources = Vec::new();
    let mut packages = Vec::new();
    match json_file(&record) {
        Ok(Some(installed)) => {
            sources.push(looked(&record, true, None));
            let root_text = root.to_string_lossy();
            for (id, entries) in installed["plugins"].as_object().into_iter().flatten() {
                for entry in entries.as_array().into_iter().flatten() {
                    let scope = entry["scope"].as_str().unwrap_or_default();
                    let applies = scope == "user"
                        || entry["projectPath"]
                            .as_str()
                            .is_some_and(|p| p == root_text);
                    let Some(path) = entry["installPath"].as_str().map(PathBuf::from) else {
                        continue;
                    };
                    if !applies || !is_bundle(&path) {
                        continue;
                    }
                    packages.push(json!({
                        "id": id, "scope": scope, "version": entry["version"],
                        "installedAt": entry["installedAt"].as_str().or(entry["lastUpdated"].as_str()),
                        "bundle": bundle(&path),
                    }));
                }
            }
        }
        Ok(None) => sources.push(looked(&record, false, None)),
        Err(error) => sources.push(looked(&record, true, Some(error))),
    }
    let mut layers = Vec::new();
    for (layer, path) in [
        ("user", config.join("settings.json")),
        ("project", root.join(".claude/settings.json")),
        ("local", root.join(".claude/settings.local.json")),
    ] {
        match documents::input(&path).map_err(|e| e.to_string()) {
            Ok(settings) => {
                sources.push(looked(&path, true, None));
                layers.push(json!({
                    "layer": layer, "path": path.to_string_lossy(),
                    "enabledPlugins": settings.get("enabledPlugins").cloned().unwrap_or(json!({})),
                    "commands": settings_commands(&settings),
                }));
            }
            Err(_) if !path.exists() => sources.push(looked(&path, false, None)),
            Err(error) => sources.push(looked(&path, true, Some(error))),
        }
    }
    json!({
        "host": "claude", "installed": config.is_dir(), "config": config.to_string_lossy(),
        "packages": packages, "settings": layers, "sources": sources,
    })
}

/// `PreToolUse` → `pre_tool_use`, as Codex keys hook approvals.
fn snake(event: &str) -> String {
    let mut out = String::new();
    for (index, ch) in event.char_indices() {
        if ch.is_ascii_uppercase() {
            if index > 0 {
                out.push('_');
            }
            out.push(ch.to_ascii_lowercase());
        } else {
            out.push(ch);
        }
    }
    out
}

/// The approval keys a Codex plugin's hooks file declares: `<event>:<group>:<hook>`.
fn declared_hooks(hooks: &Value) -> Vec<String> {
    let mut keys = Vec::new();
    for (event, groups) in hooks["hooks"].as_object().into_iter().flatten() {
        for (group, entry) in groups.as_array().into_iter().flatten().enumerate() {
            for index in 0..entry["hooks"].as_array().map_or(0, Vec::len) {
                keys.push(format!("{}:{group}:{index}", snake(event)));
            }
        }
    }
    keys
}

/// The dotted keys of a Codex `config.toml` with their values as text: `Ok(None)` when the
/// file is absent, an error text when it could not be read in full.
fn codex_keys(config_file: &Path) -> Result<Option<Map<String, Value>>, String> {
    let unread = || "config.toml could not be read in full".to_owned();
    match files::read_bounded(config_file, config::CONFIGURATION_BYTES) {
        Ok(bytes) => match config::extract("config.toml", &bytes) {
            Ok(extraction) if !extraction.partial => Ok(Some(
                extraction
                    .evidence
                    .into_iter()
                    .map(|evidence| (evidence.name, json!(evidence.target)))
                    .collect(),
            )),
            _ => Err(unread()),
        },
        Err(files::ReadError::Io(error)) if error.kind() == ErrorKind::NotFound => Ok(None),
        Err(_) => Err(unread()),
    }
}

/// How much of one plugin, in bytes and entries, a content fingerprint reads.
const PLUGIN_BYTES: u64 = 1024 * 1024 * 1024;
const PLUGIN_ENTRIES: usize = 20_000;

/// The fingerprint Codex binds a plugin's hook approvals to (`trusted_content_hash`), as
/// Codex 0.167 computes it (`hooks/src/engine/plugin_fingerprint.rs`): SHA-256 over a tag,
/// then every entry beneath the plugin root in sorted pre-order as its relative path
/// (length-prefixed), its kind and, for a file, its length and bytes. Python caches and
/// virtual environments are left out; any other kind of entry (a link) has no fingerprint.
pub fn codex_content_hash(root: &Path) -> Result<String, String> {
    use sha2::{Digest, Sha256};
    use std::io::Read;
    let skipped = |name: &std::ffi::OsStr| {
        let name = name.to_string_lossy();
        name == "__pycache__"
            || name == ".venv"
            || name.ends_with(".pyc")
            || name.ends_with(".egg-info")
    };
    let mut hash = Sha256::new();
    hash.update(b"codex-plugin-bundle-v1");
    let (mut stack, mut entries, mut bytes) = (vec![root.to_path_buf()], 0usize, 0u64);
    while let Some(path) = stack.pop() {
        entries += 1;
        if entries > PLUGIN_ENTRIES {
            return Err("plugin holds too many files to fingerprint".into());
        }
        let name = path
            .strip_prefix(root)
            .map_err(|_| "entry outside the plugin".to_owned())?
            .as_os_str()
            .as_encoded_bytes();
        hash.update((name.len() as u64).to_le_bytes());
        hash.update(name);
        let meta = fs::symlink_metadata(&path).map_err(|error| error.to_string())?;
        if meta.is_dir() {
            hash.update(b"directory");
            let mut children: Vec<PathBuf> = fs::read_dir(&path)
                .map_err(|error| error.to_string())?
                .filter_map(Result::ok)
                .filter(|entry| !skipped(&entry.file_name()))
                .map(|entry| entry.path())
                .collect();
            children.sort_by(|a, b| b.file_name().cmp(&a.file_name()));
            stack.extend(children);
        } else if meta.is_file() {
            bytes += meta.len();
            if bytes > PLUGIN_BYTES {
                return Err("plugin is too large to fingerprint".into());
            }
            hash.update(b"file");
            hash.update(meta.len().to_le_bytes());
            let mut file = fs::File::open(&path)
                .map_err(|error| error.to_string())?
                .take(meta.len());
            let (mut copied, mut buffer) = (0u64, vec![0u8; 64 * 1024]);
            loop {
                let read = file.read(&mut buffer).map_err(|error| error.to_string())?;
                if read == 0 {
                    break;
                }
                hash.update(&buffer[..read]);
                copied += read as u64;
            }
            if copied != meta.len() {
                return Err("plugin file changed while fingerprinting".into());
            }
        } else {
            return Err(format!(
                "{} is neither a file nor a folder",
                path.strip_prefix(root).unwrap_or(&path).display()
            ));
        }
    }
    let digest: String = hash.finalize().iter().map(|b| format!("{b:02x}")).collect();
    Ok(format!("sha256:{digest}"))
}

/// Codex: configured Craft plugins and whether each is enabled, the project's trust level,
/// the hooks feature switch, recorded hook approvals against the hooks each installed bundle
/// declares and against its current content, and other hook files holding commands.
pub fn codex(root: &Path) -> Value {
    let Some(home) = variable("CODEX_HOME").or_else(|| home().map(|h| h.join(".codex"))) else {
        return json!({"host": "codex", "installed": false, "reason": "no home directory"});
    };
    let config_file = home.join("config.toml");
    let mut sources = Vec::new();
    let keys = match codex_keys(&config_file) {
        Ok(Some(keys)) => {
            sources.push(looked(&config_file, true, None));
            keys
        }
        Ok(None) => {
            sources.push(looked(&config_file, false, None));
            Map::new()
        }
        Err(error) => {
            sources.push(looked(&config_file, true, Some(error)));
            Map::new()
        }
    };
    let root_text = root.to_string_lossy();
    let trust = keys
        .get(&format!("projects.{root_text}.trust_level"))
        .cloned()
        .unwrap_or(Value::Null);
    let hooks_feature = keys
        .get("features.hooks")
        .or_else(|| keys.get("features.codex_hooks"))
        .cloned()
        .unwrap_or(Value::Null);
    let approved: Vec<&str> = keys
        .keys()
        .filter_map(|key| {
            key.strip_prefix("hooks.state.")?
                .strip_suffix(".trusted_hash")
        })
        .collect();
    let mut packages = Vec::new();
    let cache = home.join("plugins/cache");
    for marketplace in fs::read_dir(&cache).into_iter().flatten().flatten() {
        for package in fs::read_dir(marketplace.path())
            .into_iter()
            .flatten()
            .flatten()
        {
            for version in fs::read_dir(package.path()).into_iter().flatten().flatten() {
                let path = version.path();
                if !is_bundle(&path) {
                    continue;
                }
                let id = format!(
                    "{}@{}",
                    package.file_name().to_string_lossy(),
                    marketplace.file_name().to_string_lossy()
                );
                let declared = json_file(&path.join("platform-hooks/hooks.json"))
                    .ok()
                    .flatten()
                    .map(|hooks| declared_hooks(&hooks))
                    .unwrap_or_default();
                let prefix = format!("{id}:platform-hooks/hooks.json:");
                let unapproved = declared
                    .iter()
                    .filter(|key| !approved.contains(&format!("{prefix}{key}").as_str()))
                    .count();
                // The plugin content each approval was given to, where Codex recorded one.
                let recorded: Vec<&Value> = declared
                    .iter()
                    .filter_map(|key| {
                        keys.get(&format!("hooks.state.{prefix}{key}.trusted_content_hash"))
                    })
                    .collect();
                let (stale, content) = if recorded.is_empty() {
                    (0, json!("not-recorded"))
                } else {
                    match codex_content_hash(&path) {
                        Ok(current) => {
                            let stale = recorded.iter().filter(|r| ***r != json!(current)).count();
                            (stale, json!(if stale == 0 { "match" } else { "changed" }))
                        }
                        Err(error) => (0, json!({"error": error})),
                    }
                };
                packages.push(json!({
                    "id": id, "version": version.file_name().to_string_lossy(),
                    "enabled": keys.get(&format!("plugins.{id}.enabled")).cloned().unwrap_or(Value::Null),
                    "hooks": declared.len(), "unapproved": unapproved,
                    "staleApprovals": stale, "approvedContent": content,
                    "bundle": bundle(&path),
                }));
            }
        }
    }
    // Craft plugins the configuration names, installed or not.
    let configured: Vec<Value> = keys
        .iter()
        .filter_map(|(key, enabled)| {
            let id = key.strip_prefix("plugins.")?.strip_suffix(".enabled")?;
            id.starts_with("craft-")
                .then(|| json!({"id": id, "enabled": enabled}))
        })
        .collect();
    let mut commands = Vec::new();
    for path in [home.join("hooks.json"), root.join(".codex/hooks.json")] {
        match json_file(&path) {
            Ok(Some(hooks)) => {
                sources.push(looked(&path, true, None));
                commands.extend(
                    settings_commands(&hooks)
                        .into_iter()
                        .map(|c| json!({"path": path.to_string_lossy(), "command": c})),
                );
            }
            Ok(None) => sources.push(looked(&path, false, None)),
            Err(error) => sources.push(looked(&path, true, Some(error))),
        }
    }
    json!({
        "host": "codex", "installed": home.is_dir(), "config": home.to_string_lossy(),
        "trust": trust, "hooksFeature": hooks_feature, "packages": packages,
        "configured": configured, "commands": commands, "sources": sources,
    })
}

/// Whether a host setting turns the plugin `id` off for `project`. Claude Code: the last of
/// the user, project and local settings naming it says `false`. Codex: its configuration
/// was read and does not enable it. Other hosts load every package an overlay ships.
fn turned_off(host: &str, project: &Path, id: &str) -> bool {
    match host {
        "claude" => {
            let Some(config) =
                variable("CLAUDE_CONFIG_DIR").or_else(|| home().map(|h| h.join(".claude")))
            else {
                return false;
            };
            [
                config.join("settings.json"),
                project.join(".claude/settings.json"),
                project.join(".claude/settings.local.json"),
            ]
            .iter()
            .rev()
            .find_map(|path| {
                documents::input(path)
                    .ok()?
                    .get("enabledPlugins")?
                    .get(id)
                    .cloned()
            })
            .is_some_and(|enabled| enabled != true)
        }
        "codex" => variable("CODEX_HOME")
            .or_else(|| home().map(|h| h.join(".codex")))
            .and_then(|home| codex_keys(&home.join("config.toml")).ok().flatten())
            .is_some_and(|keys| keys.get(&format!("plugins.{id}.enabled")) != Some(&json!("true"))),
        _ => false,
    }
}

/// The Craft packages `host` has installed beside the running `bundle` of `package`, by
/// name, or `None` when the bundle sits in no layout a host installs. A plugin cache keeps
/// `<marketplace>/<package>/<version>`; an overlay or a marketplace checkout keeps `<package>`
/// folders side by side. The running package always counts: its hook ran.
pub fn packages(host: &str, project: &Path, bundle: &Path, package: &str) -> Option<Vec<String>> {
    if !is_bundle(bundle) {
        return None;
    }
    let name = |path: &Path| path.file_name().map(|n| n.to_string_lossy().into_owned());
    let folders = |path: &Path| -> Vec<PathBuf> {
        fs::read_dir(path)
            .into_iter()
            .flatten()
            .flatten()
            .map(|entry| entry.path())
            .filter(|path| path.is_dir())
            .collect()
    };
    let parent = bundle.parent()?;
    let mut found = vec![package.to_owned()];
    if name(bundle).as_deref() == Some(package) {
        found.extend(
            folders(parent)
                .iter()
                .filter(|sibling| is_bundle(sibling))
                .filter_map(|sibling| name(sibling)),
        );
    } else if name(parent).as_deref() == Some(package) {
        let market = parent.parent()?;
        let marketplace = name(market)?;
        for sibling in folders(market) {
            let Some(sibling_name) = name(&sibling) else {
                continue;
            };
            if folders(&sibling).iter().any(|version| is_bundle(version))
                && !turned_off(host, project, &format!("{sibling_name}@{marketplace}"))
            {
                found.push(sibling_name);
            }
        }
    } else {
        return None;
    }
    found.sort();
    found.dedup();
    Some(found)
}

/// A project overlay a host loads from the repository, with the bundle each of its commands
/// starts from (commands run from the project root): `{host, path, present, commands, bundles}`.
pub fn overlay(host: &str, root: &Path, relative: &str) -> Value {
    let path = &root.join(relative);
    match json_file(path) {
        Ok(Some(found)) => {
            // Cursor and Copilot overlays list commands per event without a `hooks` group.
            let mut commands = settings_commands(&found);
            for events in found["hooks"].as_object().into_iter() {
                for entry in events.values().filter_map(Value::as_array).flatten() {
                    for key in ["command", "bash"] {
                        if let Some(command) = entry[key].as_str() {
                            commands.push(command.to_owned());
                        }
                    }
                }
            }
            // Each command starts from its package root: `r="./packages/<name>"`.
            let mut roots: Vec<String> = commands
                .iter()
                .filter_map(|c| {
                    c.strip_prefix("r=\"")?
                        .split_once('"')
                        .map(|(r, _)| r.to_owned())
                })
                .filter(|r| !r.contains('$'))
                .collect();
            roots.sort();
            roots.dedup();
            let bundles: Vec<Value> = roots.iter().map(|r| bundle(&root.join(r))).collect();
            json!({"host": host, "path": path.to_string_lossy(), "present": true, "commands": commands, "bundles": bundles})
        }
        Ok(None) => {
            json!({"host": host, "path": path.to_string_lossy(), "present": false, "commands": []})
        }
        Err(error) => {
            json!({"host": host, "path": path.to_string_lossy(), "present": true, "error": error, "commands": []})
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codex_approval_keys_follow_event_group_and_hook_order() {
        let hooks = json!({"hooks": {
            "PreToolUse": [{"hooks": [{"command": "a"}]}],
            "SessionStart": [{"hooks": [{"command": "b"}]}, {"hooks": [{"command": "c"}, {"command": "d"}]}]
        }});
        assert_eq!(
            declared_hooks(&hooks),
            [
                "pre_tool_use:0:0",
                "session_start:0:0",
                "session_start:1:0",
                "session_start:1:1"
            ]
        );
    }

    /// The value Codex 0.167 records as `trusted_content_hash` for this tree (reference
    /// computed from its published algorithm, which reproduced recorded approvals of real
    /// plugins). Windows names nested entries with its own separator, so the value differs.
    #[cfg(unix)]
    #[test]
    fn a_plugin_fingerprint_is_the_one_codex_records_with_an_approval() {
        let root = std::env::temp_dir().join(format!("craft-fingerprint-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        for dir in ["sub", "__pycache__", ".hidden"] {
            fs::create_dir_all(root.join(dir)).unwrap();
        }
        for (file, text) in [
            ("a.txt", "a\n"),
            ("sub/b.txt", "b"),
            ("__pycache__/x.pyc", "x"),
            (".hidden/h", "h"),
            ("old.pyc", "c"),
        ] {
            fs::write(root.join(file), text).unwrap();
        }
        let hash = codex_content_hash(&root).unwrap();
        assert_eq!(
            hash,
            "sha256:b014e4cd12eab7da9958c554a4fa4598ef7cf02c688144df52c8f3cae5583766"
        );
        // Any file of the plugin is part of it; a cache is not; a link has no fingerprint.
        fs::write(root.join("__pycache__/y.pyc"), "y").unwrap();
        assert_eq!(codex_content_hash(&root).unwrap(), hash);
        fs::write(root.join(".hidden/h"), "changed").unwrap();
        assert_ne!(codex_content_hash(&root).unwrap(), hash);
        std::os::unix::fs::symlink(root.join("a.txt"), root.join("link")).unwrap();
        let error = codex_content_hash(&root).unwrap_err();
        assert_eq!(error, "link is neither a file nor a folder");
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn hook_commands_are_read_from_claude_shaped_settings() {
        let settings = json!({"hooks": {"PostToolUse": [{"matcher": "Bash", "hooks": [{"type": "command", "command": "x hook claude after-tool"}]}]}});
        assert_eq!(settings_commands(&settings), ["x hook claude after-tool"]);
        assert!(settings_commands(&json!({})).is_empty());
    }
}
