//! Native worker phases driven the way an integration-owned host transport drives them:
//! prepare, observe, admit, dispatch outside native locks, then record the observation.
mod support;
use serde_json::{Value, json};
use std::fs;
use support::{Repo, assert_err, bundle, now, phase};

const SESSION: &str = "s1";
const TASK: &str = "T1";

fn brief() -> Value {
    json!({"goal":"Inspect one independent source","sources":["value.mjs"],"acceptance":["Return source evidence"],"restrictions":["Read only"],"returnShape":"Findings and evidence"})
}

/// The native host inventory a transport would observe.
#[derive(Default)]
struct Host {
    workers: Vec<Value>,
    operations: Vec<(String, String)>,
    fenced: Vec<String>,
    denied: bool,
    sequence: u32,
    calls: Vec<Value>,
    lose_acknowledgement: bool,
    model: Option<&'static str>,
}

impl Host {
    fn occupied(&self) -> usize {
        self.workers
            .iter()
            .filter(|w| w["occupiesSlot"] == true)
            .count()
    }
    fn operation(&self, key: &str) -> Option<&str> {
        self.operations
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, id)| id.as_str())
    }
    fn worker(&mut self, id: &Value) -> &mut Value {
        self.workers.iter_mut().find(|w| w["id"] == *id).unwrap()
    }
    fn observe(&self, requests: &Value) -> Value {
        let mut lookups = vec![];
        for request in requests.as_array().into_iter().flatten() {
            for key in [&request["key"], &request["cancelKey"]] {
                let Some(key) = key.as_str() else { continue };
                if let Some(id) = self.operation(key) {
                    lookups.push(json!({"key":key,"status":"started","settled":true,"workerId":id,"evidence":"host:lookup"}));
                } else if self.fenced.iter().any(|k| k == key) {
                    lookups.push(json!({"key":key,"status":"not-started","fenced":true,"workerId":null,"evidence":"host:permanent-rejection"}));
                }
            }
        }
        json!({"host":"fixture","capacity":4,"occupiedSlots":1 + self.occupied(),"taskWorkers":self.occupied(),
            "allowed":!self.denied,"sessionId":SESSION,"observedAt":now(),"source":"fixture-native-inventory",
            "workers":self.workers,"requests":lookups})
    }
    fn dispatch(&mut self, call: &Value) -> Result<Value, String> {
        self.calls.push(call.clone());
        let key = call["key"].as_str().unwrap().to_string();
        let id = match call["reuseWorker"].as_str() {
            Some(id) => id.to_string(),
            None => {
                if self.occupied() >= 3 {
                    return Err("native capacity".into());
                }
                self.sequence += 1;
                format!("worker-{}", self.sequence)
            }
        };
        let model = self.model.map_or(call["model"].clone(), |m| json!(m));
        let worker = json!({"id":id,"status":"running","occupiesSlot":true,"model":model,"evidence":"host:started"});
        self.workers.retain(|w| w["id"] != worker["id"]);
        self.workers.push(worker);
        self.operations.push((key, id.clone()));
        if self.lose_acknowledgement {
            return Err("acknowledgement lost".into());
        }
        Ok(
            json!({"workerId":id,"status":"running","occupiesSlot":true,"model":model,"evidence":"host:started"}),
        )
    }
    fn cancel(&mut self, key: &Value, worker: &Value) -> Value {
        self.operations.push((
            key.as_str().unwrap().into(),
            worker.as_str().unwrap().into(),
        ));
        let entry = self.worker(worker);
        entry["status"] = json!("cancelled");
        entry["occupiesSlot"] = json!(false);
        json!({"workerId":worker,"status":"cancelled","occupiesSlot":false,"model":entry["model"],"evidence":entry["evidence"]})
    }
}

enum Admission {
    Existing(Value),
    Request(Value),
}

/// The integration side of one coordinator task; every state transition is a native phase.
struct Controller {
    root: String,
    preferences: Option<String>,
}

