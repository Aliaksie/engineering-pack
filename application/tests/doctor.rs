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
    // The secret-file rule is on by default; Claude's own deny rule is offered, not written.
    let secret = |report: &Value| {
        report["configuration"]
            .as_array()
            .unwrap()
            .iter()
            .find(|c| c["check"] == "secret-files")
            .map(|c| c["status"].clone())
    };
    assert_eq!(secret(&report), Some(json!("off")), "{report}");
    fs::write(
        Path::new(repo.path()).join(".claude/settings.json"),
        json!({"permissions": {"deny": ["Read(**/.env)"]}}).to_string(),
    )
    .unwrap();
    let (_, report) = doctor(&repo, &homes);
    assert_eq!(secret(&report), Some(json!("ok")), "{report}");
    fs::remove_file(Path::new(repo.path()).join(".claude/settings.json")).unwrap();
    // Codex's selected permission profile and Cursor's ignore list are read the same way.
    let named = |report: &Value, name: &str| {
        report["configuration"]
            .as_array()
            .unwrap()
            .iter()
            .find(|c| c["check"] == name)
            .map(|c| c["status"].clone())
    };
    assert_eq!(named(&report, "secret-files-codex"), Some(json!("off")));
    assert_eq!(named(&report, "secret-files-cursor"), None, "no overlay");
    let config = homes.codex.join("config.toml");
    let before = fs::read_to_string(&config).unwrap();
    fs::write(
        &config,
        format!(
            "default_permissions = \"edit\"\n{before}\n[permissions.edit.filesystem.\":workspace_roots\"]\n\".\" = \"write\"\n\"**/*.env\" = \"deny\"\n"
        ),
    )
    .unwrap();
    let cursor = Path::new(repo.path()).join(".cursor");
    fs::create_dir_all(&cursor).unwrap();
    fs::write(cursor.join("hooks.json"), r#"{"version":1,"hooks":{}}"#).unwrap();
    let (_, report) = doctor(&repo, &homes);
    assert_eq!(
        named(&report, "secret-files-codex"),
        Some(json!("ok")),
        "{report}"
    );
    assert_eq!(
        named(&report, "secret-files-cursor"),
        Some(json!("ok")),
        "{report}"
    );
    fs::write(
        Path::new(repo.path()).join(".cursorignore"),
        "dist/\n!.env\n",
    )
    .unwrap();
    let (_, report) = doctor(&repo, &homes);
    assert_eq!(
        named(&report, "secret-files-cursor"),
        Some(json!("off")),
        "{report}"
    );
    fs::write(&config, before).unwrap();
    fs::remove_dir_all(&cursor).unwrap();
    fs::remove_file(Path::new(repo.path()).join(".cursorignore")).unwrap();

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

/// Claude's status line is how Craft observes context usage: doctor offers the setup command
/// when none is set, accepts Craft's own on the installed package, and names a status line
/// left on an install that is gone. It writes no setting either way.
#[test]
fn doctor_offers_the_claude_status_line_and_finds_one_left_behind() {
    let repo = Repo::new("doctor-statusline", &[("README.md", "x\n")]);
    let homes = homes(&repo);
    let settings = Path::new(repo.path()).join(".claude/settings.local.json");
    fs::create_dir_all(settings.parent().unwrap()).unwrap();
    let write = |status_line: Option<String>| {
        let mut value = json!({"enabledPlugins": {"craft-delivery@mp": true}});
        if let Some(command) = status_line {
            value["statusLine"] = json!({"type": "command", "command": command});
        }
        fs::write(&settings, value.to_string()).unwrap();
    };
    let check = |report: &Value| {
        host(report, "claude")["checks"]
            .as_array()
            .unwrap()
            .iter()
            .find(|c| c["check"] == "status-line")
            .cloned()
            .unwrap_or_else(|| panic!("{report}"))
    };
    let installed = homes.claude.join("plugins/cache/mp/craft-delivery/0.0.5");

    write(None);
    let before = fs::read(&settings).unwrap();
    let (code, report) = doctor(&repo, &homes);
    assert_eq!(code, 0, "{report}");
    assert_eq!(check(&report)["status"], "off");
    let offer = report["next"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(Value::as_str)
        .find(|n| n.contains("host-setup --host claude --package craft-delivery"))
        .unwrap_or_else(|| panic!("{report}"));
    assert!(offer.contains("bin/craft"), "{offer}");
    assert_eq!(
        fs::read(&settings).unwrap(),
        before,
        "doctor writes nothing"
    );

    write(Some(format!(
        "'{}' statusline claude --package 'craft-delivery' --native-stdin",
        installed.join("bin/craft").display()
    )));
    let (_, report) = doctor(&repo, &homes);
    assert_eq!(check(&report)["status"], "ok", "{report}");

    write(Some(
        "'/gone/craft-delivery/0.0.4/bin/craft' statusline claude --package 'craft-delivery' --native-stdin"
            .into(),
    ));
    let (code, report) = doctor(&repo, &homes);
    assert_eq!(code, 0);
    assert_eq!(check(&report)["status"], "problem", "{report}");
    assert!(
        report.to_string().contains("which no longer exists"),
        "{report}"
    );

    // Another copy of the installed version (a marketplace directory) is fine; an older is not.
    for (version, status) in [("0.0.5", "ok"), ("0.0.4", "problem")] {
        let copy = homes.claude.join(format!("marketplace-{version}"));
        support::installed_bundle(&copy, version);
        write(Some(format!(
            "'{}' statusline claude --package 'craft-delivery' --native-stdin",
            copy.join("bin/craft").display()
        )));
        let (_, report) = doctor(&repo, &homes);
        assert_eq!(check(&report)["status"], status, "{report}");
    }

    write(Some("my-own-status-line".into()));
    let (_, report) = doctor(&repo, &homes);
    assert_eq!(check(&report)["status"], "off");
    assert!(
        !report.to_string().contains("host-setup --host claude"),
        "{report}"
    );
}

/// A hook that answered nothing because its input was too large to read is counted, so a
/// silent hook is not taken for one with nothing to say.
#[test]
fn doctor_counts_hook_runs_that_gave_no_answer() {
    use std::io::Write;
    let repo = Repo::new("doctor-unanswered", &[("README.md", "x\n")]);
    let homes = homes(&repo);
    let (_, report) = doctor(&repo, &homes);
    assert!(!report.to_string().contains("gave no answer"), "{report}");

    let large = format!(
        r#"{{"tool_name":"Read","cwd":{},"session_id":"S-1","tool_response":"{}"}}"#,
        json!(repo.path()),
        "x".repeat(1024 * 1024 + 1)
    );
    let mut child = std::process::Command::new(env!("CARGO_BIN_EXE_craft"))
        .args([
            "hook",
            "claude",
            "after-tool",
            "--package",
            "craft-delivery",
        ])
        .args(["--handler", "deliver", "--scope", "any"])
        .current_dir(repo.path())
        .env("CRAFT_EVIDENCE_WARM", "0")
        .env_remove("CRAFT_PACKAGE_ROOT")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(large.as_bytes())
        .unwrap();
    let output = child.wait_with_output().unwrap();
    // Silent to the host: an advisory hook never turns unread input into an error.
    assert_eq!(
        (output.status.code(), output.stdout.as_slice()),
        (Some(0), &b""[..]),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );

    let (_, report) = doctor(&repo, &homes);
    let claude = host(&report, "claude");
    let found = claude["checks"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["check"] == "answers")
        .unwrap_or_else(|| panic!("{claude}"));
    assert_eq!(found["status"], "problem");
    let detail = found["detail"].as_str().unwrap();
    assert!(
        detail.starts_with("1 hook run gave no answer since ")
            && detail.contains(": 1 with input not read")
            && detail.contains("; last after-tool at "),
        "{detail}"
    );
    assert_eq!(report["blocking"], false);
}
