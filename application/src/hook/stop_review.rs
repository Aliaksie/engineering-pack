//! The stop summary: when a turn ends after changing files, what those changes introduced
//! (module dependencies the project does not declare, debt markers, anchor findings the
//! session was not told when the file was written) and the declared `review` items of their
//! scope, said once. The changes are read from the worktree, so a file written by a shell
//! command or a script counts like an edited one, and so does one the session committed
//! before the turn ended. A commit counts as the session's own only when a shell command the
//! session ran made it: another session working in the same checkout, or the user committing
//! by hand, is not this session's change. The session remembers each changed file's content
//! and the commit it stopped on, so a stop reviews only files changed since the last one, and
//! what it was told, so the same finding never holds a stop twice. At a workspace root, a folder that is
//! no repository itself, each member repository is read the same way and its files are named
//! from the workspace root.
use crate::failure::Failure;
use crate::session::read_record;
use craft_engine::session::delivered::{Checked, finding_key};
use craft_engine::{debt, hash, routing, standards};
use craft_platform::state;
use serde_json::{Map, Value, json};
use std::path::{Path, PathBuf};

const RECORD_BYTES: usize = 128 * 1024;
/// How much `git status` and one file's diff may print; beyond, the stop stays quiet.
const STATUS_BYTES: usize = 256 * 1024;
const DIFF_BYTES: usize = 512 * 1024;
/// How large a changed file is read, and how many changed files one stop reviews.
const FILE_BYTES: u64 = 512 * 1024;
const MAX_FILES: usize = 64;
/// How many `HEAD` movements back the last stop's commit is looked for, and how many of its
/// own commits a session remembers.
const REFLOG_ENTRIES: &str = "64";
const OWN_COMMITS: usize = 64;

fn record_file(root: &Path, session: &str, create: bool) -> Result<std::path::PathBuf, Failure> {
    Ok(state::file(
        root,
        "stop-review.json",
        create,
        Some(session),
    )?)
}

/// One Git checkout a stop reads: the project's own, or a member repository of a workspace.
pub(crate) struct Checkout {
    pub(crate) directory: PathBuf,
    /// The member's folder as files are named from the workspace root (`api/`); empty for
    /// the project's own checkout.
    pub(crate) folder: String,
    /// Where `directory` lies beneath the top of its checkout, as Git prints it (`web/` or
    /// empty). Git names changed files from the top; a project whose root is a folder below
    /// it reads them from its own root.
    pub(crate) prefix: String,
}

/// The checkouts `root` is reviewed from: its own when Git knows it, otherwise its member
/// repositories; none for a plain folder.
pub(crate) fn checkouts(root: &Path) -> Vec<Checkout> {
    let own = |prefix| Checkout {
        directory: root.to_owned(),
        folder: String::new(),
        prefix,
    };
    if std::fs::symlink_metadata(root.join(".git")).is_ok() {
        return vec![own(String::new())];
    }
    // Asked only when `root` is not the top itself.
    let below = craft_platform::process::git(root, &["rev-parse", "--show-prefix"], 4096)
        .ok()
        .flatten()
        .and_then(|output| String::from_utf8(output).ok());
    if let Some(prefix) = below {
        return vec![own(prefix.trim_end().to_owned())];
    }
    crate::conventions::repositories(root)
        .into_iter()
        .map(|member| Checkout {
            directory: root.join(&member),
            folder: format!("{member}/"),
            prefix: String::new(),
        })
        .collect()
}

