//! The revision-bound review tools: `review-prepare` snapshots a committed comparison into a
//! new artifact, `review-check` validates a returned review against it and
//! `review-publication` plans a comment for an authorized publisher. `review-followup` reads
//! a fetched pull request observation and says what its reviewer should do next. None of
//! them posts or contacts a code host.
use crate::cli::Outcome;
use crate::failure::Failure;
use crate::project::absolute;
use crate::telemetry::observe;
use crate::telemetry::{Operation, Recorder};
use craft_engine::{
    contracts::{js_blank, normalize_numbers},
    review::artifact::{self, PROVIDERS, Target, VERIFICATION},
    review::followup::{self, Facts},
    timestamp,
};
use craft_platform::{
    files::{ReadError, read_bounded},
    repository::comparison,
};
use serde_json::{Map, Value, json};
use std::{
    collections::HashMap,
    fs::OpenOptions,
    io::{self, Write},
    path::Path,
};

/// A snapshot holds a diff of up to 16 MiB; JSON escaping can grow it several times.
const SNAPSHOT_BYTES: usize = 128 * 1024 * 1024;
const REVIEW_BYTES: usize = 8 * 1024 * 1024;
const OBSERVATION_BYTES: usize = 16 * 1024 * 1024;
const STALE_DAYS: i64 = 5;

#[derive(Clone, Copy)]
pub enum Command {
    Prepare,
    Check,
    Publication,
    Followup,
}

impl Command {
    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            "review-prepare" => Some(Self::Prepare),
            "review-check" => Some(Self::Check),
            "review-publication" => Some(Self::Publication),
            "review-followup" => Some(Self::Followup),
            _ => None,
        }
    }

    fn options(self) -> &'static [&'static str] {
        match self {
            Self::Prepare => &[
                "root",
                "base",
                "head",
                "provider",
                "repository",
                "change-id",
                "output",
            ],
            Self::Check => &["snapshot", "review"],
            Self::Publication => &["snapshot", "review", "current-base", "current-head"],
            Self::Followup => &[
                "observation",
                "reviewer",
                "now",
                "stale-days",
                "acknowledged-threads",
            ],
        }
    }

    fn usage(self) -> &'static str {
        match self {
            Self::Prepare => {
                "review-prepare --base ref --head ref --output file [--root repo] [--provider local|github|gitlab --repository name --change-id n]"
            }
            Self::Check => "review-check --snapshot snapshot.json --review review.json",
            Self::Publication => {
                "review-publication --snapshot snapshot.json --review review.json --current-base sha --current-head sha [--authorized]"
            }
            Self::Followup => {
                "review-followup --observation response.json --reviewer login [--now time] [--stale-days n] [--acknowledged-threads id,...]"
            }
        }
    }

    fn phase(self) -> &'static str {
        match self {
            Self::Prepare => "prepared",
            Self::Check => "validated",
            Self::Publication => "publication-planned",
            Self::Followup => "followup-checked",
        }
    }
}

struct Options {
    values: HashMap<String, String>,
    authorized: bool,
}

impl Options {
    fn get(&self, name: &str) -> Option<&str> {
        self.values.get(name).map(String::as_str)
    }
}

/// Named options only, each at most once and with a nonempty value.
fn parse(command: Command, args: &[String]) -> Result<Options, Failure> {
    let usage = || Failure::from(format!("usage: craft {}", command.usage()));
    let mut options = Options {
        values: HashMap::new(),
        authorized: false,
    };
    let mut args = args.iter();
    while let Some(arg) = args.next() {
        let name = arg.strip_prefix("--").ok_or_else(usage)?;
        if matches!(command, Command::Publication) && name == "authorized" {
            if std::mem::replace(&mut options.authorized, true) {
                return Err(usage());
            }
            continue;
        }
        let value = args.next().filter(|value| !value.is_empty());
        match value {
            Some(value) if command.options().contains(&name) => {
                if options.values.insert(name.into(), value.clone()).is_some() {
                    return Err(usage());
                }
            }
            _ => return Err(usage()),
        }
    }
    Ok(options)
}

fn required<'a>(options: &'a Options, command: Command, name: &str) -> Result<&'a str, Failure> {
    options
        .get(name)
        .ok_or_else(|| format!("usage: craft {}", command.usage()).into())
}