impl Controller {
    fn call(&self, name: &str, extra: Value) -> Result<Value, String> {
        let mut input = json!({"root":self.root,"session":SESSION,"task":TASK,"phase":name});
        if let Some(file) = &self.preferences {
            input["preferencesFile"] = json!(file);
        }
        for (key, value) in extra.as_object().unwrap() {
            input[key] = value.clone();
        }
        phase("workers", input)
    }
    fn status(&self) -> Value {
        self.call("status", json!({})).unwrap()
    }
    fn admit_with(
        &self,
        input: &Value,
        snapshot: impl Fn(&Value) -> Value,
    ) -> Result<Admission, String> {
        let before = self.call("prepare", json!({"input":input,"inspect":true}))?;
        if !before["existing"].is_null() {
            return Ok(Admission::Existing(before["existing"].clone()));
        }
        let admitted = self.call(
            "admit",
            json!({"input":input,"targets":before["targets"],"snapshot":snapshot(&before["targets"])}),
        )?;
        if admitted["duplicate"] == true {
            let existing = self.call("prepare", json!({"input":input}))?;
            return Ok(Admission::Existing(existing["existing"].clone()));
        }
        Ok(Admission::Request(admitted["request"].clone()))
    }
    fn admit(&self, host: &Host, input: &Value) -> Result<Admission, String> {
        self.admit_with(input, |targets| host.observe(targets))
    }
    /// The bounded brief and the exact native request a transport sends to the host.
    fn host_call(&self, request: &Value, input: &Value) -> Value {
        let context = self.call("open", json!({})).unwrap()["context"].clone();
        json!({"tree":context["tree"],"root":context["root"],"session":SESSION,"task":TASK,
            "key":request["key"],"model":request["model"],"role":request["role"],"profile":request["profile"],
            "source":request["source"],"revision":request["revision"],"taskContract":request["taskContract"],
            "parent":input["parent"],"reuseWorker":input["reuseWorker"],"brief":input["brief"],"scope":input["scope"],
            "authorizationEvidence":"user:scoped-request"})
    }
    fn dispatch(&self, host: &mut Host, input: &Value) -> Result<Value, String> {
        let request = match self.admit(host, input)? {
            Admission::Existing(existing) => return Ok(existing),
            Admission::Request(request) => request,
        };
        match host.dispatch(&self.host_call(&request, input)) {
            Ok(result) => self.observed(&input["id"], result, &request["generation"]),
            Err(_) => {
                self.record(
                    &input["id"],
                    json!({"kind":"uncertain"}),
                    Some(&request["generation"]),
                )?;
                Ok(json!({"uncertain":true,"recoveryKey":request["key"]}))
            }
        }
    }
    fn record(
        &self,
        id: &Value,
        result: Value,
        generation: Option<&Value>,
    ) -> Result<Value, String> {
        let mut input = json!({"id":id,"result":result});
        if let Some(generation) = generation {
            input["generation"] = generation.clone();
        }
        self.call("record", input)
    }
    fn observed(&self, id: &Value, mut result: Value, generation: &Value) -> Result<Value, String> {
        result["kind"] = json!("observed");
        self.record(id, result, Some(generation))
    }
    fn reconcile_with(&self, snapshot: impl Fn(&Value) -> Value) -> Result<Value, String> {
        let inventory = self.call("inventory", json!({}))?;
        self.call(
            "reconcile",
            json!({"targets":inventory["targets"],"snapshot":snapshot(&inventory["targets"])}),
        )
    }
    fn reconcile(&self, host: &Host) -> Result<Value, String> {
        self.reconcile_with(|targets| host.observe(targets))
    }
    /// Checks and records cancellation intent; the native cancel call happens afterwards.
    fn cancellation(&self, host: &Host, id: &str) -> Result<(Value, Value), String> {
        let before = self.call("inventory", json!({"id":id}))?;
        let request = self.call(
            "cancel-check",
            json!({"request":before["request"],"snapshot":host.observe(&before["targets"])}),
        )?;
        let intent = self.record(&json!(id), json!({"kind":"cancelling"}), None)?;
        Ok((request, intent["request"].clone()))
    }
    fn cancel(&self, host: &mut Host, id: &str) -> Result<Value, String> {
        let (request, intent) = self.cancellation(host, id)?;
        let result = host.cancel(&intent["cancelKey"], &request["workerId"]);
        self.observed(&json!(id), result, &intent["generation"])
    }
}

struct Fixture {
    repo: Repo,
    host: Host,
}

