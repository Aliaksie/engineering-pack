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
/// `effective` are the settings the hook runs with: a set that was not applied to them is
/// not said to be in effect.
pub(crate) fn offer(root: &Path, effective: &Value) -> Result<Option<Value>, Failure> {
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
    let applied = |id: &Value| {
        effective["standards"]
            .as_array()
            .is_some_and(|standards| standards.iter().any(|standard| standard["id"] == *id))
    };
    if !offer["standards"]
        .as_array()
        .is_some_and(|ids| ids.iter().all(applied))
    {
        return Ok(None);
    }
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
/// `keep` lets the scan keep what it asked Git in the project's state. A root that is no
/// repository and declares nothing is a workspace: its member repositories are scanned too,
/// each one's standards kept to its folder.
pub(crate) fn proposed(root: &Path, settings: &Value, keep: bool) -> Result<Value, Failure> {
    let own = scan(root, root, None, settings, keep)?;
    let entry = |name: &str| std::fs::symlink_metadata(root.join(name)).is_ok();
    if entry(".git") || entry(".craft/project.json") {
        return Ok(own);
    }
    let members: Vec<(String, Value)> = repositories(root)
        .into_iter()
        .filter_map(|member| {
            let result = scan(&root.join(&member), root, Some(&member), settings, keep).ok()?;
            Some((member, result))
        })
        .collect();
    if members.is_empty() {
        return Ok(own);
    }
    let scanned: Vec<Value> = own["scanned"]
        .as_array()
        .into_iter()
        .flatten()
        .cloned()
        .chain(members.iter().flat_map(|(member, result)| {
            result["scanned"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
                .map(move |path| json!(format!("{member}/{path}")))
        }))
        .collect();
    let named: Vec<(&str, Value)> = members
        .iter()
        .map(|(member, result)| (member.as_str(), result.clone()))
        .collect();
    let mut result = conventions::workspace(settings, own, &named)?;
    result["scanned"] = json!(scanned);
    Ok(result)
}

/// The member repositories of a workspace root: its direct folders that hold a Git entry,
/// by name; links are not followed.
fn repositories(root: &Path) -> Vec<String> {
    let mut members: Vec<String> = std::fs::read_dir(root)
        .into_iter()
        .flatten()
        .take(ROOT_ENTRIES)
        .flatten()
        .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_dir()))
        .filter(|entry| std::fs::symlink_metadata(entry.path().join(".git")).is_ok())
        .filter_map(|entry| entry.file_name().into_string().ok())
        .collect();
    members.sort();
    members.truncate(conventions::MAX_MEMBERS);
    members
}

/// The scan of one directory. `home` is the project whose state keeps what Git was asked,
/// under `member`'s name when `root` is a member of a workspace.
fn scan(
    root: &Path,
    home: &Path,
    member: Option<&str>,
    settings: &Value,
    keep: bool,
) -> Result<Value, Failure> {
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
    let subjects = subjects(root, home, member, keep);
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
    // A reference is a name beneath `refs/`; anything else is not followed.
    if !reference.starts_with("refs/") || reference.split('/').any(|part| part == "..") {
        return None;
    }
    // References live in the common directory, which a linked worktree names.
    let common = read_bounded(&directory.join("commondir"), 4096)
        .ok()
        .and_then(|bytes| String::from_utf8(bytes).ok())
        .map_or_else(|| directory.clone(), |path| directory.join(path.trim()));
    let loose = read_bounded(&common.join(reference), 4096)
        .ok()
        .and_then(|bytes| String::from_utf8(bytes).ok());
    // A branch without a commit has no history to read, and nothing kept is taken for it.
    let at = match loose {
        Some(at) => at,
        None => {
            let packed = std::fs::metadata(common.join("packed-refs")).ok()?;
            format!("packed {} {:?}", packed.len(), packed.modified().ok())
        }
    };
    Some(format!("{reference} {}", at.trim()))
}

/// The subjects of the most recent commits, newest first, merges left out. Hooks resolve the
/// settings on every event, so with `keep` Git is asked once per commit the checkout is at
/// and the answer kept in the project's state. A kept answer is read either way.
fn subjects(root: &Path, home: &Path, member: Option<&str>, keep: bool) -> Vec<String> {
    let Some(head) = head_key(root) else {
        return Vec::new();
    };
    // One record per member of a workspace, named by a digest of the member's folder.
    let name = match member {
        Some(member) => format!(
            "notices/practice-{}.json",
            &craft_engine::hash(member.as_bytes())[..12]
        ),
        None => "notices/practice.json".to_string(),
    };
    let target = state::file(home, &name, false, None).ok();
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
        .map(|line| {
            let mut end = line.len().min(SUBJECT_BYTES);
            while !line.is_char_boundary(end) {
                end -= 1;
            }
            line[..end].to_string()
        })
        .collect();
    // Kept when the state can be written; a busy or missing state costs one Git start more.
    if keep
        && let Ok(target) = state::file(home, &name, true, None)
        && let Ok(Some(_lock)) = state::lock(&target)
    {
        let record = json!({"formatVersion": 1, "head": head, "subjects": subjects});
        let _ = state::write(&target, &record, PRACTICE_BYTES, false);
    }
    subjects
}
