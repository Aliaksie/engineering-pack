//! Option scanning: every command accepts the shared option set; only selected
//! command-specific options are refused on other commands.
use crate::failure::Failure;
use serde_json::{Map, Value, json};

/// Options every command accepts, as the shared JS parser does; each command reads its own.
const OPTIONS: &[&str] = &[
    "contract",
    "acceptance-evidence",
    "view",
    "current-kind",
    "current-identity",
    "probes",
    "focus",
    "blockers",
    "expected-evidence-digest",
    "expected-task-contract",
    "repository",
    "feature",
    "feature-status",
    "reason",
    "capture",
    "review-authorization",
    "action",
    "scope",
    "benefit",
    "offer",
    "decision",
    "role",
    "prior-authorization",
    "facts",
    "goal",
    "stage",
    "target",
    "approach",
    "evidence",
    "outcome",
    "result",
    "next",
    "profile",
    "complete",
    "preferences",
    "root",
    "intent",
    "file",
    "host",
    "package",
    "observed-at",
    "delegate",
    "task",
    "risk",
    "operation",
    "params",
    "session",
    "data",
    "module",
    "term",
    "path",
    "max-files",
    "max-matches",
    "max-bytes",
    "available-command",
    "available-mcp-tool",
    "bundle",
    "budget-bytes",
    "verification-mode",
    "json",
    "verbose",
    "max-output-bytes",
    "apply",
    "from",
    "plan",
    "reference",
    "expected-revision",
    "take-over",
    "brief",
    "ready",
    "shared",
    "escalate",
    "discard",
];
const FLAGS: &[&str] = &[
    "apply",
    "delegate",
    "json",
    "verbose",
    "complete",
    "blockers",
    "take-over",
    "brief",
    "ready",
    "shared",
    "escalate",
    "discard",
];
const REPEATED: &[&str] = &[
    "repository",
    "file",
    "risk",
    "term",
    "path",
    "available-command",
    "available-mcp-tool",
    "bundle",
    "reference",
];

#[derive(Default)]
pub(super) struct Options {
    pub(super) values: Map<String, Value>,
}

impl Options {
    pub(super) fn get(&self, name: &str) -> Option<&str> {
        self.values.get(name).and_then(Value::as_str)
    }
}

pub(super) fn parse(action: &str, args: &[String]) -> Result<Options, Failure> {
    let mut options = Options::default();
    let mut index = 0;
    while index < args.len() {
        let arg = &args[index];
        let name = arg
            .strip_prefix("--")
            .filter(|name| OPTIONS.contains(name))
            .ok_or_else(|| format!("unknown argument: {arg}"))?;
        index += 1;
        if FLAGS.contains(&name) {
            if options.values.insert(name.into(), json!(true)).is_some() {
                return Err(format!("duplicate option --{name}").into());
            }
            continue;
        }
        let value = args
            .get(index)
            .filter(|value| !value.starts_with("--"))
            .ok_or_else(|| format!("missing value for {arg}"))?;
        index += 1;
        if REPEATED.contains(&name) {
            options
                .values
                .entry(name)
                .or_insert_with(|| json!([]))
                .as_array_mut()
                .expect("repeated option")
                .push(json!(value));
        } else if options.values.insert(name.into(), json!(value)).is_some() {
            return Err(format!("duplicate option {arg}").into());
        }
    }
    let present = |name: &str| options.values.contains_key(name);
    if present("contract") && !["task-start", "acceptance-view"].contains(&action) {
        return Err("--contract is supported by task-start and acceptance-view".into());
    }
    if present("acceptance-evidence") && !["task-step", "acceptance-view"].contains(&action) {
        return Err("--acceptance-evidence is supported by task-step and acceptance-view".into());
    }
    if ["expected-task-contract", "expected-evidence-digest"]
        .iter()
        .any(|name| present(name))
        && action != "task-step"
    {
        return Err("structured evidence options require task-step".into());
    }
    let view_option = [
        "view",
        "current-kind",
        "current-identity",
        "probes",
        "focus",
        "blockers",
    ]
    .iter()
    .find(|name| present(name));
    if let Some(name) = view_option.filter(|_| action != "acceptance-view") {
        return Err(format!("--{name} is supported by acceptance-view").into());
    }
    if present("take-over") && !["task-start", "plan-update", "plan-release"].contains(&action) {
        return Err("--take-over is supported by task-start, plan-update and plan-release".into());
    }
    if present("ready") && action != "task-start" {
        return Err("--ready is supported by task-start".into());
    }
    if present("shared") && action != "plan-assign" {
        return Err("--shared is supported by plan-assign".into());
    }
    if ["escalate", "discard"].iter().any(|name| present(name)) && action != "plan-integrate" {
        return Err("--escalate and --discard are supported by plan-integrate".into());
    }
    if present("brief") && action != "plan-envelope" {
        return Err("--brief is supported by plan-envelope".into());
    }
    if present("repository") && !["task-start", "checkpoint-save"].contains(&action) {
        return Err("--repository is supported by task-start and checkpoint-save; other commands reuse the saved scope".into());
    }
    Ok(options)
}
