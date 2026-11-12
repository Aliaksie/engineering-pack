//! The conventions scan over a project's root files: bounded reads, then the engine's
//! proposal. Read-only. What it finds is in effect while the project declares no standards;
//! the consumer copies what it keeps into the configuration.
use crate::failure::Failure;
use craft_engine::conventions::{self, Look, MAX_BYTES, Practice, Source};
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
    let proposed = proposed(root, settings, true)?;
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
/// `keep` lets the scan keep what it asked Git in the project's state.
pub(crate) fn proposed(root: &Path, settings: &Value, keep: bool) -> Result<Value, Failure> {
    let mut found: Vec<(String, Option<String>)> = Vec::new();
    let mut unread = Vec::new();
    let mut read = |path: String, found: &mut Vec<(String, Option<String>)>| match read_bounded(
        &root.join(&path),
        MAX_BYTES,
    ) {
        Ok(bytes) => found.push((path, Some(String::from_utf8_lossy(&bytes).into_owned()))),
        Err(ReadError::Bounds | ReadError::Grew) => {
            unread.push(json!({"file": path, "reason": format!("larger than {MAX_BYTES} bytes")}));
            found.push((path, None));
        }
        Err(ReadError::Io(error)) => {
            unread.push(json!({"file": path, "reason": error.kind().to_string()}));
            found.push((path, None));
        }
    };
    // A link is not followed: the scan stays inside the project.
    let kind = |path: &Path| std::fs::symlink_metadata(path).ok();
    for (path, look) in conventions::candidates() {
        let folder = look == Look::Folder;
        if !kind(&root.join(path)).is_some_and(|entry| {
            if folder {
                entry.is_dir()
            } else {
                entry.is_file()
            }
        }) {
            continue;
        }
        if look == Look::Text {
            read(path.to_string(), &mut found);
        } else {
            found.push((path.to_string(), None));
        }
    }
    // Root files known by their ending, such as a solution file: only their names.
    let endings = conventions::endings();
    found.extend(
        listed(root, ROOT_ENTRIES, |name| {
            endings.iter().any(|ending| name.ends_with(ending))
        })
        .into_iter()
        .map(|name| (name, None)),
    );
    // The newest numbered records of the first decision folder, for the format they keep.
    if let Some(folder) = conventions::record_folders()
        .iter()
        .find(|folder| found.iter().any(|(path, _)| path == *folder))
    {
        let records = listed(&root.join(folder), ROOT_ENTRIES, |name| {
            name.ends_with(".md") && name.starts_with(|first: char| first.is_ascii_digit())
        });
        let skipped = records.len().saturating_sub(conventions::MAX_RECORDS);
        for name in records.into_iter().skip(skipped) {
            read(format!("{folder}/{name}"), &mut found);
        }
    }
    let subjects = subjects(root, keep);
    let sources: Vec<Source> = found
        .iter()
        .map(|(path, text)| Source {
            path,
            text: text.as_deref(),
        })
        .collect();
    let practice = Practice {
        subjects: &subjects,
    };
    let mut result = conventions::propose(settings, &sources, &practice)?;
    result["scanned"] = json!(found.iter().map(|(path, _)| path).collect::<Vec<_>>());
    result["unread"] = json!(unread);
    result["accept"] = json!(
        "While the project declares no standards and `routing.conventionProposals` is on, every proposal is in effect. Standards declared in .craft/project.json replace them: copy the `standard` of each proposal to keep. This command writes nothing."
    );
    Ok(result)
}

/// How many entries of one folder the scan lists at most.
const ROOT_ENTRIES: usize = 512;

/// Names of the regular files directly in `folder` that `wanted` keeps, sorted; links are
/// not followed and at most `limit` entries are looked at.
fn listed(folder: &Path, limit: usize, wanted: impl Fn(&str) -> bool) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(folder)
        .into_iter()
        .flatten()
        .take(limit)
        .flatten()
        .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_file()))
        .filter_map(|entry| entry.file_name().into_string().ok())
        .filter(|name| wanted(name))
        .collect();
    names.sort();
    names
}

const SUBJECT_BYTES: usize = 200;
const PRACTICE_BYTES: usize = 16 * 1024;

/// What names the commit `root`'s checkout is at, read from Git's own files without starting
/// Git: the text of `HEAD` and, on a branch, of the branch's reference or the packed
/// references' size and time. `None` when `root` is not the top of a checkout.
fn head_key(root: &Path) -> Option<String> {
    let dot = root.join(".git");
    let directory = if dot.is_dir() {
        dot
    } else {
        // A linked worktree: `.git` is a file naming its directory.
        let text = String::from_utf8(read_bounded(&dot, 4096).ok()?).ok()?;
        root.join(text.strip_prefix("gitdir:")?.trim())
    };
    let head = String::from_utf8(read_bounded(&directory.join("HEAD"), 4096).ok()?).ok()?;
    let Some(reference) = head.strip_prefix("ref:").map(str::trim) else {
        return Some(head.trim().to_string());
    };
    // References live in the common directory, which a linked worktree names.
    let common = read_bounded(&directory.join("commondir"), 4096)
        .ok()
        .and_then(|bytes| String::from_utf8(bytes).ok())
        .map_or_else(|| directory.clone(), |path| directory.join(path.trim()));
    let loose = read_bounded(&common.join(reference), 4096)
        .ok()
        .and_then(|bytes| String::from_utf8(bytes).ok());
    let at = loose.unwrap_or_else(|| {
        std::fs::metadata(common.join("packed-refs")).map_or_else(
            |_| "unborn".to_string(),
            |packed| format!("packed {} {:?}", packed.len(), packed.modified().ok()),
        )
    });
    Some(format!("{reference} {}", at.trim()))
}

/// The subjects of the most recent commits, newest first, merges left out. Hooks resolve the
/// settings on every event, so with `keep` Git is asked once per commit the checkout is at
/// and the answer kept in the project's state. A kept answer is read either way.
fn subjects(root: &Path, keep: bool) -> Vec<String> {
    let Some(head) = head_key(root) else {
        return Vec::new();
    };
    let target = state::file(root, "notices/practice.json", false, None).ok();
    let kept = target
        .as_deref()
        .and_then(|file| crate::session::read_record(file, PRACTICE_BYTES).ok());
    if let Some(kept) = kept.filter(|kept| kept["formatVersion"] == 1 && kept["head"] == head) {
        return kept["subjects"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|subject| subject.as_str().map(str::to_owned))
            .collect();
    }
    let count = conventions::MAX_SUBJECTS.to_string();
    let log = craft_platform::process::git(
        root,
        &["log", "-n", &count, "--no-merges", "--format=%s"],
        PRACTICE_BYTES,
    );
    let Ok(Some(output)) = log else {
        return Vec::new();
    };
    let subjects: Vec<String> = String::from_utf8_lossy(&output)
        .lines()
        .map(|line| line.chars().take(SUBJECT_BYTES).collect())
        .collect();
    // Kept when the state can be written; a busy or missing state costs one Git start more.
    if keep
        && let Ok(target) = state::file(root, "notices/practice.json", true, None)
        && let Ok(Some(_lock)) = state::lock(&target)
    {
        let record = json!({"formatVersion": 1, "head": head, "subjects": subjects});
        let _ = state::write(&target, &record, PRACTICE_BYTES, false);
    }
    subjects
}
