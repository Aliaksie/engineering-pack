//! Native phases around asynchronous host callbacks. Durable state is updated only here;
//! admission shares the active binding lock with task-start, never a host callback.
use crate::{
    failure::Failure,
    project, session,
    telemetry::{Operation, Recorder, now_ms},
};
use craft_engine::{
    actions::{self, Context},
    digest, session as decisions, timestamp,
    workers::lifecycle as workers,
};
use craft_platform::{repository::snapshot, state};
use serde_json::{Map, Value, json};
use std::path::{Path, PathBuf};
const MAX_BYTES: usize = 262144;
struct Store<'a> {
    root: &'a Path,
    session: &'a str,
    task: &'a str,
    tree: String,
}
impl<'a> Store<'a> {
    fn new(root: &'a Path, session: &'a str, task: &'a str) -> Result<Self, Failure> {
        decisions::identity(&json!(session), &json!(task))?;
        let root_path = state::canonical_root(root)?;
        let tree = digest(&json!([root_path.to_string_lossy(), session, task]));
        Ok(Self {
            root,
            session,
            task,
            tree,
        })
    }
    fn file(&self, create: bool) -> Result<PathBuf, Failure> {
        Ok(state::file(
            self.root,
            &format!("workers-{}.json", &self.tree[..24]),
            create,
            Some(self.session),
        )?)
    }
    fn read(&self) -> Result<Value, Failure> {
        Ok(workers::state(
            &session::read_record(&self.file(false)?, MAX_BYTES)?,
            &json!(self.tree),
        )?)
    }
    fn update<T>(&self, f: impl FnOnce(&mut Value) -> Result<T, Failure>) -> Result<T, Failure> {
        let file = self.file(true)?;
        let Some(_lock) = state::lock(&file)? else {
            return Err("worker tree writer is busy".into());
        };
        let mut state = self.read()?;
        let result = f(&mut state)?;
        state::write(&file, &state, MAX_BYTES, false)?;
        Ok(result)
    }
    fn status(&self, recorder: &mut Recorder, parent: &Operation) -> Result<Value, Failure> {
        let state = self.read()?;
        let (task, repository) =
            match session::read_state(self.root, self.session, self.task, recorder, parent) {
                Ok(task) => {
                    let repository = snapshot::workspace(
                        self.root,
                        task["taskRecord"].get("repositories"),
                        false,
                    )
                    .unwrap_or_else(|_| json!({}));
                    (task, repository)
                }
                Err(_) => (json!({}), json!({})),
            };
        Ok(
            json!({"tree":self.tree,"requests":workers::evidence(&state,&task,&repository)?,"transport":"Host callbacks are required; no native transport is installed by configuration.","limits":{"maxRequests":64,"maxBytes":MAX_BYTES},"acceptance":"Worker lifecycle evidence never closes main-task acceptance."}),
        )
    }
}
pub fn status(
    root: &Path,
    session: &str,
    task: &str,
    recorder: &mut Recorder,
    parent: &Operation,
) -> Result<Value, Failure> {
    Store::new(root, session, task)?.status(recorder, parent)
}
fn phase(
    input: &Value,
    root: &Path,
    recorder: &mut Recorder,
    parent: &Operation,
) -> Result<Value, Failure> {
    let name = input["session"]
        .as_str()
        .ok_or("worker operation requires session")?;
    let task = input["task"]
        .as_str()
        .ok_or("worker operation requires task")?;
    let store = Store::new(root, name, task)?;
    let context = json!({"tree":store.tree,"root":state::canonical_root(root)?.to_string_lossy(),"session":name,"task":task});
    match input["phase"].as_str().unwrap_or_default() {
        "open" => Ok(json!({"context":context})),
        "status" => store.status(recorder, parent),
        "prepare" => {
            workers::input(&input["input"])?;
            if input["inspect"] == false {
                return Ok(json!({"context":context}));
            }
            let state = store.read()?;
            let existing = state["requests"]
                .as_array()
                .unwrap()
                .iter()
                .find(|r| r["id"] == input["input"]["id"]);
            if let Some(existing) = existing
                && existing["inputHash"] != digest(&input["input"])
            {
                return Err("worker assignment id already has different content".into());
            }
            Ok(
                json!({"context":context,"targets":workers::targets(&state),"existing":existing.map(|r|json!({"supported":true,"duplicate":true,"dispatchAttempted":false,"executionObserved":workers::execution_known(r),"request":r}))}),
            )
        }
        "inventory" => {
            let state = store.read()?;
            Ok(
                json!({"targets":workers::targets(&state),"request":state["requests"].as_array().unwrap().iter().find(|r|r["id"]==input["id"])}),
            )
        }
        "cancel-check" => {
            let request = &input["request"];
            if request["workerId"].is_null() || request["occupied"] != true {
                return Err("cancellation requires an observed occupied native worker".into());
            }
            let observed = workers::snapshot(&input["snapshot"], name, now_ms())?;
            if observed["host"] != request["host"] {
                return Err("cancellation host identity does not match the occupied worker".into());
            }
            Ok(request.clone())
        }
        "reconcile" => {
            let observed = workers::snapshot(&input["snapshot"], name, now_ms())?;
            store.update(|s| {
                workers::reconcile(s, &observed, &input["targets"])?;
                Ok(())
            })?;
            let mut result = store.status(recorder, parent)?;
            result["host"] = observed["host"].clone();
            result["observedAt"] = observed["observedAt"].clone();
            Ok(result)
        }
        "admit" => {
            workers::input(&input["input"])?;
            let observed = workers::snapshot(&input["snapshot"], name, now_ms())?;
            let Some(_binding) = state::lock(&session::binding_file(root, name, true)?)? else {
                return Err("active task writer is busy".into());
            };
            if session::active_task(root, Some(name))?.as_deref() != Some(task) {
                return Err("worker dispatch requires the active coordinator task binding".into());
            }
            let preferences = input["preferencesFile"].as_str().map(Path::new);
            let settings = project::loaded(recorder, parent, root, preferences)?;
            let saved = session::read_state(root, name, task, recorder, parent)?;
            let repository =
                snapshot::workspace(root, saved["taskRecord"].get("repositories"), false)?;
            store.update(|state| {
                workers::reconcile(state, &observed, &input["targets"])?;
                let mut capacity = json!({});
                for field in [
                    "capacity",
                    "occupiedSlots",
                    "taskWorkers",
                    "allowed",
                    "sessionId",
                    "observedAt",
                    "source",
                ] {
                    capacity[field] = observed[field].clone();
                }
                let reusable = input["input"]["reuseWorker"].as_str().is_some_and(|id| {
                    observed["workers"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .any(|worker| {
                            worker["id"] == id
                                && worker["status"] == "idle"
                                && worker["occupiesSlot"] == true
                        })
                });
                let facts = json!({
                    "observations": {"host": {"name": observed["host"]}},
                    "allowed": observed["allowed"], "supported": true,
                    "independentTasks": 2, "reuseAvailable": reusable, "workers": capacity,
                });
                let saved = session::read_state(root, name, task, recorder, parent)?;
                let now = now_ms();
                let checked = actions::check(
                    &saved,
                    &json!({"id": input["input"]["offer"]}),
                    &Context {
                        settings: &settings,
                        repository: &repository,
                        status: &json!({}),
                        facts: &facts,
                        session: Some(name),
                        now,
                    },
                )?;
                let mut result = workers::admit(
                    state,
                    workers::Admission {
                        input: &input["input"],
                        checked: &checked,
                        snapshot: &observed,
                        settings: &settings,
                        session: name,
                        now,
                        at: &json!(timestamp::format(now)),
                        revision: &repository["head"],
                    },
                )?;
                if result["duplicate"] != true {
                    result["request"] = workers::transition(
                        state,
                        &input["input"]["id"],
                        &json!({"kind": "dispatching"}),
                    )?["request"]
                        .take();
                }
                result.as_object_mut().unwrap().shift_remove("state");
                Ok(result)
            })
        }
        "record" => {
            let request = store.update(|s| {
                let current = s["requests"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .find(|r| r["id"] == input["id"]);
                if input
                    .get("generation")
                    .is_some_and(|g| current.is_none_or(|r| r["generation"] != *g))
                {
                    return Ok(current.cloned().unwrap_or(Value::Null));
                }
                Ok(workers::transition(s, &input["id"], &input["result"])?["request"].take())
            })?;
            if input["result"]["kind"] == "observed" {
                // The recorded result stands; a failed offer update is recorded by its own
                // `state.updated` operation.
                let _ = session::update_state(root, name, task, recorder, parent, |state| {
                    let id = request["offer"].as_str().unwrap_or_default();
                    if let Some(offer) = state["recommendations"].get_mut(id) {
                        offer["execution"] = json!(if request["modelMatches"] == true {
                            "host-observed"
                        } else {
                            "host-observed-model-mismatch"
                        });
                    }
                    if workers::execution_known(&request) {
                        decisions::record_feature(
                            state,
                            &json!({"feature":"delegation","status":if request["modelMatches"]==true{"used"}else{"failed"},"evidence":request["evidence"],"reason":if request["modelMatches"]==true{"Host adapter observed the configured worker; main acceptance remains separate."}else{"Host adapter reported a worker model mismatch; its occupied slot remains counted."}}),
                            &request["source"],
                            &timestamp::format(now_ms()),
                            "runtime-observed",
                        )?;
                    }
                    Ok(())
                });
            }
            Ok(
                json!({"request":request,"executionObserved":workers::execution_known(&request),"modelMatches":request["modelMatches"]}),
            )
        }
        _ => Err("unknown worker phase".into()),
    }
}
pub fn run(input: &Value) -> Result<Value, Failure> {
    let root = input["root"]
        .as_str()
        .ok_or("worker operation requires root")?;
    let root = Path::new(root);
    let mut recorder = Recorder::open(root, input["session"].as_str(), None);
    let operation = recorder.start("worker.dispatch", None);
    let result = phase(input, root, &mut recorder, &operation);
    // Phase calls are transport boundaries; only admission and result recording are dispatch work.
    if input["phase"] == "admit" || input["phase"] == "record" {
        recorder.complete(
            operation,
            Map::from_iter([
                ("available".into(), json!(true)),
                (
                    "complete".into(),
                    json!(
                        result
                            .as_ref()
                            .is_ok_and(|r| r["executionObserved"] == true)
                    ),
                ),
            ]),
            result.as_ref().err().map(|_| "INVALID_INPUT"),
        );
    }
    let export = recorder.finish();
    Ok(match result {
        Ok(value) => json!({"value":value,"export":export}),
        Err(error) => json!({"error":error.message,"export":export}),
    })
}