/// A checkout's files that differ from `HEAD` (modified, added or untracked outside
/// ignores), named from the project root, each with whether Git tracks it; `None` without
/// Git or when the listing overflows.
fn dirty(checkout: &Checkout) -> Option<Vec<(String, bool)>> {
    let output = craft_platform::process::git(
        &checkout.directory,
        &[
            "status",
            "--porcelain=v1",
            "-z",
            "--untracked-files=all",
            "--no-renames",
            "--",
            ".",
        ],
        STATUS_BYTES,
    )
    .ok()??;
    Some(
        output
            .split(|byte| *byte == 0)
            .filter_map(|entry| {
                let entry = std::str::from_utf8(entry).ok()?;
                let (code, path) = (
                    entry.get(..2)?,
                    entry.get(3..)?.strip_prefix(&*checkout.prefix)?,
                );
                // A deleted file adds nothing to review.
                (!code.contains('D') && !path.is_empty() && !routing::private_path(path))
                    .then(|| (format!("{}{path}", checkout.folder), code != "??"))
            })
            .collect(),
    )
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
pub(crate) fn head(root: &Path) -> Option<String> {
    let output = craft_platform::process::git(root, &["rev-parse", "--verify", "-q", "HEAD"], 256)
        .ok()??;
    let id = String::from_utf8(output).ok()?.trim().to_owned();
    (!id.is_empty()).then_some(id)
}

/// Files the session committed since `base`, the commit its last stop saw: only when every
/// movement of `HEAD` since then was a commit. After a pull, a checkout, a rebase or a reset
/// the difference holds other people's work, so nothing is taken from it. Of the commits, only
/// those in `own` (made by a command this session ran) count, and a file another commit in
/// the range also changed is left out: its markers could be someone else's.
fn committed(root: &Path, base: &str, current: &str, own: &[&str]) -> Vec<String> {
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
        // A merge concluded by hand is a commit too, and brings other people's work.
        let kinds = ["commit:", "commit (initial):", "commit (amend):"];
        if !kinds.iter().any(|kind| action.starts_with(kind)) {
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
            "-c",
            "core.quotePath=false",
            "log",
            "--format=%x01%H",
            "--name-only",
            "--no-renames",
            "--diff-filter=d",
            // Only the project's files, named from its root.
            "--relative",
            &range,
        ],
        STATUS_BYTES,
    );
    let Ok(Some(listed)) = listed else {
        return Vec::new();
    };
    let listed = String::from_utf8_lossy(&listed);
    let (mut ours, mut theirs): (Vec<&str>, Vec<&str>) = (Vec::new(), Vec::new());
    for commit in listed.split('\u{1}').filter(|c| !c.is_empty()) {
        let mut lines = commit.lines();
        let id = lines.next().unwrap_or_default().trim();
        let files = lines.filter(|path| {
            // A name Git still quotes holds a newline or a quote: no file to read.
            !path.is_empty() && !path.starts_with('"') && !routing::private_path(path)
        });
        if own.contains(&id) {
            ours.extend(files);
        } else {
            theirs.extend(files);
        }
    }
    let mut files: Vec<String> = ours
        .into_iter()
        .filter(|file| !theirs.contains(file))
        .map(str::to_owned)
        .collect();
    files.sort();
    files.dedup();
    files
}

/// One changed file as a stop sees it.
struct Dirty {
    /// Which of the stop's checkouts holds it.
    checkout: usize,
    file: String,
    tracked: bool,
    bytes: Vec<u8>,
    digest: String,
}

/// Each dirty file's content now, with its hash and whether Git tracks it. `None` when no
/// checkout could be read; a member repository that cannot is left out.
fn observed(root: &Path, checkouts: &[Checkout]) -> Option<Vec<Dirty>> {
    let listed: Vec<(usize, Vec<(String, bool)>)> = checkouts
        .iter()
        .enumerate()
        .filter_map(|(index, checkout)| Some((index, dirty(checkout)?)))
        .collect();
    if listed.is_empty() {
        return None;
    }
    let mut files: Vec<(usize, String, bool)> = listed
        .into_iter()
        .flat_map(|(index, files)| {
            files
                .into_iter()
                .map(move |(file, tracked)| (index, file, tracked))
        })
        .collect();
    files.sort_by(|a, b| a.1.cmp(&b.1));
    files.truncate(MAX_FILES);
    Some(
        files
            .into_iter()
            .filter_map(|(checkout, file, tracked)| {
                let bytes = bytes(root, &file)?;
                let digest = hash(&bytes);
                Some(Dirty {
                    checkout,
                    file,
                    tracked,
                    bytes,
                    digest,
                })
            })
            .collect(),
    )
}

/// The commit a record keeps for a checkout: `head` for the project's own, `heads` by
/// folder for a member repository.
fn seen<'a>(record: &'a Value, checkout: &Checkout) -> Option<&'a str> {
    match checkout.folder.strip_suffix('/') {
        Some(member) => record["heads"][member].as_str(),
        None => record["head"].as_str(),
    }
}

