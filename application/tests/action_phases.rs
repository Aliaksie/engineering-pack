//! Native action phases driven the way an integration-owned host transport drives them:
//! find the offer, claim it with a fresh observation, execute outside native locks, finish.
mod support;
use craft_engine::digest;
use serde_json::{Value, json};
use std::fs;
use support::{Repo, assert_err, at, bundle, now, phase};

const SESSION: &str = "s1";
const TASK: &str = "T1";

fn call(root: &str, name: &str, extra: Value) -> Result<Value, String> {
    let mut input = json!({"root":root,"session":SESSION,"task":TASK,"phase":name});
    for (key, value) in extra.as_object().unwrap() {
        input[key] = value.clone();
    }
    phase("actions", input)
}

fn observation(facts: Value) -> Value {
    json!({"sessionId":SESSION,"observedAt":now(),"facts":facts})
}

fn ready() -> Value {
    observation(json!({"allowed":true,"supported":true}))
}

struct Fixture {
    repo: Repo,
    offer: String,
}

impl Fixture {
    /// A verified task whose next step is an offered review of the current source.
    fn new() -> Self {
        let repo = Repo::new("actions", &[("file.txt", "source")]);
        repo.cli(&[
            "task-start",
            "--session",
            SESSION,
            "--task",
            TASK,
            "--goal",
            "Verify the scoped change",
            "--risk",
            "security",
        ])
        .unwrap();
        let verified = repo
            .cli(&[
                "task-step",
                "--session",
                SESSION,
                "--task",
                TASK,
                "--stage",
                "verify",
                "--target",
                "acceptance:scope",
                "--approach",
                "bounded-check",
                "--evidence",
                "checks:current",
                "--outcome",
                "passed",
                "--result",
                "Acceptance checked",
                "--next",
                "Review current source",
                "--bundle",
                &bundle(),
            ])
            .unwrap();
        let offer = verified["actionOffer"]["offer"]["id"]
            .as_str()
            .unwrap()
            .to_string();
        Self { repo, offer }
    }
    fn root(&self) -> &str {
        self.repo.path()
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
                "user:scoped-decision",
            ])
            .unwrap();
    }
    fn claim(&self, id: &str, observation: Value) -> Result<Value, String> {
        call(
            self.root(),
            "claim",
            json!({"id":id,"observation":observation}),
        )
    }
    fn finish(&self, id: &str, receipt: &Value, result: Value) -> Result<Value, String> {
        call(
            self.root(),
            "finish",
            json!({"id":id,"receipt":receipt,"result":result}),
        )
    }
    /// Records a host result; an unpersistable one falls back to an uncertain receipt.
    fn settle(&self, id: &str, receipt: &Value, result: Value) -> Value {
        self.finish(id, receipt, result).unwrap_or_else(|_| {
            let mut settled = self
                .finish(id, receipt, json!({"state":"uncertain"}))
                .unwrap();
            settled["uncertain"] = json!(true);
            settled
        })
    }
    fn completed(receipt: &Value, evidence: &str) -> Value {
        json!({"state":"completed","source":receipt["source"],"evidence":evidence})
    }
    fn features(&self) -> Value {
        self.repo
            .cli(&["task-features", "--session", SESSION, "--task", TASK])
            .unwrap()["features"]
            .clone()
    }
}

#[test]
fn execution_requires_a_recorded_approval_and_keeps_one_attempt_per_offer() {
    let f = Fixture::new();
    assert_err(f.claim(&f.offer, ready()), &["pending", "approval"]);
    f.decide(&f.offer, "declined");
    assert_err(f.claim(&f.offer, ready()), &["declined"]);
    assert_eq!(f.features()["review"]["status"], "unknown");
    f.decide(&f.offer, "approved");
    let claimed = f.claim(&f.offer, ready()).unwrap();
    assert_ne!(claimed["duplicate"], true);
    let receipt = &claimed["receipt"];
    let result = f.settle(
        &f.offer,
        receipt,
        Fixture::completed(receipt, "host:review-current-source"),
    );
    assert_eq!(result["executed"], true);
    let observed = &f.repo.session_state(SESSION, TASK)["featureOutcomes"]["review"];
    assert_eq!(observed["status"], "used");
    assert_eq!(observed["provenance"], "runtime-observed");
    assert_eq!(observed["evidence"], "host:review-current-source");
    let found = call(f.root(), "find", json!({"id":f.offer})).unwrap();
    assert_eq!(found["executionReceipt"]["state"], "completed");
}

