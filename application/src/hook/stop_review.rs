//! The stop summary: when a turn ends after changing files, what those changes introduced
//! (module dependencies the project does not declare, debt markers, anchor findings the
//! session was not told when the file was written) and the declared `review` items of their
//! scope, said once. The changes are read from the worktree, so a file written by a shell
//! command or a script counts like an edited one, and so does one the session committed
//! before the turn ended. A commit counts as the session's own only when a shell command the
//! session ran made it: another session working in the same checkout, or the user committing
//! by hand, is not this session's change. Where the host reports every tool call that writes,
//! an uncommitted file counts only when it was last written while one of the session's tool
//! calls ran: another session editing the same checkout meanwhile is not this session's
//! change either. The session remembers each changed file's content
//! and the commit it stopped on, so a stop reviews only files changed since the last one, and
//! what it was told, so the same finding never holds a stop twice. At a workspace root, a folder that is
//! no repository itself, each member repository is read the same way and its files are named
//! from the workspace root. While a ticket's plan waits for the user's approval, files the
//! session changed since the plan was chosen are named too: a shell command writes past the
//! hold on edit tools.
use crate::failure::Failure;
use crate::project::{Checkout, checkouts};
use crate::session::read_record;
use craft_engine::safety::secrets::Declared;
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
/// How many `HEAD` movements back the last stop's commit is looked for, and how many of its
/// own commits a session remembers.
const REFLOG_ENTRIES: &str = "64";
const OWN_COMMITS: usize = 64;
/// How many tool-call spans a session keeps between two stops; beyond, the two earliest
/// become one that covers both.
const SPANS: usize = 256;
/// How far before a span and after it a file's modification time still falls within it: file
/// systems that keep coarse times round them down.
const BEFORE_MS: i64 = 2_000;
const AFTER_MS: i64 = 1_000;
/// How often a tool hook tries the record's lock before writing without it.
const LOCK_TRIES: u32 = 50;