impl Fixture {
    fn new(max_workers: u32) -> Self {
        let project = json!({"formatVersion":1,"efficiency":{"delegation":{"maxWorkers":max_workers}},
            "execution":{"roles":{"researcher":{"mode":"delegate","profile":"standard"}},
            "hosts":{"fixture":{"delegation":true,"models":{"standard":"consumer-worker"}}}}});
        let repo = Repo::new(
            "workers",
            &[
                ("value.mjs", "export const value = 1;\n"),
                (".craft/project.json", &project.to_string()),
            ],
        );
        repo.cli(&[
            "task-start",
            "--session",
            SESSION,
            "--task",
            TASK,
            "--goal",
            "Deliver the scoped change",
            "--host",
            "fixture",
        ])
        .unwrap();
        Self {
            repo,
            host: Host::default(),
        }
    }
    fn controller(&self) -> Controller {
        Controller {
            root: self.repo.path().into(),
            preferences: None,
        }
    }
    /// Proposes delegation from the current host inventory; `decision` records the user's answer.
    fn offer(&self, decision: Option<&str>) -> String {
        let observed = self.host.observe(&json!([]));
        let mut workers = json!({});
        for field in [
            "capacity",
            "occupiedSlots",
            "taskWorkers",
            "allowed",
            "sessionId",
            "observedAt",
            "source",
        ] {
            workers[field] = observed[field].clone();
        }
        let facts = json!({"observations":{"host":{"name":"fixture"}},"allowed":true,"supported":true,"independentTasks":3,"workers":workers});
        let data = self.repo.input("offer.json", &facts);
        let proposed = self
            .repo
            .cli(&[
                "task-offer",
                "--session",
                SESSION,
                "--task",
                TASK,
                "--action",
                "delegate",
                "--scope",
                "sources:independent",
                "--benefit",
                "Inspect independent sources",
                "--evidence",
                "scope:independent",
                "--role",
                "researcher",
                "--data",
                &data,
                "--bundle",
                &bundle(),
            ])
            .unwrap();
        let id = proposed["offer"]["id"].as_str().unwrap().to_string();
        if let Some(decision) = decision {
            self.decide(&id, decision);
        }
        id
    }
    fn decide(&self, id: &str, decision: &str) {
        self.repo
            .cli(&[
                "task-decision",
                "--session",
                SESSION,
                "--task",
                TASK,
                "--offer",
                id,
                "--decision",
                decision,
                "--evidence",
                "user:decision",
            ])
            .unwrap();
    }
    fn workers_file(&self, tree: &Value) -> std::path::PathBuf {
        let name = format!("workers-{}.json", &tree.as_str().unwrap()[..24]);
        craft_platform::state::file(&self.repo.root, &name, true, Some(SESSION)).unwrap()
    }
}

fn assignment(id: &str, offer: &str) -> Value {
    json!({"id":id,"offer":offer,"scope":"sources:independent","brief":brief()})
}

const REJECTED: &[&str] = &["approval", "capacity", "ceiling"];

#[test]
fn approved_dispatch_records_the_observed_worker_once() {
    let mut f = Fixture::new(3);
    let offer = f.offer(None);
    let c = f.controller();
    let input = assignment("one", &offer);
    match c.admit(&f.host, &input) {
        Err(error) => assert!(error.contains("approval"), "{error}"),
        Ok(_) => panic!("an undecided offer was admitted"),
    }
    f.decide(&offer, "approved");
    let result = c.dispatch(&mut f.host, &input).unwrap();
    assert_eq!(result["executionObserved"], true);
    assert_eq!(result["modelMatches"], true);
    let observed = &f.repo.session_state(SESSION, TASK)["featureOutcomes"]["delegation"];
    assert_eq!(observed["status"], "used");
    assert_eq!(observed["provenance"], "runtime-observed");
    assert_eq!(observed["evidence"], "host:started");
    assert_eq!(f.host.calls[0]["model"], "consumer-worker");
    assert_eq!(f.host.calls[0]["brief"], brief());
    assert_eq!(c.dispatch(&mut f.host, &input).unwrap()["duplicate"], true);
    assert_eq!(f.host.calls.len(), 1);
    let mut changed = input.clone();
    changed["brief"]["goal"] = json!("Different task");
    assert_err(c.dispatch(&mut f.host, &changed), &["different content"]);
}