#[test]
fn an_unknown_outcome_is_never_redispatched_and_reconciles_from_a_lookup() {
    let f = Fixture::new();
    f.decide(&f.offer, "approved");
    let claimed = f.claim(&f.offer, ready()).unwrap();
    let receipt = &claimed["receipt"];
    // The host call failed without an acknowledgement.
    let first = f.settle(&f.offer, receipt, json!({"state":"uncertain"}));
    assert_eq!(first["uncertain"], true);
    assert_eq!(first["executed"], false);
    let found = call(f.root(), "find", json!({"id":f.offer})).unwrap();
    assert_eq!(found["executionReceipt"]["state"], "uncertain");
    let again = f.claim(&f.offer, ready()).unwrap();
    assert_eq!(again["duplicate"], true);
    let looked_up = f.settle(
        &f.offer,
        &found["executionReceipt"],
        Fixture::completed(receipt, "host:operation-confirmed"),
    );
    assert_eq!(looked_up["executed"], true);
}

#[test]
fn a_second_caller_shares_the_durable_claim_while_the_host_call_is_pending() {
    let f = Fixture::new();
    f.decide(&f.offer, "approved");
    let first = f.claim(&f.offer, ready()).unwrap();
    let second = f.claim(&f.offer, ready()).unwrap();
    assert_eq!(second["duplicate"], true);
    assert_ne!(second["receipt"]["state"], "completed");
    let receipt = &first["receipt"];
    let result = f.settle(
        &f.offer,
        receipt,
        Fixture::completed(receipt, "host:review-complete"),
    );
    assert_eq!(result["executed"], true);
}

#[test]
fn a_source_change_rejects_the_claim_and_a_mismatched_completion_is_not_completed() {
    let f = Fixture::new();
    f.decide(&f.offer, "approved");
    f.repo.write("file.txt", "sourcechanged");
    assert_err(f.claim(&f.offer, ready()), &["stale"]);
    f.repo.write("file.txt", "source");
    let claimed = f.claim(&f.offer, ready()).unwrap();
    let result = f.settle(
        &f.offer,
        &claimed["receipt"],
        json!({"state":"completed","source":"a".repeat(64),"evidence":"host:wrong-source"}),
    );
    assert_eq!(result["executed"], false);
    assert_eq!(result["uncertain"], true);
}

#[test]
fn amended_task_criteria_invalidate_an_approval_without_a_source_edit() {
    let f = Fixture::new();
    f.decide(&f.offer, "approved");
    let contract = format!(
        "{}/../tests/fixtures/acceptance/contract.json",
        env!("CARGO_MANIFEST_DIR")
    );
    f.repo
        .cli(&[
            "task-start",
            "--session",
            SESSION,
            "--task",
            TASK,
            "--goal",
            "Verify the scoped change",
            "--contract",
            &contract,
        ])
        .unwrap();
    match f.claim(&f.offer, ready()) {
        Err(error) => assert!(
            error.contains("stale") && error.contains("contract"),
            "{error}"
        ),
        Ok(value) => panic!("claimed against an amended contract: {value}"),
    }
    assert_eq!(
        f.repo.session_state(SESSION, TASK)["taskRecord"]["goalComplete"],
        false
    );
}

#[test]
fn claims_reject_stale_observations_a_busy_binding_writer_and_a_switched_task() {
    let f = Fixture::new();
    f.decide(&f.offer, "approved");
    let facts = json!({"allowed":true,"supported":true});
    for stale in [
        json!({"sessionId":SESSION,"observedAt":at(-31000),"facts":facts}),
        json!({"sessionId":SESSION,"observedAt":at(60000),"facts":facts}),
        json!({"sessionId":"other","observedAt":now(),"facts":{}}),
    ] {
        assert_err(f.claim(&f.offer, stale), &["fresh matching"]);
    }
    let lock = format!("{}.lock", f.repo.binding_file(SESSION).display());
    fs::write(&lock, "").unwrap();
    let busy = f.claim(&f.offer, ready());
    fs::remove_file(&lock).unwrap();
    assert_err(busy, &["writer is busy"]);
    f.repo
        .cli(&[
            "task-start",
            "--session",
            SESSION,
            "--task",
            "T2",
            "--goal",
            "Another outcome",
        ])
        .unwrap();
    assert_err(f.claim(&f.offer, ready()), &["active coordinator task"]);
}

