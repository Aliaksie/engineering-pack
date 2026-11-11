//! The stop summary: when a turn ends after changing files, what those changes introduced
//! (module dependencies the project does not declare, debt markers, anchor findings the
//! session was not told when the file was written) and the declared `review` items of their
//! scope, said once. The changes are read from the worktree, so a file written by a shell
//! command or a script counts like an edited one, and so does one the session committed
//! before the turn ended. The session remembers each changed file's content and the commit
//! it stopped on, so a stop reviews only files changed since the last one, and what it was
//! told, so the same finding never holds a stop twice.
use crate::failure::Failure;
use crate::session::read_record;
use craft_engine::session::delivered::{Checked, finding_key};
use craft_engine::{debt, hash, routing, standards};
use craft_platform::state;
use serde_json::{Map, Value, json};
use std::path::Path;

const RECORD_BYTES: usize = 128 * 1024;
/// How much `git status` and one file's diff may print; beyond, the stop stays quiet.
const STATUS_BYTES: usize = 256 * 1024;
const DIFF_BYTES: usize = 512 * 1024;
/// How large a changed file is read, and how many changed files one stop reviews.
const FILE_BYTES: u64 = 512 * 1024;
const MAX_FILES: usize = 64;
/// How many `HEAD` movements back the last stop's commit is looked for.
const REFLOG_ENTRIES: &str = "64";

fn record_file(root: &Path, session: &str, create: bool) -> Result<std::path::PathBuf, Failure> {
    Ok(state::file(
        root,
        "stop-review.json",
        create,
        Some(session),
    )?)
}

/// Project files that differ from `HEAD` (modified, added or untracked outside ignores), each
/// with whether Git tracks it; `None` without Git or when the listing overflows.
fn dirty(root: &Path) -> Option<Vec<(String, bool)>> {
    let output = craft_platform::process::git(
        root,
        &[
            "status",
            "--porcelain=v1",
            "-z",
            "--untracked-files=all",
            "--no-renames",
        ],
        STATUS_BYTES,
    )
    .ok()??;
    let mut files: Vec<(String, bool)> = output
        .split(|byte| *byte == 0)
        .filter_map(|entry| {
            let entry = std::str::from_utf8(entry).ok()?;
            let (code, path) = (entry.get(..2)?, entry.get(3..)?);
            // A deleted file adds nothing to review.
            (!code.contains('D') && !path.is_empty() && !routing::private_path(path))
                .then(|| (path.to_owned(), code != "??"))
        })
        .collect();
    files.sort();
    files.truncate(MAX_FILES);
    Some(files)
}

/// A regular file's bytes, `None` when absent, a link or too large.
fn bytes(root: &Path, file: &str) -> Option<Vec<u8>> {
    match std::fs::symlink_metadata(root.join(file)) {
        Ok(meta) if meta.is_file() && meta.len() <= FILE_BYTES => {
            std::fs::read(root.join(file)).ok()
        }
        _ => None,
    }
}

/// The commit `HEAD` names, `None` without Git or before the first commit.
fn head(root: &Path) -> Option<String> {
    let output = craft_platform::process::git(root, &["rev-parse", "--verify", "-q", "HEAD"], 256)
        .ok()??;
    let id = String::from_utf8(output).ok()?.trim().to_owned();
    (!id.is_empty()).then_some(id)
}

/// Files the session committed since `base`, the commit its last stop saw: only when every
/// movement of `HEAD` since then was a commit made here. After a pull, a checkout, a rebase
/// or a reset the difference holds other people's work, so nothing is taken from it.
fn committed(root: &Path, base: &str, current: &str) -> Vec<String> {
    let moves = craft_platform::process::git(
        root,
        &["reflog", "-n", REFLOG_ENTRIES, "--format=%H %gs", "HEAD"],
        STATUS_BYTES,
    );
    let Ok(Some(moves)) = moves else {
        return Vec::new();
    };
    let moves = String::from_utf8_lossy(&moves);
    let mut reached = false;
    for entry in moves.lines() {
        let (id, action) = entry.split_once(' ').unwrap_or((entry, ""));
        if id == base {
            reached = true;
            break;
        }
        if !action.starts_with("commit") {
            return Vec::new();
        }
    }
    if !reached {
        return Vec::new();
    }
    let range = format!("{base}..{current}");
    let listed = craft_platform::process::git(
        root,
        &[
            "diff",
            "--name-only",
            "-z",
            "--no-renames",
            "--diff-filter=d",
            &range,
        ],
        STATUS_BYTES,
    );
    let Ok(Some(listed)) = listed else {
        return Vec::new();
    };
    listed
        .split(|byte| *byte == 0)
        .filter_map(|path| std::str::from_utf8(path).ok())
        .filter(|path| !path.is_empty() && !routing::private_path(path))
        .map(str::to_owned)
        .collect()
}

