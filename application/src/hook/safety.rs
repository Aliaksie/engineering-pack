//! The project's safety policy at a hook: the engine judges a shell command against the
//! project root under each spelling it has on disk and the temporary directories, and a
//! tool call that names a secret file, by a built-in name or a pattern a layer declared.
use craft_engine::safety::Place;
use craft_platform::files;
use serde_json::Value;
use std::path::Path;

/// `allow`, or `deny` with the rule and the reason, for `command` run from `cwd`.
pub(crate) fn decision(settings: &Value, root: &Path, cwd: &Path, command: &str) -> Value {
    if !craft_engine::safety::enabled(settings) {
        return serde_json::json!({"decision": "allow"});
    }
    let roots = files::spellings(root);
    let cwd = files::spellings(cwd).swap_remove(0);
    let temp = files::temporary_directories();
    let place = Place {
        roots: &roots,
        cwd: &cwd,
        temp: &temp,
    };
    craft_engine::safety::decision(settings, &place, command)
}

/// `allow`, or the secret-file refusal, for a tool call that names `secret`, a file or
/// search filter that reaches secret files. A call that `rewrites` a file is judged only when
/// the file exists: its old content comes back to the model, while a new file holds nothing.
pub(crate) fn read_decision(settings: &Value, cwd: &Path, rewrites: bool, secret: &str) -> Value {
    if rewrites && !cwd.join(secret).exists() {
        return serde_json::json!({"decision": "allow"});
    }
    craft_engine::safety::read_decision(settings, secret)
}

/// What a tool call names that no built-in name marks secret: the files and search filters
/// judged against the patterns a configuration layer declared.
#[derive(Default)]
pub(crate) struct Named<'a> {
    pub files: Vec<&'a str>,
    pub filters: Vec<&'a str>,
}

impl<'a> Named<'a> {
    /// The `named` hook fact: `{files, filters}`, each a list of text.
    pub(crate) fn of(fact: &'a Value) -> Result<Self, String> {
        let list = |name: &str| match &fact[name] {
            Value::Null => Ok(Vec::new()),
            Value::Array(items) => items
                .iter()
                .map(|item| {
                    item.as_str()
                        .ok_or_else(|| "invalid hook named".to_string())
                })
                .collect(),
            _ => Err("invalid hook named".to_string()),
        };
        match fact {
            Value::Null => Ok(Self::default()),
            Value::Object(_) => Ok(Self {
                files: list("files")?,
                filters: list("filters")?,
            }),
            _ => Err("invalid hook named".into()),
        }
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.files.is_empty() && self.filters.is_empty()
    }
}

/// The file or filter a tool call names that a secret-file pattern of the project at `root`
/// matches, read without recording: an observation hook loads, and records, the project's
/// settings only for a call that names a secret file.
pub(crate) fn declared<'a>(root: &Path, named: &Named<'a>) -> Option<&'a str> {
    if named.is_empty() {
        return None;
    }
    let patterns = crate::project::secret_files(root);
    (!patterns.is_empty())
        .then(|| patterns.named(&named.files, &named.filters))
        .flatten()
}

/// The file or filter a tool call names that reaches secret files under `settings`: `found`,
/// matched by a built-in name before settings were loaded, else the first of `named` a
/// declared pattern matches.
pub(crate) fn secret<'a>(
    settings: &Value,
    found: Option<&'a str>,
    named: &Named<'a>,
) -> Option<&'a str> {
    found.or_else(|| {
        (!named.is_empty())
            .then(|| craft_engine::safety::declared_secret(settings, &named.files, &named.filters))
            .flatten()
    })
}