#[test]
fn reservations_bound_nested_dispatch_until_the_slot_is_released() {
    let mut f = Fixture::new(1);
    let offer = f.offer(Some("approved"));
    let c = f.controller();
    let spawned = c
        .dispatch(&mut f.host, &assignment("parent", &offer))
        .unwrap();
    let mut nested = assignment("nested", &offer);
    nested["parent"] = json!("parent");
    assert_err(c.dispatch(&mut f.host, &nested), REJECTED);
    let worker = spawned["request"]["workerId"].clone();
    f.host.worker(&worker)["status"] = json!("completed");
    c.reconcile(&f.host).unwrap();
    assert_err(
        c.dispatch(&mut f.host, &assignment("next", &offer)),
        REJECTED,
    );
    assert_eq!(f.host.calls.len(), 1);
    let entry = f.host.worker(&worker);
    entry["status"] = json!("released");
    entry["occupiesSlot"] = json!(false);
    c.reconcile(&f.host).unwrap();
    let next = c
        .dispatch(&mut f.host, &assignment("next", &offer))
        .unwrap();
    assert_eq!(next["executionObserved"], true);
}

#[test]
fn idle_workers_are_reused_at_capacity_and_a_zero_ceiling_stops_new_work() {
    let mut f = Fixture::new(1);
    let offer = f.offer(Some("approved"));
    let c = f.controller();
    let first = c.dispatch(&mut f.host, &assignment("one", &offer)).unwrap();
    let id = first["request"]["workerId"].clone();
    f.host.worker(&id)["status"] = json!("idle");
    c.reconcile(&f.host).unwrap();
    let mut reuse = assignment("two", &offer);
    reuse["reuseWorker"] = id.clone();
    let reused = c.dispatch(&mut f.host, &reuse).unwrap();
    assert_eq!(reused["request"]["workerId"], id);
    assert_eq!(f.host.workers.len(), 1);
    let preferences = f.repo.root.join(".git/preferences.json");
    fs::write(
        &preferences,
        r#"{"efficiency":{"delegation":{"maxWorkers":0}}}"#,
    )
    .unwrap();
    let lowered = Controller {
        root: f.repo.path().into(),
        preferences: Some(preferences.to_str().unwrap().into()),
    };
    assert_err(
        lowered.dispatch(&mut f.host, &assignment("three", &offer)),
        &["stale", "approval", "ceiling"],
    );
    assert_eq!(f.host.worker(&id)["occupiesSlot"], true);
    assert_eq!(f.host.calls.len(), 2);
}

#[test]
fn a_lost_acknowledgement_holds_capacity_until_reconciled_by_its_key() {
    let mut f = Fixture::new(1);
    let offer = f.offer(Some("approved"));
    let c = f.controller();
    f.host.lose_acknowledgement = true;
    let result = c.dispatch(&mut f.host, &assignment("one", &offer)).unwrap();
    assert_eq!(result["uncertain"], true);
    assert_eq!(c.status()["requests"][0]["occupied"], true);
    assert_eq!(
        c.dispatch(&mut f.host, &assignment("one", &offer)).unwrap()["duplicate"],
        true
    );
    assert_err(
        c.dispatch(&mut f.host, &assignment("two", &offer)),
        REJECTED,
    );
    // Without an observed worker there is nothing the host could be asked to cancel.
    assert_err(
        c.cancellation(&f.host, "one").map(|_| json!(null)),
        &["observed occupied native worker"],
    );
    c.reconcile(&f.host).unwrap();
    assert_eq!(c.status()["requests"][0]["workerId"], "worker-1");
    assert_eq!(f.host.calls.len(), 1);
}

