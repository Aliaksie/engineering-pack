//! Rust and `platform/node/state-files.mjs` share record families until JS retires.
//! These checks run real Node processes against the same files; they require `node`.
use craft_platform::state;
use serde_json::{Value, json};
use std::{
    fs,
    io::{BufRead, BufReader},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::atomic::{AtomicU64, Ordering},
    thread,
    time::Duration,
};

const SESSION: &str = "session λ";
static NEXT: AtomicU64 = AtomicU64::new(0);

struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "craft-interop-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        Self(state::canonical_root(&path).unwrap())
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// `node -e` with the JS state module bound to `s`; script arguments start at argv[2].
fn node(body: &str, args: &[&str]) -> Command {
    let module = Path::new(env!("CARGO_MANIFEST_DIR")).join("node/state-files.mjs");
    let mut command = Command::new("node");
    command
        .args(["--input-type=module", "-e"])
        .arg(format!(
            "import {{ pathToFileURL }} from 'node:url';\nconst s = await import(pathToFileURL(process.argv[1]).href);\n{body}"
        ))
        .arg(module)
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    command
}

fn js(body: &str, args: &[&str]) -> String {
    let output = node(body, args)
        .output()
        .expect("node is required for cross-language state tests");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout)
        .unwrap()
        .trim_end()
        .to_string()
}

fn js_spawn(body: &str, args: &[&str]) -> std::process::Child {
    node(body, args)
        .spawn()
        .expect("node is required for cross-language state tests")
}

#[test]
fn both_runtimes_place_records_identically() {
    let fixture = Fixture::new();
    let root = fixture.0.to_str().unwrap();
    for session in [Some(SESSION), None] {
        let rust = state::file(&fixture.0, "record.json", true, session).unwrap();
        let node = js(
            "console.log(s.stateFile(process.argv[2], 'record.json', true, process.argv[3] || undefined));",
            &[root, session.unwrap_or("")],
        );
        assert_eq!(rust, PathBuf::from(node));
    }
}

#[test]
fn records_round_trip_between_runtimes() {
    let fixture = Fixture::new();
    let record = state::file(&fixture.0, "record.json", true, Some(SESSION)).unwrap();
    let value = json!({"z": [1, -2, 3.5], "a": {"text": "λ 😀\n\u{1}"}, "n": null});
    state::write(&record, &value, 4096, true).unwrap();
    let rust_bytes = fs::read(&record).unwrap();
    let echoed = js(
        "const value = s.readState(process.argv[2], 4096); s.writeState(process.argv[2], value, 4096, true); console.log(JSON.stringify(value));",
        &[record.to_str().unwrap()],
    );
    assert_eq!(serde_json::from_str::<Value>(&echoed).unwrap(), value);
    assert_eq!(fs::read(&record).unwrap(), rust_bytes, "identical bytes");
}