/// An I/O failure that names what was attempted and keeps its kind for telemetry.
fn described(error: io::Error, action: &str) -> Failure {
    Failure::from_io(io::Error::new(error.kind(), format!("{action}: {error}")))
}

/// A JSON artifact, decoded as text first so invalid UTF-8 is replaced rather than refused.
fn artifact(file: &str, max_bytes: usize, label: &str) -> Result<Value, Failure> {
    let bytes = read_bounded(&absolute(file), max_bytes).map_err(|error| match error {
        ReadError::Io(error) => described(error, &format!("cannot read {label} {file}")),
        _ => format!(
            "{label} exceeds {} MiB or is not a regular file",
            max_bytes >> 20
        )
        .into(),
    })?;
    let mut value: Value = serde_json::from_str(&String::from_utf8_lossy(&bytes))
        .map_err(|error| format!("invalid {label} JSON: {error}"))?;
    normalize_numbers(&mut value);
    Ok(value)
}

fn snapshot(options: &Options) -> Result<Value, Failure> {
    let provider = options.get("provider").unwrap_or("local");
    let repository = options.get("repository").unwrap_or("local");
    let change_id = options.get("change-id");
    if !PROVIDERS.contains(&provider) {
        return Err("unsupported review provider".into());
    }
    if provider != "local" && !change_id.is_some_and(|id| artifact::change_id(&json!(id))) {
        return Err("remote change id required".into());
    }
    if js_blank(repository) {
        return Err("repository identity required".into());
    }
    let refs = [options.get("base"), options.get("head")];
    let [Some(base), Some(head)] = refs.map(|r| r.filter(|r| !r.starts_with('-'))) else {
        return Err("explicit base and head references required".into());
    };
    let root = absolute(options.get("root").unwrap_or("."));
    let compared = comparison::compare(&root, base, head)?;
    let snapshot = json!({
        "formatVersion": 1,
        "provider": provider,
        "repository": repository,
        "changeId": change_id,
        "baseSha": compared.base,
        "headSha": compared.head,
        "mergeBase": compared.merge_base,
        "files": compared.files,
        "diff": compared.diff,
        "verification": VERIFICATION,
    });
    artifact::validate_snapshot(&snapshot)?;
    Ok(snapshot)
}

/// Writes a new artifact only; an existing file is never replaced.
fn write_new(file: &Path, text: &str) -> Result<(), Failure> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
    let failed = |error| described(error, &format!("cannot create {}", file.display()));
    let mut handle = options.open(file).map_err(failed)?;
    handle.write_all(text.as_bytes()).map_err(failed)
}

fn prepare(
    options: &Options,
    recorder: &mut Recorder,
    parent: &Operation,
) -> Result<String, Failure> {
    let output = options.get("output").ok_or(
        "--output is required; keep the diff in an artifact rather than the model transcript",
    )?;
    let summary = Map::from_iter([("phase".to_string(), json!("prepared"))]);
    let snapshot = observe(
        recorder,
        "review.completed",
        parent,
        "REVIEW_INVALID",
        summary,
        |_, _| snapshot(options),
    )?;
    let file = absolute(output);
    let text = format!(
        "{}\n",
        serde_json::to_string_pretty(&snapshot).expect("JSON values serialize")
    );
    write_new(&file, &text)?;
    let receipt = json!({
        "artifact": file.to_string_lossy(),
        "baseSha": snapshot["baseSha"],
        "headSha": snapshot["headSha"],
        "files": snapshot["files"].as_array().map_or(0, Vec::len),
    });
    Ok(format!("{receipt}\n"))
}

/// The follow-up check and whether every pull request in it could be decided.
fn followup(
    options: &Options,
    recorder: &mut Recorder,
    parent: &Operation,
) -> Result<(Value, bool), Failure> {
    let command = Command::Followup;
    let file = required(options, command, "observation")?;
    let reviewer = required(options, command, "reviewer")?;
    let now = match options.get("now") {
        Some(text) => timestamp::parse(text).ok_or("--now must be an ISO 8601 time")?,
        None => crate::telemetry::now_ms(),
    };
    let stale_days = match options.get("stale-days") {
        Some(text) => text
            .parse::<u16>()
            .map(i64::from)
            .map_err(|_| "--stale-days must be a whole number of days up to 65535")?,
        None => STALE_DAYS,
    };
    let acknowledged = options
        .get("acknowledged-threads")
        .map(|ids| {
            ids.split(',')
                .filter(|id| !id.is_empty())
                .map(String::from)
                .collect()
        })
        .unwrap_or_default();
    let facts = Facts {
        reviewer,
        now,
        stale_days,
        acknowledged: &acknowledged,
    };
    let response = artifact(file, OBSERVATION_BYTES, "observation")?;
    let summary = Map::from_iter([("phase".to_string(), json!("followup-checked"))]);
    let result = observe(
        recorder,
        "review.completed",
        parent,
        "REVIEW_INVALID",
        summary,
        |_, _| Ok(followup::check(&response, &facts)?),
    )?;
    let complete = result["complete"] == true;
    Ok((result, complete))
}

