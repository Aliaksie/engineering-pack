//! The stop summary: when a turn ends after changing files, what those changes introduced
//! against `HEAD` (module dependencies the project does not declare, debt markers) and the
//! declared `review` items of their scope, said once. The session remembers each changed
//! file's content, so a stop reviews only files changed since the last one, and what it was
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

/// One dirty file as a stop sees it.
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
    let record = json!({"formatVersion": 1, "files": hashes(&files), "delivered": []});
    Ok(state::write(&file, &record, RECORD_BYTES, false)?)
}

/// Debt markers `file` adds against `HEAD`; an untracked file adds every line.
fn markers(root: &Path, file: &str, tracked: bool, bytes: &[u8]) -> Vec<Value> {
    let diff = tracked
        .then(|| {
            craft_platform::process::git(
                root,
                &[
                    "diff",
                    "HEAD",
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
        // Too large to compare, or no `HEAD` to compare with: unchecked.
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
    let changed: Vec<&Dirty> = files
        .iter()
        .filter(|dirty| record["files"][dirty.file.as_str()] != json!(dirty.digest))
        .collect();
    let mut next = json!({"formatVersion": 1, "files": hashes(&files), "delivered": delivered});
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
                for mut finding in markers(root, file, *tracked, bytes) {
                    finding["file"] = json!(file);
                    findings.push(finding);
                }
            }
            // The stop looked at this file for dependency findings and, when on, markers.
            checked.push(Checked {
                scope: file.clone(),
                kinds: if markers_on { "dm" } else { "d" }.into(),
                keys: findings[from..]
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
                    .collect(),
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
