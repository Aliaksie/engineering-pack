//! Consumer-owned Claude display settings. Permissions are never written here.
use crate::{Error, documents, files, state};
use serde_json::{Value, json};
use std::{
    fs,
    io::ErrorKind,
    path::{Path, PathBuf},
};
const BYTES: usize = 65_536;

fn target(root: &Path, create: bool) -> Result<PathBuf, Error> {
    let root = state::canonical_root(root)?;
    let directory = root.join(".claude");
    if create {
        files::create_private_directory(&directory)?;
    }
    match fs::symlink_metadata(&directory) {
        Ok(meta) if !meta.is_dir() || meta.file_type().is_symlink() => {
            return Err("Claude settings directory must be a real directory".into());
        }
        Err(error) if error.kind() != ErrorKind::NotFound => return Err(error.into()),
        _ => {}
    }
    Ok(directory.join("settings.local.json"))
}

fn read(file: &Path) -> Result<Value, Error> {
    let value = match documents::input(file) {
        Ok(value) => value,
        Err(error) if error.io_kind() == Some(ErrorKind::NotFound) => json!({}),
        Err(error) => return Err(error.to_string().into()),
    };
    if !value.is_object() {
        return Err("Claude settings must be a JSON object".into());
    }
    Ok(value)
}

pub fn inspect(root: &Path) -> Result<Value, Error> {
    let file = target(root, false)?;
    let settings = read(&file)?;
    Ok(json!({"file":file,"statusLine":settings.get("statusLine")}))
}

/// Explicit application of the display field only. Refuse replacement of a custom display.
/// The lock serializes Craft writers; the snapshot check also detects intervening edits.
pub fn apply(root: &Path, display: &Value) -> Result<Value, Error> {
    let file = target(root, true)?;
    let Some(_lock) = state::lock(&file)? else {
        return Err(Error::busy("Claude settings writer is busy"));
    };
    let before = read(&file)?;
    if let Some(existing) = before.get("statusLine") {
        if existing == display {
            return Ok(json!({"status":"already-configured","file":file}));
        }
        return Err(
            "Claude already has a statusLine; merge the preview manually to preserve your display"
                .into(),
        );
    }
    let mut after = before.clone();
    after["statusLine"] = display.clone();
    if read(&file)? != before {
        return Err("Claude settings changed during setup; preview again".into());
    }
    state::write(&file, &after, BYTES, true)?;
    Ok(
        json!({"status":"configured-unverified","file":file,"next":"Run an ordinary Claude turn and verify the context display; configuration alone does not prove native dispatch."}),
    )
}
