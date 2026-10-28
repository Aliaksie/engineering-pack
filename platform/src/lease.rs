//! Locks whose owner is a calling host process. There is no age-based takeover: an
//! unknown/live pid, malformed record or legacy ownerless lock always remains busy.
use crate::Error;
use crate::{process::Processes, state};
use craft_engine::values::sha256;
use craft_engine::{scratch::Liveness, values::safe_integer};
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
        && sha256(&value["lease"])
        && value.as_object().is_some_and(|v| v.len() == 4)
}
/// Called by ordinary state writers as well, so task-start recovers dead host fences.
/// The guard is a workspace lock on the lock primitive, so it never recovers through here.
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    /// A pid no process has.
    const DEAD: i64 = 2_147_483_647;

    struct Fixture(PathBuf);
    impl Fixture {
        fn new() -> Self {
            static NEXT: AtomicU64 = AtomicU64::new(0);
            let path = std::env::temp_dir().join(format!(
                "craft-lease-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir(&path).unwrap();
            Self(path)
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn lease(pid: i64) -> String {
        json!({"owner":OWNER,"formatVersion":1,"pid":pid,"lease":craft_engine::hash(b"lease")})
            .to_string()
    }

    fn suffixed(file: &Path, suffix: &str) -> PathBuf {
        let mut name = file.as_os_str().to_owned();
        name.push(suffix);
        name.into()
    }

    #[test]
    fn only_a_dead_hosts_lease_is_reclaimed_and_only_by_the_record_lock() {
        let fixture = Fixture::new();
        let file = fixture.0.join("task.json");
        let kept = [
            String::new(),
            "{}".into(),
            lease(DEAD).replace(OWNER, "other"),
            lease(i64::from(std::process::id())),
        ];
        for content in kept {
            fs::write(lock_path(&file), &content).unwrap();
            assert!(state::lock(&file).unwrap().is_none(), "{content}");
            assert_eq!(fs::read_to_string(lock_path(&file)).unwrap(), content);
        }
        fs::write(lock_path(&file), lease(DEAD)).unwrap();
        assert!(
            state::ticket(&file).unwrap().is_none(),
            "the primitive never recovers"
        );
        assert!(scratch_lock(&file).is_none(), "nor does a workspace lock");
        assert_eq!(fs::read_to_string(lock_path(&file)).unwrap(), lease(DEAD));
        let held = state::lock(&file).unwrap().expect("dead lease reclaimed");
        drop(held);
        assert!(!lock_path(&file).exists());
        assert!(!suffixed(&file, ".lease-guard.lock").exists());
    }

    fn scratch_lock(file: &Path) -> Option<state::Lock> {
        crate::scratch::lock(file).unwrap()
    }

    #[test]
    fn a_guard_left_by_a_dead_writer_is_recovered_and_a_live_one_is_busy() {
        let fixture = Fixture::new();
        let file = fixture.0.join("task.json");
        let guard = suffixed(&file, ".lease-guard.lock");
        let holder = |pid: i64| {
            json!({"owner":"craft-workspace-lock","formatVersion":1,"pid":pid}).to_string()
        };
        fs::write(lock_path(&file), lease(DEAD)).unwrap();
        fs::write(&guard, holder(i64::from(std::process::id()))).unwrap();
        assert_eq!(
            state::lock(&file).unwrap_err().kind(),
            Some(std::io::ErrorKind::ResourceBusy)
        );
        assert_eq!(fs::read_to_string(lock_path(&file)).unwrap(), lease(DEAD));
        fs::write(&guard, holder(DEAD)).unwrap();
        assert!(state::lock(&file).unwrap().is_some());
        assert!(!guard.exists());
    }

    #[test]
    fn an_acquired_lease_outlives_its_ticket_until_released() {
        let fixture = Fixture::new();
        let file = fixture.0.join("task.json");
        let ticket = acquire(&file, i64::from(std::process::id())).unwrap();
        assert_eq!(read(&file).unwrap().as_ref(), Some(&ticket));
        assert!(
            state::lock(&file).unwrap().is_none(),
            "a live lease is busy"
        );
        assert_eq!(with(&file, &ticket, || Ok::<_, Error>(1)).unwrap(), 1);
        let other = json!({"lease": craft_engine::hash(b"other")});
        assert!(release(&file, &other).is_err(), "only the owner releases");
        release(&file, &ticket).unwrap();
        assert!(state::lock(&file).unwrap().is_some());
    }

    #[test]
    fn only_written_lease_shapes_are_recognized() {
        let lease = |token: String| json!({"owner":OWNER,"formatVersion":1,"pid":1,"lease":token});
        let token = craft_engine::hash(b"lease");
        assert!(recognized(&lease(token.clone())));
        assert!(!recognized(&lease(token.to_uppercase())));
        assert!(!recognized(&lease(token[1..].into())));
    }
}
