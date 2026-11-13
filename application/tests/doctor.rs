//! `craft doctor` over fake Claude Code and Codex homes: a bundle whose runtime matches its
//! manifest reports ok; an altered or missing runtime in an enabled package exits 1; nothing
//! in the host homes is written.
mod support;
use serde_json::{Value, json};
use std::fs;
use std::path::{Path, PathBuf};
use support::{Repo, doctor, homes};

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

/// Codex skips a hook whose approval was given to other plugin content, without a message:
/// doctor compares each approval with the installed plugin and says when to approve again.
#[test]
fn doctor_says_when_a_codex_update_left_its_hook_approvals_behind() {
    let repo = Repo::new("doctor-approvals", &[("README.md", "x\n")]);
    let homes = homes(&repo);
    let plugin = homes.codex.join("plugins/cache/pack/craft-delivery/0.0.5");
    let content = craft_platform::host_install::codex_content_hash(&plugin).unwrap();
    let mut config = fs::read_to_string(homes.codex.join("config.toml")).unwrap();
    config.push_str(&format!("trusted_content_hash = \"{content}\"\n\n[hooks.state.\"craft-delivery@pack:platform-hooks/hooks.json:session_start:0:0\"]\ntrusted_hash = \"sha256:bb\"\ntrusted_content_hash = \"{content}\"\n"));
    fs::write(homes.codex.join("config.toml"), config).unwrap();
    let approval = |report: &Value| {
        host(report, "codex")["packages"][0]["checks"]
            .as_array()
            .unwrap()
            .iter()
            .find(|c| c["check"] == "approval")
            .unwrap()
            .clone()
    };
    let (_, report) = doctor(&repo, &homes);
    let found = approval(&report);
    assert_eq!(found["status"], "ok", "{found}");
    assert!(
        found["detail"]
            .as_str()
            .unwrap()
            .contains("2 hook approvals recorded, given to the installed plugin content"),
        "{found}"
    );

    // An update changes a file of the plugin; its hook declarations stay the same.
    fs::write(plugin.join("bin/craft"), "#!/bin/sh\n# 0.0.6\n").unwrap();
    let (code, report) = doctor(&repo, &homes);
    assert_eq!(code, 0, "an approval to renew does not block: {report}");
    let found = approval(&report);
    assert_eq!(found["status"], "problem", "{found}");
    assert!(
        found["detail"]
            .as_str()
            .unwrap()
            .contains("2 of 2 hook approvals were given to other plugin content"),
        "{found}"
    );
    assert!(
        report["next"]
            .to_string()
            .contains("Trust craft-delivery@pack hooks again in Codex with `/hooks` in the terminal or Settings → Hooks"),
        "{report}"
    );
}

/// A Codex surface that never asks to trust hooks leaves them skipped without a word: doctor
/// counts the sessions since the last Craft hook that ran none, from Codex's own records.
#[test]
fn doctor_counts_codex_sessions_that_ran_no_craft_hook() {
    let repo = Repo::new("doctor-gap", &[("README.md", "x\n")]);
    let homes = homes(&repo);
    let root = fs::canonicalize(repo.path()).unwrap();
    let sessions = |report: &Value| {
        host(report, "codex")["checks"]
            .as_array()
            .unwrap()
            .iter()
            .find(|c| c["check"] == "sessions")
            .cloned()
    };
    support::codex_session(&homes.codex, "S-old", &root, true);
    support::codex_started(&root, &homes.codex, "S-old");
    assert_eq!(sessions(&doctor(&repo, &homes).1), None);

    std::thread::sleep(std::time::Duration::from_millis(20));
    support::codex_session(&homes.codex, "S-skipped", &root, true);
    let (code, report) = doctor(&repo, &homes);
    assert_eq!(code, 0, "skipped hooks do not block: {report}");
    let found = sessions(&report).expect("a sessions check");
    assert_eq!(found["status"], "problem", "{found}");
    let detail = found["detail"].as_str().unwrap();
    assert!(
        detail.starts_with("1 Codex session in this project since")
            && detail
                .ends_with("ran no Craft hook: the safety policy checked none of their commands"),
        "{detail}"
    );
    assert!(
        report["next"].to_string().contains(
            "Trust Craft's hooks in Codex with `/hooks` in the terminal or Settings → Hooks"
        ),
        "{report}"
    );
}
