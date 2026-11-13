//! Fixtures for tests that play an integration's role around the native worker, action and
//! control phases: a committed repository, public CLI commands and session records.
#![allow(dead_code)]
use craft_engine::digest;
use craft_platform::state;
use serde_json::{Value, json};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU32, Ordering};

pub struct Repo {
    pub root: PathBuf,
}

impl Drop for Repo {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
        let _ = fs::remove_dir_all(self.root.with_extension("inputs"));
    }
}

impl Repo {
    /// A committed repository holding `files`; one directory per fixture.
    pub fn new(prefix: &str, files: &[(&str, &str)]) -> Self {
        static NEXT: AtomicU32 = AtomicU32::new(0);
        let dir = std::env::temp_dir().join(format!(
            "craft-{prefix}-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let repo = Self {
            root: plain(dir.canonicalize().unwrap()),
        };
        repo.git(&["init", "-q"]);
        for (file, text) in files {
            repo.write(file, text);
        }
        repo.git(&["add", "."]);
        repo.git(&["commit", "-qm", "fixture"]);
        repo
    }

    /// A directory that is not a repository.
    pub fn bare(prefix: &str) -> Self {
        static NEXT: AtomicU32 = AtomicU32::new(0);
        let dir = std::env::temp_dir().join(format!(
            "craft-{prefix}-bare-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        Self {
            root: plain(dir.canonicalize().unwrap()),
        }
    }

    pub fn path(&self) -> &str {
        self.root.to_str().unwrap()
    }

    pub fn write(&self, file: &str, text: &str) {
        let path = self.root.join(file);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }

    pub fn git(&self, args: &[&str]) {
        let output = Command::new("git")
            .arg("-C")
            .arg(&self.root)
            .args([
                "-c",
                "core.hooksPath=/dev/null",
                "-c",
                "commit.gpgsign=false",
                "-c",
                "user.name=Fixture",
                "-c",
                "user.email=fixture@example.test",
            ])
            .args(args)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    /// One public CLI command with structured output; `Err` carries its diagnostic.
    pub fn cli(&self, args: &[&str]) -> Result<Value, String> {
        let mut argv = vec![args[0].to_string()];
        argv.extend(["--root".into(), self.path().into()]);
        argv.extend(args[1..].iter().map(|arg| arg.to_string()));
        let outcome = craft_application::cli::run_structured(&argv);
        if outcome.exit_code != 0 {
            return Err(outcome.stderr.trim_end().to_string());
        }
        Ok(serde_json::from_str(&outcome.stdout).unwrap())
    }

    /// A JSON input file outside the repository tree, for `--data` and `--contract`.
    pub fn input(&self, name: &str, value: &Value) -> String {
        let dir = self.root.with_extension("inputs");
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join(name);
        fs::write(&path, value.to_string()).unwrap();
        path.to_str().unwrap().to_string()
    }

    pub fn session_file(&self, session: &str, task: &str) -> PathBuf {
        let name = format!("activity-{}.json", &digest(&json!([session, task]))[..24]);
        state::file(&self.root, &name, true, Some(session)).unwrap()
    }

    pub fn binding_file(&self, session: &str) -> PathBuf {
        state::file(&self.root, "task.json", true, Some(session)).unwrap()
    }

    pub fn session_state(&self, session: &str, task: &str) -> Value {
        serde_json::from_slice(&fs::read(self.session_file(session, task)).unwrap()).unwrap()
    }

    /// Edits a session record as a concurrent writer would, under its lock.
    pub fn update_session(&self, session: &str, task: &str, edit: impl FnOnce(&mut Value)) {
        let file = self.session_file(session, task);
        let _lock = state::lock(&file).unwrap().expect("session record lock");
        let mut record: Value = serde_json::from_slice(&fs::read(&file).unwrap()).unwrap();
        edit(&mut record);
        state::write(&file, &record, 131072, false).unwrap();
    }
}

/// The repository root, used as the installed bundle for guidance accounting.
pub fn bundle() -> String {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .to_str()
        .unwrap()
        .to_string()
}

/// Physical, but without Windows' verbatim prefix, as the hosts pass roots.
fn plain(physical: PathBuf) -> PathBuf {
    match physical.to_str().and_then(|p| p.strip_prefix(r"\\?\")) {
        Some(path) => PathBuf::from(path),
        None => physical,
    }
}

/// One native phase request; `Err` carries the phase's failure message.
pub fn phase(operation: &str, input: Value) -> Result<Value, String> {
    let response =
        craft_application::evaluate(json!({"formatVersion":1,"operation":operation,"input":input}));
    if response["ok"] != true {
        return Err(response["error"]["message"]
            .as_str()
            .unwrap_or_default()
            .to_string());
    }
    let result = response["result"].clone();
    // Worker and action phases wrap their value with a telemetry export.
    match operation {
        "workers" | "actions" => match result.get("error") {
            Some(error) => Err(error.as_str().unwrap().to_string()),
            None => Ok(result["value"].clone()),
        },
        _ => Ok(result),
    }
}

pub fn now() -> String {
    at(0)
}

/// An RFC 3339 instant `offset_ms` from now.
pub fn at(offset_ms: i64) -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64;
    craft_engine::timestamp::format(now + offset_ms)
}

pub fn assert_err(result: Result<Value, String>, pattern: &[&str]) {
    match result {
        Ok(value) => panic!("expected failure matching {pattern:?}, got {value}"),
        Err(error) => assert!(
            pattern.iter().any(|p| error.contains(p)),
            "{error:?} does not match {pattern:?}"
        ),
    }
}

/// A Craft bundle for this platform at `dir`: launcher, manifest and runtime bytes.
pub fn installed_bundle(dir: &Path, version: &str) {
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

pub struct Homes {
    pub claude: PathBuf,
    pub codex: PathBuf,
}

/// Claude Code and Codex homes with Craft installed for `repo`: the Claude package enabled
/// locally for it, the Codex plugin enabled, the project trusted and one hook approved.
pub fn homes(repo: &Repo) -> Homes {
    let base = PathBuf::from(repo.path()).with_extension("homes");
    let _ = fs::remove_dir_all(&base);
    let claude = base.join("claude");
    let codex = base.join("codex");
    let installed = claude.join("plugins/cache/mp/craft-delivery/0.0.5");
    installed_bundle(&installed, "0.0.5");
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
    installed_bundle(
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

/// `craft doctor --json` over `homes`: its exit code and report.
pub fn doctor(repo: &Repo, homes: &Homes) -> (i32, Value) {
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

/// `craft hook codex session-start` for the delivery package, Codex's home at `home`.
pub fn codex_started(project: &Path, home: &Path, session: &str) -> String {
    use std::io::Write;
    let mut child = std::process::Command::new(env!("CARGO_BIN_EXE_craft"))
        .args(["hook", "codex", "session-start"])
        .args(["--package", "craft-delivery", "--handler", "deliver"])
        .env("CRAFT_EVIDENCE_WARM", "0")
        .env_remove("CRAFT_PACKAGE_ROOT")
        .env("CODEX_HOME", home)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    let payload = json!({"session_id": session, "cwd": project, "hook_event_name": "SessionStart", "source": "startup"});
    child
        .stdin
        .take()
        .unwrap()
        .write_all(payload.to_string().as_bytes())
        .unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).into_owned()
}

/// A Codex session record as Codex writes it: its header, then a turn when one ran.
pub fn codex_session(home: &Path, id: &str, cwd: &Path, turned: bool) {
    let started = craft_engine::timestamp::format(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as i64,
    );
    let day = home.join(format!(
        "sessions/{}/{}/{}",
        &started[..4],
        &started[5..7],
        &started[8..10]
    ));
    fs::create_dir_all(&day).unwrap();
    let mut text = format!(
        "{}\n",
        json!({"timestamp": started, "type": "session_meta", "payload": {"id": id, "timestamp": started, "cwd": cwd, "originator": "codex-tui", "source": "vscode"}})
    );
    if turned {
        text.push_str(&format!(
            "{}\n",
            json!({"timestamp": started, "type": "event_msg", "payload": {"type": "task_started", "turn_id": "t"}})
        ));
    }
    fs::write(day.join(format!("rollout-{started}-{id}.jsonl")), text).unwrap();
}