/// One changed file as a stop sees it.
struct Dirty {
    file: String,
    tracked: bool,
    bytes: Vec<u8>,
    digest: String,
}

/// Each dirty file's content now, with its hash and whether Git tracks it.
fn observed(root: &Path) -> Option<Vec<Dirty>> {
    Some(
        dirty(root)?
            .into_iter()
            .filter_map(|(file, tracked)| {
                let bytes = bytes(root, &file)?;
                let digest = hash(&bytes);
                Some(Dirty {
                    file,
                    tracked,
                    bytes,
                    digest,
                })
            })
            .collect(),
    )
}

fn hashes(files: &[Dirty]) -> Value {
    Value::Object(
        files
            .iter()
            .map(|dirty| (dirty.file.clone(), json!(dirty.digest)))
            .collect::<Map<_, _>>(),
    )
}

/// Remember what the worktree already holds when a session starts, so the first stop reviews
/// only what this session changes. A resumed session keeps its record.
pub fn baseline(root: &Path, session: &str, settings: &Value) -> Result<(), Failure> {
    if !standards::stop_relevant(settings) {
        return Ok(());
    }
    let file = record_file(root, session, true)?;
    if file.exists() {
        return Ok(());
    }
    let Some(files) = observed(root) else {
        return Ok(());
    };
    let mut record = json!({"formatVersion": 1, "files": hashes(&files), "delivered": []});
    if let Some(head) = head(root) {
        record["head"] = json!(head);
    }
    Ok(state::write(&file, &record, RECORD_BYTES, false)?)
}

/// Debt markers `file` adds against the commit `base`; an untracked file adds every line.
fn markers(root: &Path, file: &str, tracked: bool, bytes: &[u8], base: &str) -> Vec<Value> {
    let diff = tracked
        .then(|| {
            craft_platform::process::git(
                root,
                &[
                    "diff",
                    base,
                    "-U0",
                    "--no-color",
                    "--no-ext-diff",
                    "--no-textconv",
                    "--",
                    file,
                ],
                DIFF_BYTES,
            )
        })
        .map(|output| output.ok().flatten());
    let (added, removed) = match diff {
        Some(Some(diff)) => debt::changes(&String::from_utf8_lossy(&diff)),
        // Too large to compare, or no commit to compare with: unchecked.
        Some(None) => return Vec::new(),
        None => (
            String::from_utf8_lossy(bytes)
                .lines()
                .enumerate()
                .map(|(index, line)| (index + 1, line.to_owned()))
                .collect(),
            Vec::new(),
        ),
    };
    debt::markers(&added, &removed)
}