/// The record of a stop: each changed file's content, what the session was told, the commits
/// it made and the commit every checkout is on.
fn record(
    files: &[Dirty],
    delivered: &[String],
    own: &[&str],
    checkouts: &[Checkout],
    heads: &[Option<String>],
) -> Value {
    let mut record = json!({"formatVersion": 1, "files": hashes(files), "delivered": delivered});
    if !own.is_empty() {
        record["commits"] = json!(own);
    }
    let mut members = Map::new();
    for (checkout, head) in checkouts.iter().zip(heads) {
        let Some(head) = head else { continue };
        match checkout.folder.strip_suffix('/') {
            Some(member) => drop(members.insert(member.to_owned(), json!(head))),
            None => record["head"] = json!(head),
        }
    }
    if !members.is_empty() {
        record["heads"] = Value::Object(members);
    }
    record
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
    if !relevant(settings) {
        return Ok(());
    }
    let file = record_file(root, session, true)?;
    if file.exists() {
        return Ok(());
    }
    let checkouts = checkouts(root);
    let Some(files) = observed(root, &checkouts) else {
        return Ok(());
    };
    let heads: Vec<Option<String>> = checkouts.iter().map(|c| head(&c.directory)).collect();
    let record = record(&files, &[], &[], &checkouts, &heads);
    Ok(state::write(&file, &record, RECORD_BYTES, false)?)
}

/// Whether stops follow the session's changes: to review them, or to name a declared check
/// that covers them.
fn relevant(settings: &Value) -> bool {
    standards::stop_relevant(settings) || craft_engine::session::next::tracks_changes(settings)
}

