//! The committed comparison a review snapshot records: resolved revisions, their merge
//! base, the changed files and the diff. Reads Git only; project code never runs.
use crate::process;
use std::{path::Path, time::Duration};

const OUTPUT: usize = 16 * 1024 * 1024;
const TIMEOUT: Duration = Duration::from_secs(15);
/// Run state is never part of a reviewed change.
const SCOPE: [&str; 6] = [
    "--",
    ".",
    ":(exclude,glob)**/.craft/state/**",
    ":(exclude,glob)**/.craft/state",
    ":(exclude,glob)**/.capabilities/state/**",
    ":(exclude,glob)**/.capabilities/state",
];

pub struct Comparison {
    pub base: String,
    pub head: String,
    pub merge_base: String,
    pub files: Vec<String>,
    pub diff: String,
}

fn git(root: &Path, args: &[&str], failure: impl FnOnce() -> String) -> Result<String, String> {
    match process::git_within("Git snapshot", root, args, OUTPUT, TIMEOUT) {
        Ok(Some(bytes)) => Ok(String::from_utf8_lossy(&bytes).into_owned()),
        Ok(None) => Err(format!("Git snapshot failed: {}", failure())),
        Err(error) => Err(error),
    }
}

fn commit(root: &Path, reference: &str) -> Result<String, String> {
    let spec = format!("{reference}^{{commit}}");
    let args = ["rev-parse", "--verify", "--end-of-options", &spec];
    let id = git(root, &args, || {
        format!("`{reference}` does not name a commit in {}", root.display())
    })?;
    Ok(id.trim().to_string())
}

/// The merge-base comparison from `base` to `head`, which callers must already have
/// checked are explicit, non-option references.
pub fn compare(root: &Path, base: &str, head: &str) -> Result<Comparison, String> {
    let base_id = commit(root, base)?;
    let head_id = commit(root, head)?;
    let merge_base = git(root, &["merge-base", &base_id, &head_id], || {
        format!("`{base}` and `{head}` have no merge base")
    })?
    .trim()
    .to_string();
    let range = |options: &[&'static str]| {
        let mut args = vec!["diff"];
        args.extend_from_slice(options);
        args.extend([merge_base.as_str(), head_id.as_str()]);
        args.extend(SCOPE);
        args
    };
    let unavailable = || "the diff could not be read".to_string();
    let files = git(
        root,
        &range(&["--name-only", "-z", "--no-renames"]),
        unavailable,
    )?
    .split('\0')
    .filter(|file| !file.is_empty())
    .map(str::to_string)
    .collect();
    let diff = git(
        root,
        &range(&[
            "--no-ext-diff",
            "--no-textconv",
            "--no-color",
            "--no-renames",
            "--unified=3",
        ]),
        unavailable,
    )?;
    Ok(Comparison {
        base: base_id,
        head: head_id,
        merge_base,
        files,
        diff,
    })
}
