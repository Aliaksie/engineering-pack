//! Locks whose owner is a calling host process. There is no age-based takeover: an
//! unknown/live pid, malformed record or legacy ownerless lock always remains busy.
use crate::Error;
use crate::{scratch::Processes, state};
use craft_engine::{contracts::safe_integer, scratch::Liveness};
use serde_json::{Value, json};
use std::{
    fs,
    path::{Path, PathBuf},
};
const OWNER: &str = "craft-native-lease";
const BYTES: usize = 4096;
fn lock_path(file: &Path) -> PathBuf {
    let mut name = file.as_os_str().to_owned();
    name.push(".lock");
    name.into()
}
fn guard(file: &Path) -> Result<state::Lock, Error> {
    let mut name = file.as_os_str().to_owned();
    name.push(".lease-guard");
    crate::scratch::lock(Path::new(&name))?
        .ok_or_else(|| Error::busy("native lease writer is busy"))
}
fn read(file: &Path) -> Result<Option<Value>, Error> {
    state::read(&lock_path(file), BYTES)
}
fn recognized(value: &Value) -> bool {
    value["owner"] == OWNER
        && value["formatVersion"] == 1
        && safe_integer(&value["pid"]).is_some_and(|pid| pid > 0 && pid <= u32::MAX.into())
        && value["lease"]
            .as_str()
            .is_some_and(|s| s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit()))
        && value.as_object().is_some_and(|v| v.len() == 4)
}
/// Called by ordinary state writers as well, so task-start recovers dead host fences.
/// Only recognized leases enter the guard protocol (scratch locks cannot recurse here).
pub(crate) fn reclaim(file: &Path) -> Result<bool, Error> {
    if !read(file).ok().flatten().as_ref().is_some_and(recognized) {
        return Ok(false);
    }
    let _guard = guard(file)?;
    let Some(owner) = read(file)? else {
        return Ok(true);
    };
    if !recognized(&owner)
        || Processes::default().liveness(safe_integer(&owner["pid"]).unwrap()) != Liveness::Dead
    {
        return Ok(false);
    }
    fs::remove_file(lock_path(file))?;
    Ok(true)
}
pub fn acquire(file: &Path, pid: i64) -> Result<Value, Error> {
    if pid < 1 || pid > u32::MAX.into() || Processes::default().liveness(pid) != Liveness::Live {
        return Err("native lease requires a live host pid".into());
    }
    let Some(mut lock) = state::lock(file)? else {
        return Err(Error::busy(
            "native task control already has an owner or active task writer is busy",
        ));
    };
    let mut random = [0; 32];
    getrandom::fill(&mut random).map_err(|e| Error::from(e.to_string()))?;
    let token = craft_engine::hash(&random);
    let owner = json!({"owner":OWNER,"formatVersion":1,"pid":pid,"lease":token});
    lock.record(owner.to_string().as_bytes())?;
    lock.retain();
    Ok(owner)
}
fn matches(file: &Path, ticket: &Value) -> Result<(), Error> {
    let owner = read(file)?.ok_or("native lease is unavailable")?;
    if !recognized(ticket)
        || owner != *ticket
        || Processes::default().liveness(safe_integer(&ticket["pid"]).unwrap()) != Liveness::Live
    {
        return Err("native lease ownership changed or is unavailable".into());
    }
    Ok(())
}
/// Serialize validation and a bounded read/write against release or dead-owner recovery.
pub fn with<T, E: From<Error>>(
    file: &Path,
    ticket: &Value,
    effect: impl FnOnce() -> Result<T, E>,
) -> Result<T, E> {
    let _guard = guard(file)?;
    matches(file, ticket)?;
    effect()
}
pub fn release(file: &Path, ticket: &Value) -> Result<(), Error> {
    with(file, ticket, || {
        fs::remove_file(lock_path(file)).map_err(Error::from)
    })
}