fn record_file(root: &Path, session: &str, create: bool) -> Result<std::path::PathBuf, Failure> {
    Ok(state::file(
        root,
        "stop-review.json",
        create,
        Some(session),
    )?)
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

/// A regular file's bytes, `None` when absent, a link, too large or a file that commonly
/// holds secrets or matches a declared secret-file pattern: a finding quotes the line it is
/// about.
fn bytes(root: &Path, secrets: &Declared, file: &str) -> Option<Vec<u8>> {
    if secrets.secret_file(file) {
        return None;
    }
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
fn observed(root: &Path, secrets: &Declared, checkouts: &[Checkout]) -> Option<Vec<Dirty>> {
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
                let bytes = bytes(root, secrets, &file)?;
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
    if !relevant(root, session, settings) {
        return Ok(());
    }
    let file = record_file(root, session, true)?;
    if file.exists() {
        return Ok(());
    }
    let checkouts = checkouts(root);
    let secrets = Declared::of(settings);
    let Some(files) = observed(root, &secrets, &checkouts) else {
        return Ok(());
    };
    let heads: Vec<Option<String>> = checkouts.iter().map(|c| head(&c.directory)).collect();
    let record = record(&files, &[], &[], &checkouts, &heads);
    Ok(state::write(&file, &record, RECORD_BYTES, false)?)
}

/// Remember what the worktree holds when the user chooses to plan a ticket first: those files
/// were not changed while the plan waits for approval. A worktree that cannot be read keeps
/// no baseline.
pub fn approval_baseline(root: &Path, session: &str, settings: &Value) -> Result<(), Failure> {
    let secrets = Declared::of(settings);
    let held = observed(root, &secrets, &checkouts(root)).map(|files| hashes(&files));
    update(root, session, |record| match held {
        Some(held) => record["beforeApproval"] = held,
        None => drop(record.as_object_mut().map(|r| r.remove("beforeApproval"))),
    })
}

/// Whether stops follow the session's changes: to review them, to name a declared check that
/// covers them, or to offer the review of a planned change.
fn relevant(root: &Path, session: &str, settings: &Value) -> bool {
    standards::stop_relevant(settings)
        || craft_engine::session::next::tracks_changes(settings)
        // A change the user chose to plan first waits for approval of its plan and ends with a
        // review offer once files changed.
        || crate::ticket::plan_first(root, session)
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
    if !relevant(root, session, settings) {
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
    if !relevant(root, session, settings) {
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

/// Change the session's record under its lock. Parallel tool calls of one session write it
/// at once; a lock still held after a short wait is not waited for.
fn update(root: &Path, session: &str, change: impl FnOnce(&mut Value)) -> Result<(), Failure> {
    let file = record_file(root, session, true)?;
    let mut lock = None;
    for _ in 0..LOCK_TRIES {
        lock = state::lock(&file)?;
        if lock.is_some() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
    let mut record = read_record(&file, RECORD_BYTES)?;
    if !record.is_object() {
        record = json!({"formatVersion": 1});
    }
    change(&mut record);
    state::write(&file, &record, RECORD_BYTES, false)?;
    drop(lock);
    Ok(())
}

/// Before a tool call of the session runs: a span opens, to close when the call ends.
pub fn tool_started(root: &Path, session: &str, settings: &Value) -> Result<(), Failure> {
    if !relevant(root, session, settings) {
        return Ok(());
    }
    let now = crate::telemetry::now_ms();
    update(root, session, |record| {
        let mut spans = record["spans"].as_array().cloned().unwrap_or_default();
        spans.push(json!([now, null]));
        if spans.len() > SPANS {
            let (first, second) = (spans.remove(0), spans.remove(0));
            let end = match (first[1].as_i64(), second[1].as_i64()) {
                (Some(a), Some(b)) => json!(a.max(b)),
                _ => Value::Null,
            };
            spans.insert(0, json!([first[0], end]));
        }
        record["spans"] = json!(spans);
    })
}

/// After a tool call of the session: the latest open span closes. Parallel calls close each
/// other's spans, which covers the same time. A span that never closes (a host that reports
/// no failed call) lasts until the stop. Without an open span (its start was not recorded),
/// the call's end stands for it: a write that just finished falls within the slack.
pub fn tool_finished(root: &Path, session: &str, settings: &Value) -> Result<(), Failure> {
    if !relevant(root, session, settings) {
        return Ok(());
    }
    let now = crate::telemetry::now_ms();
    update(root, session, |record| {
        let open = record["spans"]
            .as_array_mut()
            .and_then(|spans| spans.iter_mut().rev().find(|span| span[1].is_null()));
        match open {
            Some(open) => open[1] = json!(now),
            None => {
                let mut spans = record["spans"].as_array().cloned().unwrap_or_default();
                if spans.len() < SPANS {
                    spans.push(json!([now, now]));
                }
                record["spans"] = json!(spans);
            }
        }
    })
}

/// Whether `file` was last written while a span of `spans` ran; an open span lasts to `now`.
fn written_within(root: &Path, file: &str, spans: &[Value], now: i64) -> bool {
    let Ok(meta) = std::fs::symlink_metadata(root.join(file)) else {
        return true;
    };
    let Some(modified) = meta
        .modified()
        .ok()
        .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
        .and_then(|since| i64::try_from(since.as_millis()).ok())
    else {
        return true;
    };
    // A command can keep a file's old modification time (`cp -p`, an archive extracted); the
    // status change time it cannot set says when the content was written. Windows keeps no
    // such time, so there a file written that way is not counted.
    #[cfg(unix)]
    let modified = {
        use std::os::unix::fs::MetadataExt;
        modified.max(meta.ctime().saturating_mul(1000) + meta.ctime_nsec() / 1_000_000)
    };
    spans.iter().any(|span| {
        let (Some(start), end) = (span[0].as_i64(), span[1].as_i64().unwrap_or(now)) else {
            return false;
        };
        start - BEFORE_MS <= modified && modified <= end + AFTER_MS
    })
}

/// Lines a change adds, each with its line in the new file, and lines it removes.
type Lines = (Vec<(usize, String)>, Vec<String>);

/// The lines `file`, named within its checkout at `root`, adds and removes against `base`:
/// every line of an untracked file; `None` when the diff is too large or there is no commit
/// to compare with.
fn lines(root: &Path, file: &str, tracked: bool, bytes: &[u8], base: &str) -> Option<Lines> {
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
    match diff {
        Some(Some(diff)) => Some(debt::changes(&String::from_utf8_lossy(&diff))),
        // Too large to compare, or no commit to compare with: unknown.
        Some(None) => None,
        None => Some((
            String::from_utf8_lossy(bytes)
                .lines()
                .enumerate()
                .map(|(index, line)| (index + 1, line.to_owned()))
                .collect(),
            Vec::new(),
        )),
    }
}

/// What `base` held of `file`, named within its checkout at `root`: `Ok(None)` when it held no
/// such file, `Err` when it cannot be read or compared.
fn earlier(root: &Path, file: &str, tracked: bool, base: &str) -> Result<Option<Vec<u8>>, ()> {
    if !tracked {
        return Ok(None);
    }
    craft_platform::process::git(root, &["show", &format!("{base}:./{file}")], DIFF_BYTES)
        .map_err(|_| ())
}

/// Whether `base` lacks the top-level folder of `file`, named within its checkout at `root`.
fn new_folder(root: &Path, file: &str, base: &str) -> bool {
    file.split_once('/').is_some_and(|(top, _)| {
        matches!(
            craft_platform::process::git(root, &["cat-file", "-e", &format!("{base}:{top}")], 1024),
            Ok(None)
        )
    })
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

/// The files of `changed` written while the session's ticket plan waits for the user's
/// approval, with why it waits and the step that lets work go on; `None` when nothing waits or
/// every changed file held the same content when the plan was chosen.
fn unapproved(
    root: &Path,
    session: &str,
    host: &str,
    record: &Value,
    changed: &[&Dirty],
) -> Option<Value> {
    let files: Vec<&str> = changed
        .iter()
        .filter(|dirty| record["beforeApproval"][dirty.file.as_str()] != json!(dirty.digest))
        .map(|dirty| dirty.file.as_str())
        .collect();
    if files.is_empty() {
        return None;
    }
    let crate::ticket::Due { why, next } = crate::ticket::awaiting(root, session, host)?;
    Some(json!({"files": files, "why": why, "next": next}))
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
/// stop. With `attribute`, an uncommitted file counts only when the session's tool calls
/// were running as it was last written.
pub fn stop(
    root: &Path,
    session: &str,
    settings: &Value,
    intent: Option<&str>,
    stop_active: bool,
    attribute: bool,
    host: &str,
) -> Result<Stopped, Failure> {
    if stop_active || !relevant(root, session, settings) {
        return Ok(Stopped::default());
    }
    let checkouts = checkouts(root);
    let secrets = Declared::of(settings);
    let Some(files) = observed(root, &secrets, &checkouts) else {
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
    // Every file the session committed since its last stop, including one edited again
    // since: it is dirty, so not in `landed`, and still in a commit the session made.
    let mut committed_files: Vec<String> = Vec::new();
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
        let from = committed_files.len();
        committed_files.extend(
            committed(&checkout.directory, base, current, &own)
                .into_iter()
                .map(|file| format!("{}{file}", checkout.folder)),
        );
        landed.extend(
            committed_files[from..]
                .iter()
                .filter(|file| !files.iter().any(|dirty| dirty.file == **file))
                .take(room)
                .cloned()
                .filter_map(|file| {
                    let bytes = bytes(root, &secrets, &file)?;
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
    let spans = record["spans"].as_array().cloned().unwrap_or_default();
    let now = crate::telemetry::now_ms();
    let written = |dirty: &Dirty| !attribute || written_within(root, &dirty.file, &spans, now);
    let changed: Vec<&Dirty> = files
        .iter()
        .filter(|dirty| written(dirty))
        .chain(&landed)
        .filter(|dirty| record["files"][dirty.file.as_str()] != json!(dirty.digest))
        .collect();
    let mut next = self::record(&files, &delivered, &own, &checkouts, &heads);
    if let Some(held) = record.get("beforeApproval") {
        next["beforeApproval"] = held.clone();
    }
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
        let mut facts: Vec<(String, Vec<&str>)> = Vec::new();
        let markers_on = standards::debt_markers(settings);
        let conditions = standards::conditions(settings);
        let wants = |fact: &str| conditions.contains(&fact);
        for Dirty {
            checkout,
            file,
            tracked,
            bytes,
            ..
        } in changed.iter().copied()
        {
            let within = &checkouts[*checkout];
            let inside = file.strip_prefix(&*within.folder).unwrap_or(file);
            let base = bases[*checkout];
            let diff = (markers_on || wants("behavior") || wants("prose"))
                .then(|| lines(&within.directory, inside, *tracked, bytes, base))
                .flatten();
            if !conditions.is_empty() {
                let (created, imports) = if wants("structure") {
                    match earlier(&within.directory, inside, *tracked, base) {
                        Ok(old) => (
                            old.is_none(),
                            crate::dependencies::added(root, settings, file, bytes, old.as_deref()),
                        ),
                        Err(()) => (false, Vec::new()),
                    }
                } else {
                    (false, Vec::new())
                };
                let change = standards::Change {
                    file,
                    lines: diff
                        .as_ref()
                        .map(|(added, removed)| (&added[..], &removed[..])),
                    created,
                    new_folder: created && new_folder(&within.directory, inside, base),
                    imports: &imports,
                    committed: committed_files.contains(file),
                };
                facts.push((file.clone(), standards::change::facts(settings, &change)));
            }
            let from = findings.len();
            for verdict in crate::dependencies::verdicts(root, settings, file, bytes) {
                for finding in verdict["findings"].as_array().into_iter().flatten() {
                    let mut finding = finding.clone();
                    finding["file"] = json!(file);
                    finding["standard"] = verdict["standard"].clone();
                    findings.push(finding);
                }
            }
            if markers_on && let Some((added, removed)) = &diff {
                for mut finding in debt::markers(added, removed) {
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
            .map(|scope| {
                standards::reviewed(settings, intent.unwrap_or("implement"), &scope, &facts)
            })
            .unwrap_or_default();
        if let Some(stop) = debt::stop(&findings, &review, &delivered) {
            next["delivered"] = json!(debt::delivered(&delivered, &stop.keys));
            summary = Some(stop.summary);
        }
        if let Some(waiting) = unapproved(root, session, host, &record, &changed) {
            summary.get_or_insert_with(|| json!({}))["approval"] = waiting;
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
