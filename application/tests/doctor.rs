//! `craft doctor` over fake Claude Code and Codex homes: a bundle whose runtime matches its
//! manifest reports ok; an altered or missing runtime in an enabled package exits 1; nothing
//! in the host homes is written.
mod support;
use serde_json::{Value, json};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use support::Repo;

/// A Craft bundle for this platform at `dir`: launcher, manifest and runtime bytes.
fn bundle(dir: &Path, version: &str) {
    let (os, arch) = craft_platform::host_install::platform();
    let target = "test-target";
    let runtime = b"#!/bin/sh\nexit 0\n";
    let executable = format!("{target}/native-bin/craft");
    fs::create_dir_all(dir.join("bin")).unwrap();
    fs::write(dir.join("bin/craft"), "#!/bin/sh\n").unwrap();
    fs::write(dir.join("bin/craft.cmd"), "@echo off\n").unwrap();
    let native = dir.join("support/native-bin");
    fs::create_dir_all(native.join(format!("{target}/native-bin"))).unwrap();
    fs::write(native.join(&executable), runtime).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(native.join(&executable), fs::Permissions::from_mode(0o755)).unwrap();
    }
    let manifest = json!({"formatVersion": 1, "packVersion": version, "artifacts": [
        {"target": target, "platform": os, "arch": arch, "executable": executable,
         "sha256": craft_engine::hash(runtime), "binaryVersion": version}
    ]});
    fs::write(native.join("runtime.json"), manifest.to_string()).unwrap();
    fs::create_dir_all(dir.join("platform-hooks")).unwrap();
    let hooks = json!({"hooks": {"PreToolUse": [{"hooks": [{"type": "command", "command": "x"}]}],
        "SessionStart": [{"hooks": [{"type": "command", "command": "y"}]}]}});
    fs::write(dir.join("platform-hooks/hooks.json"), hooks.to_string()).unwrap();
}

struct Homes {
    claude: PathBuf,
    codex: PathBuf,
}

fn homes(repo: &Repo) -> Homes {
    let base = PathBuf::from(repo.path()).with_extension("homes");
    let _ = fs::remove_dir_all(&base);
    let claude = base.join("claude");
    let codex = base.join("codex");
    let installed = claude.join("plugins/cache/mp/craft-delivery/0.0.5");
    bundle(&installed, "0.0.5");
    fs::write(
        claude.join("plugins/installed_plugins.json"),
        json!({"version": 2, "plugins": {"craft-delivery@mp": [
            {"scope": "local", "projectPath": repo.path(), "installPath": installed, "version": "0.0.5",
             "installedAt": "2026-11-08T00:00:00.000Z"},
            {"scope": "local", "projectPath": "/elsewhere", "installPath": installed, "version": "0.0.5"}
        ]}})
        .to_string(),
    )
    .unwrap();
    fs::write(claude.join("settings.json"), "{}").unwrap();
    bundle(
        &codex.join("plugins/cache/pack/craft-delivery/0.0.5"),
        "0.0.5",
    );
    fs::write(
        codex.join("config.toml"),
        format!(
            "[projects.'{}']\ntrust_level = \"trusted\"\n\n[plugins.\"craft-delivery@pack\"]\nenabled = true\n\n[hooks.state.\"craft-delivery@pack:platform-hooks/hooks.json:pre_tool_use:0:0\"]\ntrusted_hash = \"sha256:aa\"\n",
            repo.path()
        ),
    )
    .unwrap();
    Homes { claude, codex }
}

fn doctor(repo: &Repo, homes: &Homes) -> (i32, Value) {
    let output = Command::new(env!("CARGO_BIN_EXE_craft"))
        .args(["doctor", "--root", repo.path(), "--json"])
        .env("CLAUDE_CONFIG_DIR", &homes.claude)
        .env_remove("CLAUDE_CODE_PLUGIN_CACHE_DIR")
        .env("CODEX_HOME", &homes.codex)
        .output()
        .unwrap();
    let report = serde_json::from_slice(&output.stdout)
        .unwrap_or_else(|_| panic!("{}", String::from_utf8_lossy(&output.stderr)));
    (output.status.code().unwrap(), report)
}

fn host<'a>(report: &'a Value, name: &str) -> &'a Value {
    report["hosts"]
        .as_array()
        .unwrap()
        .iter()
        .find(|h| h["host"] == name)
        .unwrap()
}

/// Every file under `dir` with its bytes, to show nothing was written.
fn tree(dir: &Path) -> Vec<(PathBuf, Vec<u8>)> {
    let mut found = Vec::new();
    for entry in fs::read_dir(dir).unwrap().flatten() {
        let path = entry.path();
        if path.is_dir() {
            found.extend(tree(&path));
        } else {
            found.push((path.clone(), fs::read(&path).unwrap()));
        }
    }
    found.sort();
    found
}

#[test]
fn doctor_reads_host_installs_and_blocks_only_on_a_runtime_that_cannot_run() {
    let repo = Repo::new("doctor", &[("README.md", "x\n")]);
    let homes = homes(&repo);
    fs::create_dir_all(Path::new(repo.path()).join(".claude")).unwrap();
    fs::write(
        Path::new(repo.path()).join(".claude/settings.local.json"),
        json!({"enabledPlugins": {"craft-delivery@mp": true}}).to_string(),
    )
    .unwrap();
    let before = (tree(&homes.claude), tree(&homes.codex));

    let (code, report) = doctor(&repo, &homes);
    assert_eq!(code, 0, "{report}");
    assert_eq!(report["blocking"], false);
    let claude = host(&report, "claude");
    assert_eq!(
        claude["packages"].as_array().unwrap().len(),
        1,
        "only this project's install: {claude}"
    );
    assert_eq!(
        claude["packages"][0]["checks"][1]["status"], "ok",
        "{claude}"
    );
    let codex = host(&report, "codex");
    assert_eq!(codex["checks"][0]["detail"], "project trusted");
    // One of the two declared hooks has a recorded approval.
    assert!(
        codex
            .to_string()
            .contains("1 of 2 hooks have no recorded approval"),
        "{codex}"
    );
    assert_eq!(host(&report, "cursor")["status"], "not-installed");
    assert_eq!(
        (tree(&homes.claude), tree(&homes.codex)),
        before,
        "doctor writes nothing"
    );

    // An altered runtime in the enabled Claude package blocks, and still reports in full.
    let runtime = homes.claude.join(
        "plugins/cache/mp/craft-delivery/0.0.5/support/native-bin/test-target/native-bin/craft",
    );
    fs::write(&runtime, b"tampered").unwrap();
    let (code, report) = doctor(&repo, &homes);
    assert_eq!(code, 1);
    assert_eq!(report["blocking"], true);
    assert!(
        host(&report, "claude")
            .to_string()
            .contains("does not match its manifest digest")
    );

    // The same package disabled: its runtime no longer matters.
    fs::write(
        Path::new(repo.path()).join(".claude/settings.local.json"),
        json!({"enabledPlugins": {"craft-delivery@mp": false}}).to_string(),
    )
    .unwrap();
    assert_eq!(doctor(&repo, &homes).0, 0);
}