#[test]
fn a_late_dispatch_result_cannot_overwrite_a_newer_cancellation() {
    let mut f = Fixture::new(3);
    let offer = f.offer(Some("approved"));
    let c = f.controller();
    let input = assignment("one", &offer);
    let Admission::Request(request) = c.admit(&f.host, &input).unwrap() else {
        panic!("not admitted");
    };
    let late = f.host.dispatch(&c.host_call(&request, &input)).unwrap();
    // The host started the worker; its acknowledgement arrives after a cancellation.
    c.reconcile(&f.host).unwrap();
    c.cancel(&mut f.host, "one").unwrap();
    c.observed(&input["id"], late, &request["generation"])
        .unwrap();
    let status = c.status();
    assert_eq!(status["requests"][0]["occupied"], false);
    assert_eq!(status["requests"][0]["status"], "cancelled");
    let unfenced = |targets: &Value| {
        let mut snapshot = f.host.observe(targets);
        snapshot["requests"] = json!([{"key":"a".repeat(64),"status":"not-started","workerId":null,"evidence":"host:missing"}]);
        snapshot
    };
    assert_err(c.reconcile_with(unfenced), &["fence"]);
}

#[test]
fn a_model_mismatch_stays_visible_and_never_completes_the_task() {
    let mut f = Fixture::new(3);
    let offer = f.offer(Some("approved"));
    let c = f.controller();
    f.host.model = Some("other-native-model");
    let result = c.dispatch(&mut f.host, &assignment("one", &offer)).unwrap();
    assert_eq!(result["modelMatches"], false);
    assert_eq!(result["request"]["occupied"], true);
    let mut child = assignment("child", &offer);
    child["parent"] = json!("one");
    assert_err(c.dispatch(&mut f.host, &child), &["parent"]);
    assert_eq!(
        f.repo.session_state(SESSION, TASK)["taskRecord"]["goalComplete"],
        false
    );
}

#[test]
fn worker_evidence_keeps_its_task_contract_and_goes_stale_after_amendments() {
    let mut f = Fixture::new(3);
    let offer = f.offer(Some("approved"));
    let c = f.controller();
    let spawned = c.dispatch(&mut f.host, &assignment("one", &offer)).unwrap();
    let bound = f.host.calls[0]["taskContract"].clone();
    let contract = bound.as_str().unwrap();
    assert!(contract.len() == 64 && contract.bytes().all(|b| b.is_ascii_hexdigit()));
    assert_eq!(c.status()["requests"][0]["taskContract"], bound);
    assert_eq!(c.status()["requests"][0]["freshness"], "current");
    f.host.worker(&spawned["request"]["workerId"])["status"] = json!("completed");
    f.repo
        .write("value.mjs", "export const value = 1;\n// later source\n");
    c.reconcile(&f.host).unwrap();
    let stale = &c.status()["requests"][0];
    assert_eq!(stale["status"], "completed");
    assert_eq!(stale["freshness"], "stale");
    assert_eq!(
        f.repo.session_state(SESSION, TASK)["taskRecord"]["goalComplete"],
        false
    );
    f.repo.write("value.mjs", "export const value = 1;\n");
    f.repo
        .cli(&[
            "task-start",
            "--session",
            SESSION,
            "--task",
            TASK,
            "--goal",
            "Deliver the scoped change",
            "--risk",
            "new acceptance risk",
        ])
        .unwrap();
    assert_eq!(c.status()["requests"][0]["freshness"], "stale");
    match c.dispatch(&mut f.host, &assignment("two", &offer)) {
        Err(error) => assert!(
            error.contains("stale") && error.contains("contract"),
            "{error}"
        ),
        Ok(value) => panic!("dispatched against an amended contract: {value}"),
    }
    assert_eq!(f.host.calls.len(), 1);
    f.repo.update_session(SESSION, TASK, |state| {
        state["recommendations"]
            .as_object_mut()
            .unwrap()
            .remove(&offer);
    });
    let retained = c.status();
    assert_eq!(retained["requests"][0]["taskContract"], bound);
    assert_eq!(retained["requests"][0]["freshness"], "stale");
    let requests: Vec<Value> = retained["requests"]
        .as_array()
        .unwrap()
        .iter()
        .map(|request| {
            let mut request = request.clone();
            request.as_object_mut().unwrap().remove("freshness");
            request
        })
        .collect();
    let file = f.workers_file(&retained["tree"]);
    fs::write(
        &file,
        json!({"formatVersion":1,"tree":retained["tree"],"requests":requests}).to_string(),
    )
    .unwrap();
    assert_err(
        c.call("status", json!({})),
        &["incompatible worker state version"],
    );
    fs::write(
        &file,
        json!({"formatVersion":2,"tree":retained["tree"],"requests":requests}).to_string(),
    )
    .unwrap();
    assert_eq!(c.status()["requests"][0]["taskContract"], bound);
}

