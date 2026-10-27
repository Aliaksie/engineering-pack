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

    fn git(&self, args: &[&str]) {
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
        let name = format!("session-{}.json", &digest(&json!([session, task]))[..24]);
        state::file(&self.root, &name, true, Some(session)).unwrap()
    }

    pub fn binding_file(&self, session: &str) -> PathBuf {
        let name = format!("active-{}.json", &digest(&json!(session))[..24]);
        state::file(&self.root, &name, true, Some(session)).unwrap()
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