/// The stop summary for this turn, or `None` when nothing changed since the last stop or the
/// session was told all of it. While the host continues a held stop (`stop_active`) nothing
/// is said or recorded: a stop is never held twice in a row, and what the continuation
/// changes is reviewed at the next stop. With it, each changed file as a scope the stop
/// checked, so a finding that is gone counts as fixed.
pub fn stop(
    root: &Path,
    session: &str,
    settings: &Value,
    intent: Option<&str>,
    stop_active: bool,
) -> Result<(Option<Value>, Vec<Checked>), Failure> {
    if stop_active || !standards::stop_relevant(settings) {
        return Ok((None, Vec::new()));
    }
    let Some(files) = observed(root) else {
        return Ok((None, Vec::new()));
    };
    let file = record_file(root, session, true)?;
    let record = read_record(&file, RECORD_BYTES)?;
    let delivered: Vec<String> = record["delivered"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|key| key.as_str().map(str::to_owned))
        .collect();
    // Work the session committed since its last stop is clean now, and still its change.
    let current = head(root);
    let since = record["head"].as_str().zip(current.as_deref());
    let landed: Vec<Dirty> = since
        .filter(|(base, current)| base != current)
        .map(|(base, current)| committed(root, base, current))
        .unwrap_or_default()
        .into_iter()
        .filter(|file| !files.iter().any(|dirty| dirty.file == *file))
        .take(MAX_FILES.saturating_sub(files.len()))
        .filter_map(|file| {
            let bytes = bytes(root, &file)?;
            let digest = hash(&bytes);
            Some(Dirty {
                file,
                tracked: true,
                bytes,
                digest,
            })
        })
        .collect();
    // Markers are new against the commit the last stop saw, whether or not they were
    // committed since; without one (an older record, no commit yet), against `HEAD`.
    let base = if landed.is_empty() {
        "HEAD"
    } else {
        since.map_or("HEAD", |(base, _)| base)
    };
    let changed: Vec<&Dirty> = files
        .iter()
        .chain(&landed)
        .filter(|dirty| record["files"][dirty.file.as_str()] != json!(dirty.digest))
        .collect();
    let mut next = json!({"formatVersion": 1, "files": hashes(&files), "delivered": delivered});
    if let Some(current) = &current {
        next["head"] = json!(current);
    }
    // Anchor findings the session heard when it edited the file are not said again.
    let told = crate::delivered::read(root, session).unwrap_or(Value::Null);
    let mut summary = None;
    let mut checked = Vec::new();
    if !changed.is_empty() {
        let mut findings = Vec::new();
        let markers_on = standards::debt_markers(settings);
        for Dirty {
            file,
            tracked,
            bytes,
            ..
        } in changed.iter().copied()
        {
            let from = findings.len();
            for verdict in crate::dependencies::verdicts(root, settings, file, bytes) {
                for finding in verdict["findings"].as_array().into_iter().flatten() {
                    let mut finding = finding.clone();
                    finding["file"] = json!(file);
                    finding["standard"] = verdict["standard"].clone();
                    findings.push(finding);
                }
            }
            if markers_on {
                for mut finding in markers(root, file, *tracked, bytes, base) {
                    finding["file"] = json!(file);
                    findings.push(finding);
                }
            }
            let mut keys: Vec<String> = findings[from..]
                .iter()
                .map(|finding| {
                    let kind = if finding.get("standard").is_some() {
                        'd'
                    } else {
                        'm'
                    };
                    finding_key(
                        kind,
                        &finding["standard"],
                        &finding["rule"],
                        &finding["message"],
                    )
                })
                .collect();
            // A file no edit hook saw (a shell command or a script wrote it) was never
            // checked against its anchors: the stop does it, and says what is new.
            let anchors = standards::file_checks(settings, file);
            if !anchors.is_empty() {
                let heard = &told["findings"]["open"][file.as_str()];
                let text = String::from_utf8_lossy(bytes);
                for verdict in standards::verdicts(&anchors, &text, Some(file)) {
                    for finding in verdict["findings"].as_array().into_iter().flatten() {
                        let key = finding_key(
                            'a',
                            &verdict["standard"],
                            &finding["rule"],
                            &finding["message"],
                        );
                        let known = heard
                            .as_array()
                            .is_some_and(|heard| heard.iter().any(|k| k == key.as_str()));
                        if !known {
                            let mut finding = finding.clone();
                            finding["file"] = json!(file);
                            finding["standard"] = verdict["standard"].clone();
                            findings.push(finding);
                        }
                        keys.push(key);
                    }
                }
            }
            // The stop looked at this file for dependency findings and, where they apply,
            // markers and anchor checks.
            let mut kinds = String::from("d");
            if markers_on {
                kinds.push('m');
            }
            if !anchors.is_empty() {
                kinds.push('a');
            }
            checked.push(Checked {
                scope: file.clone(),
                kinds,
                keys,
            });
        }
        let paths: Vec<Value> = changed.iter().map(|dirty| json!(dirty.file)).collect();
        let review = routing::scope(settings, &paths)
            .map(|scope| standards::reviewed(settings, intent.unwrap_or("implement"), &scope))
            .unwrap_or_default();
        if let Some(stop) = debt::stop(&findings, &review, &delivered) {
            next["delivered"] = json!(debt::delivered(&delivered, &stop.keys));
            summary = Some(stop.summary);
        }
    }
    if next != record {
        state::write(&file, &next, RECORD_BYTES, false)?;
    }
    Ok((summary, checked))
}