/// The commits a record names as made by the session.
fn own_commits(record: &Value) -> Vec<&str> {
    record["commits"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .collect()
}

/// Before a shell command of the session runs `git commit`: the commit each checkout is on,
/// so the command's own commit can be told from one another session makes meanwhile.
pub fn commit_started(root: &Path, session: &str, settings: &Value) -> Result<(), Failure> {
    if !relevant(settings) {
        return Ok(());
    }
    let file = record_file(root, session, true)?;
    let mut record = read_record(&file, RECORD_BYTES)?;
    if !record.is_object() {
        record = json!({"formatVersion": 1});
    }
    let heads: Map<String, Value> = checkouts(root)
        .iter()
        .map(|checkout| {
            let head = head(&checkout.directory).map_or(Value::Null, Value::from);
            (checkout.folder.clone(), head)
        })
        .collect();
    record["committing"] = Value::Object(heads);
    Ok(state::write(&file, &record, RECORD_BYTES, false)?)
}

/// After that command: each checkout whose `HEAD` moved by a commit since `commit_started`
/// adds the commit to the session's own. A failed commit leaves `HEAD` where it was.
pub fn commit_made(root: &Path, session: &str, settings: &Value) -> Result<(), Failure> {
    if !relevant(settings) {
        return Ok(());
    }
    let file = record_file(root, session, false)?;
    let mut record = read_record(&file, RECORD_BYTES)?;
    let Some(before) = record
        .as_object_mut()
        .and_then(|record| record.remove("committing"))
    else {
        return Ok(());
    };
    let mut own: Vec<String> = own_commits(&record)
        .into_iter()
        .map(str::to_owned)
        .collect();
    for checkout in checkouts(root) {
        let Some(now) = head(&checkout.directory) else {
            continue;
        };
        if before[checkout.folder.as_str()].as_str() == Some(now.as_str()) {
            continue;
        }
        let latest = craft_platform::process::git(
            &checkout.directory,
            &["reflog", "-n", "1", "--format=%H %gs", "HEAD"],
            4096,
        )
        .ok()
        .flatten()
        .map(|line| String::from_utf8_lossy(&line).into_owned())
        .unwrap_or_default();
        let made = latest
            .strip_prefix(&format!("{now} "))
            .is_some_and(|action| action.starts_with("commit"));
        if made && !own.contains(&now) {
            own.push(now);
        }
    }
    let excess = own.len().saturating_sub(OWN_COMMITS);
    own.drain(..excess);
    record["commits"] = json!(own);
    Ok(state::write(&file, &record, RECORD_BYTES, false)?)
}

/// Debt markers `file`, named from its checkout's directory, adds against the commit `base`;
/// an untracked file adds every line.
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

/// Files the session was told findings for that the stop does not review because they no
/// longer differ from what it compares against (reverted, or rewritten outside any hook),
/// checked again on their current content so that findings gone from them count as fixed.
/// Nothing is said for them. A deleted file has no finding of any kind; otherwise only
/// anchor and dependency checks are repeated: debt markers are new against a base that a
/// clean file no longer shows.
fn settled(root: &Path, settings: &Value, told: &Value, reviewed: &[&str]) -> Vec<Checked> {
    let Some(open) = told["findings"]["open"].as_object() else {
        return Vec::new();
    };
    open.iter()
        .filter(|(scope, _)| *scope != "commit" && !reviewed.contains(&scope.as_str()))
        .filter_map(|(scope, keys)| {
            if !root.join(scope).exists() {
                let mut kinds: Vec<char> = keys
                    .as_array()?
                    .iter()
                    .filter_map(|key| key.as_str()?.chars().next())
                    .collect();
                kinds.sort_unstable();
                kinds.dedup();
                return Some(Checked {
                    scope: scope.clone(),
                    kinds: kinds.into_iter().collect(),
                    keys: Vec::new(),
                });
            }
            crate::hook::delivery::file_findings(root, settings, scope, &Value::Null).1
        })
        .collect()
}

/// What one stop found.
#[derive(Default)]
pub struct Stopped {
    pub summary: Option<Value>,
    pub checked: Vec<Checked>,
    /// Files named from the project root that changed since the last stop.
    pub changed: Vec<String>,
}

/// The stop summary for this turn, or `None` when nothing changed since the last stop or the
/// session was told all of it. While the host continues a held stop (`stop_active`) nothing
/// is said or recorded: a stop is never held twice in a row, and what the continuation
/// changes is reviewed at the next stop. With it, each changed file as a scope the stop
/// checked, so a finding that is gone counts as fixed, and the files changed since the last
/// stop.
pub fn stop(
    root: &Path,
    session: &str,
    settings: &Value,
    intent: Option<&str>,
    stop_active: bool,
) -> Result<Stopped, Failure> {
    if stop_active || !relevant(settings) {
        return Ok(Stopped::default());
    }
    let checkouts = checkouts(root);
    let Some(files) = observed(root, &checkouts) else {
        return Ok(Stopped::default());
    };
    let file = record_file(root, session, true)?;
    let record = read_record(&file, RECORD_BYTES)?;
    let delivered: Vec<String> = record["delivered"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|key| key.as_str().map(str::to_owned))
        .collect();
    let own = own_commits(&record);
    let heads: Vec<Option<String>> = checkouts.iter().map(|c| head(&c.directory)).collect();
    // Work the session committed since its last stop is clean now, and still its change.
    // Markers are new against the commit the last stop saw, whether or not they were
    // committed since; without one (an older record, no commit yet), against `HEAD`.
    let mut landed: Vec<Dirty> = Vec::new();
    let mut bases: Vec<&str> = vec!["HEAD"; checkouts.len()];
    for (index, checkout) in checkouts.iter().enumerate() {
        let (Some(base), Some(current)) = (seen(&record, checkout), heads[index].as_deref()) else {
            continue;
        };
        if base == current {
            continue;
        }
        let room = MAX_FILES.saturating_sub(files.len() + landed.len());
        let before = landed.len();
        landed.extend(
            committed(&checkout.directory, base, current, &own)
                .into_iter()
                .map(|file| format!("{}{file}", checkout.folder))
                .filter(|file| !files.iter().any(|dirty| dirty.file == *file))
                .take(room)
                .filter_map(|file| {
                    let bytes = bytes(root, &file)?;
                    let digest = hash(&bytes);
                    Some(Dirty {
                        checkout: index,
                        file,
                        tracked: true,
                        bytes,
                        digest,
                    })
                }),
        );
        if landed.len() > before {
            bases[index] = base;
        }
    }
    let changed: Vec<&Dirty> = files
        .iter()
        .chain(&landed)
        .filter(|dirty| record["files"][dirty.file.as_str()] != json!(dirty.digest))
        .collect();
    let mut next = self::record(&files, &delivered, &own, &checkouts, &heads);
    // Anchor findings the session heard when it edited the file are not said again.
    let told = crate::hook::delivered::read(root, session).unwrap_or(Value::Null);
    let mut summary = None;
    let reviewed: Vec<&str> = files
        .iter()
        .chain(&landed)
        .map(|dirty| dirty.file.as_str())
        .collect();
    let mut checked = settled(root, settings, &told, &reviewed);
    if !changed.is_empty() {
        let mut findings = Vec::new();
        let markers_on = standards::debt_markers(settings);
        for Dirty {
            checkout,
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
                let within = &checkouts[*checkout];
                let inside = file.strip_prefix(&*within.folder).unwrap_or(file);
                let base = bases[*checkout];
                for mut finding in markers(&within.directory, inside, *tracked, bytes, base) {
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
    let changed = changed.iter().map(|dirty| dirty.file.clone()).collect();
    Ok(Stopped {
        summary,
        checked,
        changed,
    })
}
