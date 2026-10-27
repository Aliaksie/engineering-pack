//! Native ownership phases for a host's asynchronous transport lane. A host pid owns
//! each retained fence; short native calls validate the opaque ticket before I/O.
use crate::{failure::Failure, session};
use craft_engine::{control, digest, session as decisions, values, values::safe_integer};
use craft_platform::{lease, repository::snapshot, state};
use serde_json::{Value, json};
use std::path::Path;
const BYTES: usize = 262144;
fn admission(root: &Path, name: &str, task: &str, source: Option<&Value>) -> Result<(), Failure> {
    if session::active_task(root, Some(name))
        .map_err(|e| e.message)?
        .as_deref()
        != Some(task)
    {
        return Err("native effect requires the active task binding".into());
    }
    if let Some(source) = source {
        let saved = session::read_record(&session::state_file(root, name, task, false)?, 131072)?;
        let repository = snapshot::workspace(root, saved["taskRecord"].get("repositories"), false)?;
        if !source.as_str().is_some_and(|s| s.len() == 64)
            || decisions::source(&repository) != *source
        {
            return Err("native source changed after admission".into());
        }
    }
    Ok(())
}
pub fn run(input: &Value) -> Result<Value, Failure> {
    let phase = input["phase"]
        .as_str()
        .ok_or("native control phase required")?;
    if phase == "support" {
        return Ok(control::support(
            input["host"].as_str().ok_or("unknown host adapter")?,
        )?);
    }
    let root = state::canonical_root(Path::new(
        input["root"]
            .as_str()
            .ok_or("native control root required")?,
    ))?;
    let name = input["session"]
        .as_str()
        .ok_or("native control session required")?;
    let task = input["task"]
        .as_str()
        .ok_or("native control task required")?;
    decisions::identity(&input["session"], &input["task"])?;
    if matches!(
        phase,
        "source" | "admission-acquire" | "admission-check" | "admission-release"
    ) {
        let file = session::binding_file(&root, name, true).map_err(|e| e.message)?;
        if phase == "source" {
            admission(&root, name, task, input.get("source"))?;
            return Ok(Value::Null);
        }
        if phase == "admission-acquire" {
            let ticket = lease::acquire(
                &file,
                safe_integer(&input["pid"]).ok_or("host pid required")?,
            )?;
            if let Err(error) = admission(&root, name, task, input.get("source")) {
                lease::release(&file, &ticket)?;
                return Err(error);
            }
            return Ok(ticket);
        }
        if phase == "admission-release" {
            lease::release(&file, &input["ticket"])?;
        } else {
            lease::with(&file, &input["ticket"], || {
                admission(&root, name, task, input.get("source"))
            })?;
        }
        return Ok(Value::Null);
    }
    let host = input["host"]
        .as_str()
        .filter(|_| values::text(&input["host"]))
        .ok_or("native host required")?;
    let coordinator = input["coordinator"]
        .as_str()
        .filter(|_| values::text(&input["coordinator"]))
        .ok_or("native coordinator required")?;
    let tree = digest(&json!([root.to_string_lossy(), name, task]));
    let context = json!({"root":root.to_string_lossy(),"session":name,"task":task,"tree":tree});
    let filename = format!(
        "native-control-{}.json",
        &digest(&json!([host, tree]))[..24]
    );
    let file = state::file(&root, &filename, true, Some(name))?;
    let validate = |journal: &Value| {
        control::validate(journal, &json!(tree), &json!(host), &json!(coordinator))
    };
    let read = || -> Result<Value, Failure> {
        let saved = session::read_record(&file, BYTES).map_err(|error| error.message)?;
        let journal = if saved.is_null() {
            control::empty(&tree, host, coordinator)
        } else {
            saved
        };
        validate(&journal)?;
        Ok(journal)
    };
    if phase == "open" {
        let ticket = lease::acquire(
            &file,
            safe_integer(&input["pid"]).ok_or("host pid required")?,
        )?;
        return match read() {
            Ok(journal) => Ok(
                json!({"context":context,"ticket":ticket,"revision":digest(&journal),"journal":journal}),
            ),
            Err(error) => {
                lease::release(&file, &ticket)?;
                Err(error)
            }
        };
    }
    if phase == "close" {
        lease::release(&file, &input["ticket"])?;
        return Ok(Value::Null);
    }
    lease::with(&file, &input["ticket"], || {
        let mut journal = read()?;
        if input["expected"] != digest(&journal) {
            return Err("native journal changed; reconcile retained ownership".into());
        }
        match phase {
            "begin" => {
                let mut operation = input["input"].clone();
                if input["kind"] == "release" && operation.get("releaseWorker").is_some() {
                    operation["key"] = json!(digest(&json!([
                        tree,
                        "release",
                        operation["releaseWorker"]
                    ])));
                }
                control::begin(&mut journal, &operation, &input["kind"], &input["nativeId"])?;
            }
            "persist" => {
                validate(&input["journal"])?;
                control::replacement(&journal, &input["journal"], input["withdrawn"].as_str())?;
                journal = input["journal"].clone();
            }
            _ => return Err("unknown native control phase".into()),
        }
        validate(&journal)?;
        state::write(&file, &journal, BYTES, false)?;
        Ok(json!({"revision":digest(&journal),"journal":journal}))
    })
}