fn evaluate(
    command: Command,
    options: &Options,
    recorder: &mut Recorder,
    parent: &Operation,
) -> Result<(String, i32), Failure> {
    let pretty = |value: &Value| {
        format!(
            "{}\n",
            serde_json::to_string_pretty(value).expect("JSON values serialize")
        )
    };
    match command {
        Command::Prepare => return prepare(options, recorder, parent).map(|text| (text, 0)),
        // An undecided pull request is a result too; exit 1 tells the caller to refetch it.
        Command::Followup => {
            let (result, complete) = followup(options, recorder, parent)?;
            return Ok((pretty(&result), if complete { 0 } else { 1 }));
        }
        Command::Check | Command::Publication => {}
    }
    let snapshot_file = required(options, command, "snapshot")?;
    let review_file = required(options, command, "review")?;
    let summary = Map::from_iter([("phase".to_string(), json!(command.phase()))]);
    let result = if matches!(command, Command::Check) {
        let snapshot = artifact(snapshot_file, SNAPSHOT_BYTES, "snapshot")?;
        let review = artifact(review_file, REVIEW_BYTES, "review")?;
        observe(
            recorder,
            "review.completed",
            parent,
            "REVIEW_INVALID",
            summary,
            |_, _| Ok(artifact::validate_review(&snapshot, &review)?),
        )?
    } else {
        let target = Target {
            current_base: required(options, command, "current-base")?,
            current_head: required(options, command, "current-head")?,
            authorized: options.authorized,
        };
        let snapshot = artifact(snapshot_file, SNAPSHOT_BYTES, "snapshot")?;
        let review = artifact(review_file, REVIEW_BYTES, "review")?;
        let name = "review.publication-planned";
        observe(recorder, name, parent, "REVIEW_INVALID", summary, |_, _| {
            Ok(artifact::publication_plan(&snapshot, &review, &target)?)
        })?
    };
    Ok((pretty(&result), 0))
}

/// One review command under a top-level `review.command` operation.
pub fn run(command: Command, args: &[String], context: &Value) -> Outcome {
    if args.iter().any(|arg| arg == "--help") {
        let help = json!({ "usage": command.usage() });
        return Outcome {
            error_code: None,
            exit_code: 0,
            stdout: format!(
                "{}\n",
                serde_json::to_string_pretty(&help).expect("JSON values serialize")
            ),
            stderr: String::new(),
            export: None,
        };
    }
    let root = args
        .iter()
        .position(|arg| arg == "--root")
        .and_then(|index| args.get(index + 1))
        .filter(|value| !value.starts_with("--"))
        .map_or(".", String::as_str);
    let mut recorder = Recorder::open(&absolute(root), None, None);
    recorder.package(context["package"].as_str());
    let operation = recorder.start("review.command", None);
    let result = parse(command, args)
        .and_then(|options| evaluate(command, &options, &mut recorder, &operation));
    let (attributes, failure) = match &result {
        Ok(_) => (
            Map::from_iter([("phase".to_string(), json!(command.phase()))]),
            None,
        ),
        Err(failure) => (Map::new(), Some(failure.code("REVIEW_INVALID"))),
    };
    recorder.complete(operation, attributes, failure);
    let (exit_code, stdout, stderr, error_code) = match result {
        Ok((text, exit_code)) => (exit_code, text, String::new(), None),
        Err(failure) => (
            2,
            String::new(),
            format!("{}\n", failure.message),
            failure.node_code(),
        ),
    };
    Outcome {
        error_code,
        exit_code,
        stdout,
        stderr,
        export: recorder.finish(),
    }
}
