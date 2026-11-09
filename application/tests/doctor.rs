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
