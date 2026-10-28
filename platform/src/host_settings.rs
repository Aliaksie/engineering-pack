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

// Highest-precedence persistent consumer settings first. Session flags and managed
// host policy cannot be inferred by this standalone setup command.
fn snapshot(file: &Path) -> Result<Vec<(PathBuf, Value)>, Error> {
    let directory = file.parent().expect("settings directory");
    let mut paths = vec![file.to_path_buf(), directory.join("settings.json")];
    let user_directory = std::env::var_os("CLAUDE_CONFIG_DIR")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .or_else(|| std::env::home_dir().map(|home| home.join(".claude")));
    if let Some(user_directory) = user_directory {
        paths.push(user_directory.join("settings.json"));
    }
    paths
        .into_iter()
        .map(|path| {
            let settings = read(&path)?;
            Ok((path, settings))
        })
        .collect()
}

fn effective_display(settings: &[(PathBuf, Value)]) -> Option<(&Path, &Value)> {
    settings.iter().find_map(|(path, value)| {
        value
            .get("statusLine")
            .map(|display| (path.as_path(), display))
    })
}

pub fn inspect(root: &Path) -> Result<Value, Error> {
    let file = target(root, false)?;
    let settings = snapshot(&file)?;
    let current = effective_display(&settings);
    Ok(
        json!({"file":file,"statusLine":current.map(|(_, value)| value),
        "source":current.map(|(path, _)| path)}),
    )
}

/// Explicit application of the display field only. Refuse replacement of a custom display.
/// The lock serializes Craft writers; the snapshot check also detects intervening edits.
pub fn apply(root: &Path, display: &Value) -> Result<Value, Error> {
    let file = target(root, true)?;
    let Some(_lock) = state::lock(&file)? else {
        return Err(Error::busy("Claude settings writer is busy"));
    };
    let before = snapshot(&file)?;
    if let Some((_, existing)) = effective_display(&before) {
        if existing == display {
            return Ok(json!({"status":"already-configured","file":file}));
        }
        return Err(
            "Claude already has a statusLine; merge the preview manually to preserve your display"
                .into(),
        );
    }
    let mut after = before[0].1.clone();
    after["statusLine"] = display.clone();
    if snapshot(&file)? != before {
        return Err("Claude settings changed during setup; preview again".into());
    }
    state::write(&file, &after, BYTES, true)?;
    Ok(
        json!({"status":"configured-unverified","file":file,"next":"Run an ordinary Claude turn and verify the context display; configuration alone does not prove native dispatch."}),
    )
}
