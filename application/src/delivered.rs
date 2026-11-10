//! The session's delivery record on disk: read to bound what hooks inject, updated with what
//! each hook delivered. Best effort: a busy or unreadable record never delays or blocks a
//! hook; the delivery goes out uncounted.
use crate::failure::Failure;
use crate::session::read_record;
use craft_engine::session::delivered;
use craft_platform::state;
use serde_json::Value;
use std::path::{Path, PathBuf};

const RECORD_BYTES: usize = 131_072;

fn file(root: &Path, session: &str, create: bool) -> Result<PathBuf, Failure> {
    Ok(state::file(root, "delivered.json", create, Some(session))?)
}

/// The session's record; `Null` when it has none.
pub fn read(root: &Path, session: &str) -> Result<Value, Failure> {
    let record = read_record(&file(root, session, false)?, RECORD_BYTES)?;
    delivered::validate(&record)?;
    Ok(record)
}

/// Replace the record with `change` of it, when that gives one. Returns the written record;
/// `None` when nothing changed or another hook holds the record.
pub fn update(
    root: &Path,
    session: &str,
    change: impl FnOnce(&Value) -> Option<Value>,
) -> Result<Option<Value>, Failure> {
    let target = file(root, session, true)?;
    let Some(_lock) = state::lock(&target)? else {
        return Ok(None);
    };
    let record = read_record(&target, RECORD_BYTES)?;
    delivered::validate(&record)?;
    let Some(next) = change(&record) else {
        return Ok(None);
    };
    state::write(&target, &next, RECORD_BYTES, false)?;
    Ok(Some(next))
}
