//! The conventions scan over a project's root files: bounded reads, then the engine's
//! proposal. Read-only. What it finds is in effect while the project declares no standards;
//! the consumer copies what it keeps into the configuration.
use crate::failure::Failure;
use craft_engine::conventions::{self, Look, MAX_BYTES, Source};
use craft_platform::files::{ReadError, read_bounded};
use craft_platform::state;
use serde_json::{Value, json};
use std::path::Path;

const RECORD_BYTES: usize = 1024;

/// What a session start says about an undeclared project's conventions, once per set of
/// proposals: the offer with the command that lists them, recorded as shown. `None` when
/// there is nothing new to say or another hook is recording it.
pub(crate) fn offer(root: &Path) -> Result<Option<Value>, Failure> {
    let settings = &crate::project::declared_settings(root)?;
    // Standards inherited from an organization or user layer are not the project's own:
    // only then is its file read again, for what it declares itself.
    let listed = |value: &Value| {
        value["standards"]
            .as_array()
            .is_some_and(|standards| !standards.is_empty())
    };
    let project = match listed(settings) {
        true => crate::project::configuration(root)?,
        false => json!({}),
    };
    if listed(&project) {
        return Ok(None);
    }
    let proposed = proposed(root, settings)?;
    let target = state::file(root, "notices/conventions.json", true, None)?;
    let Some(_lock) = state::lock(&target)? else {
        return Ok(None);
    };
    let shown = crate::session::read_record(&target, RECORD_BYTES).unwrap_or(Value::Null);
    let Some((mut offer, record)) = conventions::offer(&project, &proposed, &shown) else {
        return Ok(None);
    };
    state::write(&target, &record, RECORD_BYTES, false)?;
    // This hook's own executable is the runtime that lists the proposals.
    if let Some(runtime) = std::env::current_exe()
        .ok()
        .as_deref()
        .and_then(Path::to_str)
    {
        offer["run"] = json!([
            runtime,
            "standards",
            "--root",
            root.to_string_lossy(),
            "--propose"
        ]);
    }
    Ok(Some(offer))
}

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
        "While the project declares no standards and `routing.conventionProposals` is on, every proposal is in effect. Standards declared in .craft/project.json replace them: copy the `standard` of each proposal to keep. This command writes nothing."
    );
    Ok(result)
}
