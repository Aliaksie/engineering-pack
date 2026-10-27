//! Native ownership phases for an integration-owned transport lane: the journal lease, its
//! revision fence, and the admission fence shared with task-start.
mod support;
use craft_engine::digest;
use serde_json::{Value, json};
use std::fs;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use support::{Repo, assert_err, phase};

const SESSION: &str = "s";
const TASK: &str = "t";

struct Lane {
    repo: Repo,
}

impl Lane {
    fn new() -> Self {
        Self {
            repo: Repo::bare("control"),
        }
    }
    fn call(&self, name: &str, extra: Value) -> Result<Value, String> {
        let mut input = json!({"root":self.repo.path(),"session":SESSION,"task":TASK,
            "host":"claude","coordinator":"main","phase":name});
        for (key, value) in extra.as_object().unwrap() {
            input[key] = value.clone();
        }
        phase("control", input)
    }
    fn open(&self, pid: u32) -> Result<Value, String> {
        self.call("open", json!({"pid":pid}))
    }
    fn journal(&self, tree: &Value) -> PathBuf {
        let name = format!("control-{}.json", &digest(&json!(["claude", tree]))[..24]);
        craft_platform::state::file(&self.repo.root, &name, true, Some(SESSION)).unwrap()
    }
    fn start(&self, task: &str, goal: &str) -> Result<Value, String> {
        self.repo.cli(&[
            "task-start",
            "--session",
            SESSION,
            "--task",
            task,
            "--goal",
            goal,
        ])
    }
}

fn key() -> Value {
    json!("a".repeat(64))
}

#[test]
fn the_journal_fences_live_owners_duplicate_keys_stale_revisions_and_old_tickets() {
    let lane = Lane::new();
    let pid = std::process::id();
    let opened = lane.open(pid).unwrap();
    let ticket = &opened["ticket"];
    let request = json!({"key":key(),"source":null});
    assert_err(lane.open(pid), &["already has an owner"]);
    let begun = lane
        .call("begin", json!({"ticket":ticket,"expected":opened["revision"],"input":request,"kind":"worker","nativeId":null}))
        .unwrap();
    assert_err(
        lane.call(
            "begin",
            json!({"ticket":ticket,"expected":begun["revision"],"input":request,"kind":"worker"}),
        ),
        &["already attempted"],
    );
    assert_err(
        lane.call(
            "persist",
            json!({"ticket":ticket,"expected":opened["revision"],"journal":opened["journal"]}),
        ),
        &["journal changed"],
    );
    let mut changed = begun["journal"].clone();
    changed["operations"][0]["source"] = json!("b".repeat(64));
    assert_err(
        lane.call(
            "persist",
            json!({"ticket":ticket,"expected":begun["revision"],"journal":changed}),
        ),
        &["identity changed"],
    );
    let saved: Value =
        serde_json::from_slice(&fs::read(lane.journal(&opened["context"]["tree"])).unwrap())
            .unwrap();
    assert_eq!(saved["operations"].as_array().unwrap().len(), 1);
    lane.call("close", json!({"ticket":ticket})).unwrap();
    let next = lane.open(pid).unwrap();
    assert_ne!(next["ticket"]["lease"], ticket["lease"]);
    assert_eq!(next["journal"]["operations"][0]["phase"], "intent");
    assert_err(
        lane.call("close", json!({"ticket":ticket})),
        &["ownership changed"],
    );
    assert_err(lane.open(pid), &["already has an owner"]);
    lane.call("close", json!({"ticket":next["ticket"]}))
        .unwrap();
}

#[test]
fn malformed_locks_stay_busy_and_a_corrupt_journal_releases_only_its_own_lease() {
    let lane = Lane::new();
    let pid = std::process::id();
    let opened = lane.open(pid).unwrap();
    lane.call("close", json!({"ticket":opened["ticket"]}))
        .unwrap();
    let journal = lane.journal(&opened["context"]["tree"]);
    let lock = PathBuf::from(format!("{}.lock", journal.display()));
    let lease = "a".repeat(64);
    for bytes in [
        String::new(),
        "{}".into(),
        format!(
            r#"{{"owner":"craft-native-lease","formatVersion":1,"pid":{pid}.0,"lease":"{lease}"}}"#
        ),
        json!({"owner":"craft-native-lease","formatVersion":1,"pid":0,"lease":lease}).to_string(),
    ] {
        fs::write(&lock, &bytes).unwrap();
        assert_err(lane.open(pid), &["already has an owner"]);
        assert_eq!(fs::read_to_string(&lock).unwrap(), bytes);
        fs::remove_file(&lock).unwrap();
    }
    let mut corrupt = opened["journal"].clone();
    corrupt["operations"] = json!([{"key":"bad"}]);
    fs::write(&journal, corrupt.to_string()).unwrap();
    assert_err(lane.open(pid), &["invalid bounded native"]);
    assert!(!lock.exists());
}

#[test]
fn a_released_admission_lets_the_binding_move_and_then_refuses_the_old_task() {
    let lane = Lane::new();
    lane.start(TASK, "Inspect explicit scope").unwrap();
    let ticket = lane
        .call("admission-acquire", json!({"pid":std::process::id()}))
        .unwrap();
    lane.call("admission-check", json!({"ticket":ticket}))
        .unwrap();
    // The host effect failed; the integration releases its fence.
    lane.call("admission-release", json!({"ticket":ticket}))
        .unwrap();
    lane.start("other", "Continue scope").unwrap();
    assert_err(
        lane.call("admission-acquire", json!({"pid":std::process::id()})),
        &["active task binding"],
    );
}

#[test]
fn a_dead_owner_is_reclaimed_by_journal_open_and_task_start_without_replay() {
    let lane = Lane::new();
    // A non-Git task still has a bound checkout identity and must respect admission.
    lane.start(TASK, "Inspect explicit scope").unwrap();
    // The owner is a separate live process: craft waiting for its request on stdin.
    let mut owner = Command::new(env!("CARGO_BIN_EXE_craft"))
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .spawn()
        .unwrap();
    let pid = owner.id();
    let opened = lane.open(pid).unwrap();
    lane.call("begin", json!({"ticket":opened["ticket"],"expected":opened["revision"],"input":{"key":key()},"kind":"worker"}))
        .unwrap();
    let admission = lane.call("admission-acquire", json!({"pid":pid})).unwrap();
    assert_err(lane.open(std::process::id()), &["already has an owner"]);
    assert_err(lane.start("other", "New scope"), &["writer is busy"]);
    owner.kill().unwrap();
    owner.wait().unwrap();
    let recovered = lane.open(std::process::id()).unwrap();
    let operations = recovered["journal"]["operations"].as_array().unwrap();
    assert_eq!(operations.len(), 1);
    assert_eq!(operations[0]["phase"], "intent");
    assert_err(
        lane.call("close", json!({"ticket":opened["ticket"]})),
        &["ownership changed"],
    );
    lane.call("close", json!({"ticket":recovered["ticket"]}))
        .unwrap();
    lane.start("other", "New scope").unwrap();
    assert_err(
        lane.call("admission-check", json!({"ticket":admission})),
        &["unavailable", "ownership"],
    );
}

#[test]
fn support_returns_the_shared_host_capability_table() {
    let table: Value = serde_json::from_str(include_str!("../../hosts/capabilities.json")).unwrap();
    for (host, expected) in table.as_object().unwrap() {
        assert_eq!(
            phase("control", json!({"phase":"support","host":host})).unwrap(),
            *expected
        );
    }
    assert_err(
        phase("control", json!({"phase":"support","host":"missing"})),
        &["unknown host adapter"],
    );
}