#[test]
fn concurrent_writers_share_one_lock_protocol() {
    const ROUNDS: u64 = 40;
    const WRITERS: u64 = 3;
    let fixture = Fixture::new();
    let record = state::file(&fixture.0, "counter.json", true, None).unwrap();
    state::write(&record, &json!({"count": 0}), 1024, false).unwrap();
    let path = record.to_str().unwrap().to_string();
    let rounds = ROUNDS.to_string();
    let children: Vec<_> = (0..WRITERS)
        .map(|_| {
            js_spawn(
                "const [file, rounds] = [process.argv[2], Number(process.argv[3])];
                 const wait = new Int32Array(new SharedArrayBuffer(4));
                 for (let done = 0; done < rounds;) {
                   const ticket = s.lockState(file);
                   if (!ticket) { Atomics.wait(wait, 0, 0, 1); continue; }
                   try { const value = s.readState(file, 1024); s.writeState(file, { count: value.count + 1 }, 1024); done++; }
                   finally { s.releaseState(ticket); }
                 }",
                &[&path, &rounds],
            )
        })
        .collect();
    let threads: Vec<_> = (0..WRITERS)
        .map(|_| {
            let record = record.clone();
            thread::spawn(move || {
                let mut done = 0;
                while done < ROUNDS {
                    let Some(_ticket) = state::lock(&record).unwrap() else {
                        thread::sleep(Duration::from_millis(1));
                        continue;
                    };
                    let count = state::read(&record, 1024).unwrap().unwrap()["count"]
                        .as_u64()
                        .unwrap();
                    state::write(&record, &json!({"count": count + 1}), 1024, false).unwrap();
                    done += 1;
                }
            })
        })
        .collect();
    for thread in threads {
        thread.join().unwrap();
    }
    for child in children {
        let output = child.wait_with_output().unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    assert_eq!(
        state::read(&record, 1024).unwrap(),
        Some(json!({"count": 2 * WRITERS * ROUNDS}))
    );
    let mut names: Vec<_> = fs::read_dir(record.parent().unwrap())
        .unwrap()
        .map(|entry| entry.unwrap().file_name().into_string().unwrap())
        .collect();
    names.sort();
    assert_eq!(names, ["counter.json"], "no lock or temporary file remains");
}

#[test]
fn killed_writers_leave_locks_that_neither_runtime_steals() {
    let fixture = Fixture::new();
    let record = state::file(&fixture.0, "record.json", true, None).unwrap();
    state::write(&record, &json!({"value": 1}), 1024, false).unwrap();
    let path = record.to_str().unwrap();
    let mut holder = js_spawn(
        "s.lockState(process.argv[2]); console.log('locked'); setInterval(() => {}, 1000);",
        &[path],
    );
    let mut line = String::new();
    BufReader::new(holder.stdout.take().unwrap())
        .read_line(&mut line)
        .unwrap();
    assert_eq!(line.trim(), "locked");
    holder.kill().unwrap();
    holder.wait().unwrap();
    assert!(state::lock(&record).unwrap().is_none());
    assert_eq!(
        js("console.log(s.lockState(process.argv[2]));", &[path]),
        "null"
    );
    fs::remove_file(record.with_file_name("record.json.lock")).unwrap();

    // A Rust writer that dies without unwinding leaves its lock file the same way.
    std::mem::forget(state::lock(&record).unwrap().unwrap());
    assert_eq!(
        js("console.log(s.lockState(process.argv[2]));", &[path]),
        "null"
    );
    assert_eq!(
        state::read(&record, 1024).unwrap(),
        Some(json!({"value": 1}))
    );
}

fn legacy_store(root: &Path) {
    let legacy = root.join(".craft").join("state");
    fs::create_dir_all(legacy.join("sessions").join("b λ")).unwrap();
    fs::create_dir_all(legacy.join("sessions").join("a")).unwrap();
    fs::write(legacy.join("workspace-guard.json"), "{\"guard\":true}\n").unwrap();
    fs::write(legacy.join("sessions/a/owner.json"), "{\"owner\":\"a\"}").unwrap();
    fs::write(legacy.join("sessions/b λ/owner.json"), "{\"owner\":\"b\"}").unwrap();
}

fn receipt(record: &Path) -> String {
    let directory = record.parent().unwrap();
    let names: Vec<_> = fs::read_dir(directory)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().into_string().unwrap())
        .filter(|name| name.starts_with("migration-"))
        .collect();
    assert_eq!(names.len(), 1);
    fs::read_to_string(directory.join(&names[0])).unwrap()
}

#[test]
fn legacy_migration_receipts_are_interchangeable() {
    let by_node = Fixture::new();
    let by_rust = Fixture::new();
    legacy_store(&by_node.0);
    legacy_store(&by_rust.0);
    let migrate = "console.log(s.stateFile(process.argv[2], 'workspace-guard.json', false));";
    let node_record = PathBuf::from(js(migrate, &[by_node.0.to_str().unwrap()]));
    let rust_record = state::file(&by_rust.0, "workspace-guard.json", false, None).unwrap();
    assert_eq!(
        receipt(&node_record),
        receipt(&rust_record),
        "same fingerprint"
    );
    for record in [&node_record, &rust_record] {
        let owner = record
            .with_file_name("sessions")
            .join("b λ")
            .join("owner.json");
        assert_eq!(fs::read_to_string(owner).unwrap(), "{\"owner\":\"b\"}");
    }
    // Each runtime accepts the other's receipt and still detects later source changes.
    assert_eq!(
        state::file(&by_node.0, "workspace-guard.json", false, None).unwrap(),
        node_record
    );
    assert_eq!(
        PathBuf::from(js(migrate, &[by_rust.0.to_str().unwrap()])),
        rust_record
    );
    fs::write(
        by_node.0.join(".craft/state/sessions/a/owner.json"),
        "{\"owner\":\"changed\"}",
    )
    .unwrap();
    assert!(
        state::file(&by_node.0, "workspace-guard.json", false, None)
            .unwrap_err()
            .contains("changed after migration")
    );
}