#[test]
fn compaction_binds_the_checkpoint_and_counts_only_after_matching_resume_evidence() {
    let f = Fixture::new();
    let facts = || {
        json!({"allowed":true,"supported":true,"observations":{"sessionId":SESSION,
            "host":{"name":"fixture","reliableCompaction":true},
            "usage":{"kind":"current-context","currentTokens":90,"capacityTokens":100,"sessionId":SESSION,"source":"host:context","observedAt":now()}}})
    };
    let data = f.repo.input("compact.json", &facts());
    let offered = f
        .repo
        .cli(&[
            "task-offer",
            "--session",
            SESSION,
            "--task",
            TASK,
            "--action",
            "compact",
            "--scope",
            "Current task",
            "--benefit",
            "Continue with bounded context",
            "--evidence",
            "host:context",
            "--data",
            &data,
            "--bundle",
            &bundle(),
        ])
        .unwrap();
    let id = offered["offer"]["id"].as_str().unwrap().to_string();
    f.decide(&id, "approved");
    let claimed = f.claim(&id, observation(facts())).unwrap();
    let receipt = &claimed["receipt"];
    let issued = f.settle(&id, receipt, Fixture::completed(receipt, "host:issued"));
    assert_eq!(issued["uncertain"], true);
    assert!(
        f.repo.session_state(SESSION, TASK)["featureOutcomes"]
            .get("compaction")
            .is_none()
    );
    let found = call(f.root(), "find", json!({"id":id})).unwrap();
    let receipt = &found["executionReceipt"];
    let resumed = f.settle(
        &id,
        receipt,
        json!({"state":"completed","source":receipt["source"],"checkpoint":receipt["checkpoint"],"resumed":true,
            "evidence":"host:resumed","acceptanceEvidence":"checks:after-resume"}),
    );
    assert_eq!(resumed["executed"], true);
    let compaction = &f.repo.session_state(SESSION, TASK)["featureOutcomes"]["compaction"];
    assert_eq!(compaction["provenance"], "runtime-observed");
    let saved = f
        .repo
        .cli(&["checkpoint-read", "--session", SESSION])
        .unwrap();
    assert_eq!(
        resumed["receipt"]["checkpoint"],
        json!(digest(&saved["saved"]))
    );
}

#[test]
fn an_unpersistable_completion_keeps_the_recovery_key_and_blocks_repeats() {
    let f = Fixture::new();
    f.decide(&f.offer, "approved");
    let claimed = f.claim(&f.offer, ready()).unwrap();
    let receipt = &claimed["receipt"];
    let lock = format!("{}.lock", f.repo.session_file(SESSION, TASK).display());
    fs::write(&lock, "").unwrap();
    let completed = f.finish(
        &f.offer,
        receipt,
        Fixture::completed(receipt, "host:completed"),
    );
    let uncertain = f.finish(&f.offer, receipt, json!({"state":"uncertain"}));
    fs::remove_file(&lock).unwrap();
    assert!(completed.is_err() && uncertain.is_err());
    let found = call(f.root(), "find", json!({"id":f.offer})).unwrap();
    assert_eq!(found["executionReceipt"]["key"], receipt["key"]);
    assert_ne!(found["executionReceipt"]["state"], "completed");
    assert_eq!(f.claim(&f.offer, ready()).unwrap()["duplicate"], true);
}

#[test]
fn the_craft_process_posts_the_phase_telemetry_export_itself() {
    use std::io::{Read, Write};
    use std::process::{Command, Stdio};
    let f = Fixture::new();
    f.decide(&f.offer, "approved");
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let endpoint = format!("http://{}/v1/traces", listener.local_addr().unwrap());
    f.repo.write(
        ".craft/telemetry.json",
        &json!({"remote":{"enabled":true,"endpoint":endpoint}}).to_string(),
    );
    listener.set_nonblocking(true).unwrap();
    let server = std::thread::spawn(move || {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        let mut stream = loop {
            match listener.accept() {
                Ok((stream, _)) => break stream,
                Err(_) if std::time::Instant::now() < deadline => {
                    std::thread::sleep(std::time::Duration::from_millis(20))
                }
                Err(_) => return String::new(),
            }
        };
        stream.set_nonblocking(false).unwrap();
        stream
            .set_read_timeout(Some(std::time::Duration::from_secs(5)))
            .unwrap();
        let mut request = Vec::new();
        let mut buffer = [0; 8192];
        // Read the headers and the declared body.
        loop {
            let n = stream.read(&mut buffer).unwrap();
            request.extend_from_slice(&buffer[..n]);
            let text = String::from_utf8_lossy(&request);
            if let Some(end) = text.find("\r\n\r\n") {
                let length = text[..end]
                    .lines()
                    .find_map(|l| {
                        l.to_ascii_lowercase()
                            .strip_prefix("content-length:")
                            .map(|v| v.trim().parse::<usize>().unwrap())
                    })
                    .unwrap_or(0);
                if request.len() >= end + 4 + length || n == 0 {
                    break;
                }
            }
        }
        stream
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\n{}")
            .unwrap();
        String::from_utf8_lossy(&request).into_owned()
    });
    let request = json!({"formatVersion":1,"operation":"actions","input":{"root":f.root(),"session":SESSION,"task":TASK,
        "phase":"claim","id":f.offer,"observation":ready()}});
    let mut child = Command::new(env!("CARGO_BIN_EXE_craft"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(request.to_string().as_bytes())
        .unwrap();
    let output = child.wait_with_output().unwrap();
    let response: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(response["ok"], true, "{response}");
    // The configuration file is new source, so the claim itself is refused as stale.
    assert!(
        response["result"]["error"]
            .as_str()
            .unwrap()
            .contains("stale"),
        "{response}"
    );
    assert!(response["result"].get("export").is_none(), "{response}");
    let posted = server.join().unwrap();
    assert!(posted.starts_with("POST /v1/traces"), "{posted}");
    assert!(posted.contains("resourceSpans"), "{posted}");
}
