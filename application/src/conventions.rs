//! The conventions scan over a project's root files: bounded reads, then the engine's
//! proposal. Read-only; the consumer copies what it accepts into the configuration.
use crate::failure::Failure;
use craft_engine::conventions::{self, Look, MAX_BYTES, Source};
use craft_platform::files::{ReadError, read_bounded};
use serde_json::{Value, json};
use std::path::Path;

/// What the scan found at `root`: `{proposals, declared, scanned, unread, accept}`.
pub(crate) fn proposed(root: &Path, settings: &Value) -> Result<Value, Failure> {
    let mut found: Vec<(&str, Option<String>)> = Vec::new();
    let mut unread = Vec::new();
    for (path, look) in conventions::candidates() {
        let file = root.join(path);
        // A link is not followed: the scan stays inside the project.
        let folder = look == Look::Folder;
        if !std::fs::symlink_metadata(&file).is_ok_and(|entry| {
            if folder {
                entry.is_dir()
            } else {
                entry.is_file()
            }
        }) {
            continue;
        }
        if look != Look::Text {
            found.push((path, None));
            continue;
        }
        match read_bounded(&file, MAX_BYTES) {
            Ok(bytes) => found.push((path, Some(String::from_utf8_lossy(&bytes).into_owned()))),
            Err(ReadError::Bounds | ReadError::Grew) => {
                unread.push(
                    json!({"file": path, "reason": format!("larger than {MAX_BYTES} bytes")}),
                );
                found.push((path, None));
            }
            Err(ReadError::Io(error)) => {
                unread.push(json!({"file": path, "reason": error.kind().to_string()}));
                found.push((path, None));
            }
        }
    }
    let sources: Vec<Source> = found
        .iter()
        .map(|(path, text)| Source {
            path,
            text: text.as_deref(),
        })
        .collect();
    let mut result = conventions::propose(settings, &sources)?;
    result["scanned"] = json!(found.iter().map(|(path, _)| *path).collect::<Vec<_>>());
    result["unread"] = json!(unread);
    result["accept"] = json!(
        "Copy the `standard` of each proposal you accept into `standards` in .craft/project.json. A proposal is inactive until then; this command writes nothing."
    );
    Ok(result)
}