#[test]
fn concurrent_callers_with_stale_snapshots_never_both_pass_a_one_worker_ceiling() {
    let f = Fixture::new(1);
    let offer = f.offer(Some("approved"));
    let root = f.repo.path();
    let barrier = std::sync::Barrier::new(2);
    // Neither snapshot shows the other caller's worker; only the native reservation can.
    let empty = |_: &Value| {
        json!({"host":"fixture","capacity":4,"occupiedSlots":1,"taskWorkers":0,"allowed":true,"sessionId":SESSION,
            "observedAt":now(),"source":"fixture:stale-but-fresh-inventory","workers":[],"requests":[]})
    };
    let results: Vec<Result<Admission, String>> = std::thread::scope(|scope| {
        let handles: Vec<_> = ["left", "right"]
            .into_iter()
            .map(|id| {
                let (barrier, offer) = (&barrier, &offer);
                scope.spawn(move || {
                    let c = Controller { root: root.into(), preferences: None };
                    let input = assignment(id, offer);
                    let before = c.call("prepare", json!({"input":input,"inspect":true})).unwrap();
                    barrier.wait();
                    c.call("admit", json!({"input":input,"targets":before["targets"],"snapshot":empty(&json!([]))}))
                        .map(|admitted| Admission::Request(admitted["request"].clone()))
                })
            })
            .collect();
        handles.into_iter().map(|h| h.join().unwrap()).collect()
    });
    assert_eq!(results.iter().filter(|r| r.is_ok()).count(), 1);
    for result in &results {
        if let Err(error) = result {
            assert!(
                ["capacity", "ceiling", "busy", "approval"]
                    .iter()
                    .any(|p| error.contains(p)),
                "{error}"
            );
        }
    }
    let occupied = f.controller().status()["requests"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|r| r["occupied"] == true)
        .count();
    assert_eq!(occupied, 1);
}

#[test]
fn pending_cancellation_stays_exclusive_and_blocks_worker_reuse() {
    let mut f = Fixture::new(1);
    let offer = f.offer(Some("approved"));
    let c = f.controller();
    let first = c.dispatch(&mut f.host, &assignment("one", &offer)).unwrap();
    let id = first["request"]["workerId"].clone();
    f.host.worker(&id)["status"] = json!("idle");
    c.reconcile(&f.host).unwrap();
    let (request, intent) = c.cancellation(&f.host, "one").unwrap();
    c.reconcile(&f.host).unwrap();
    assert_eq!(c.status()["requests"][0]["state"], "cancelling");
    let mut reuse = assignment("two", &offer);
    reuse["reuseWorker"] = id;
    assert_err(
        c.dispatch(&mut f.host, &reuse),
        &["pending", "scope", "full"],
    );
    let result = f.host.cancel(&intent["cancelKey"], &request["workerId"]);
    c.observed(&json!("one"), result, &intent["generation"])
        .unwrap();
    assert_eq!(f.host.calls.len(), 1);
}

#[test]
fn contract_freshness_survives_an_unavailable_repository_observation() {
    let mut f = Fixture::new(3);
    let offer = f.offer(Some("approved"));
    let c = f.controller();
    c.dispatch(&mut f.host, &assignment("one", &offer)).unwrap();
    f.repo.update_session(SESSION, TASK, |state| {
        state["taskRecord"]["goal"] = json!("Amended task");
        state["taskRecord"]["repositories"] = json!(["missing-repository"]);
    });
    assert_eq!(c.status()["requests"][0]["freshness"], "stale");
}

#[test]
fn input_validation_without_inspection_never_reads_a_corrupt_ledger() {
    let f = Fixture::new(3);
    let offer = f.offer(Some("approved"));
    let c = f.controller();
    let tree = c.status()["tree"].clone();
    fs::write(f.workers_file(&tree), "{").unwrap();
    c.call(
        "prepare",
        json!({"input":assignment("one", &offer),"inspect":false}),
    )
    .unwrap();
    assert_err(
        c.call("prepare", json!({"input":{"id":"bad"},"inspect":false})),
        &["assignment"],
    );
}
