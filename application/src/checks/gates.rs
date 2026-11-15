//! The declared checks at Git's pre-commit hook and in CI: `craft check --install-hook`
//! writes a hook only on `--apply` and only where no other hook or hook manager owns
//! pre-commit; `craft check --ci-snippet` prints a job and writes nothing.
use crate::failure::Failure;
use craft_engine::checks::gates;
use craft_platform::{files, process, state};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};

/// The largest pre-commit hook read to tell Craft's own from another.
const HOOK_BYTES: usize = 65_536;

/// The checkout `root` lies in: its top and the project folder below it (`""` at the top).
fn checkout(root: &Path) -> Option<(PathBuf, String)> {
    let line = |args: &[&str]| {
        process::git(root, args, 4096)
            .ok()
            .flatten()
            .and_then(|output| String::from_utf8(output).ok())
            .map(|text| text.trim_end_matches(['\n', '\r']).to_owned())
    };
    let top = line(&["rev-parse", "--show-toplevel"]).filter(|top| !top.is_empty())?;
    let prefix = line(&["rev-parse", "--show-prefix"])?;
    Some((PathBuf::from(top), prefix))
}

/// The launcher a hook runs: the installed package's `bin/craft` when this runs from one,
/// otherwise this executable.
fn launcher() -> Result<String, Failure> {
    let package = std::env::var(crate::statusline::PACKAGE_ROOT)
        .ok()
        .filter(|root| !root.is_empty())
        .and_then(|root| state::physical(&Path::new(&root).join("bin/craft")).ok());
    let path = match package {
        Some(path) => path,
        None => std::env::current_exe()
            .map_err(|error| format!("the craft executable cannot be located: {error}"))?,
    };
    path.to_str()
        .map(str::to_owned)
        .ok_or_else(|| "the craft launcher path is not UTF-8 text".into())
}

/// The current pre-commit hook: `None` when there is none; a hook Craft cannot read as text
/// (a link, too large, not UTF-8) reads as another's.
fn existing(hook: &Path) -> Result<Option<String>, Failure> {
    match std::fs::symlink_metadata(hook) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(format!("{} cannot be read: {error}", hook.display()).into()),
        Ok(meta) if !meta.is_file() => return Ok(Some(String::new())),
        Ok(_) => {}
    }
    Ok(Some(
        files::read_bounded(hook, HOOK_BYTES)
            .ok()
            .and_then(|bytes| String::from_utf8(bytes).ok())
            .unwrap_or_default(),
    ))
}

/// Write the hook through a sibling file, executable, replacing the previous one in one step.
fn write_hook(hook: &Path, script: &str) -> Result<(), Failure> {
    let directory = hook.parent().ok_or("the hook has no directory")?;
    std::fs::create_dir_all(directory)
        .map_err(|error| format!("{} cannot be created: {error}", directory.display()))?;
    let staged = directory.join(".pre-commit.craft");
    std::fs::write(&staged, script)
        .map_err(|error| format!("{} cannot be written: {error}", staged.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&staged, std::fs::Permissions::from_mode(0o755))
            .map_err(|error| format!("{} cannot be made executable: {error}", staged.display()))?;
    }
    std::fs::rename(&staged, hook).map_err(|error| {
        let _ = std::fs::remove_file(&staged);
        format!("{} cannot be written: {error}", hook.display()).into()
    })
}

/// `craft check --install-hook [--apply]`: the pre-commit hook for the project at `root`,
/// previewed, written on `apply`, or the line to add where something else owns the hook.
pub fn install_hook(root: &Path, apply: bool) -> Result<Value, Failure> {
    let root = state::canonical_root(root)?;
    let (top, prefix) = checkout(&root).ok_or(
        "no Git checkout at the project root: install the hook from the project inside a repository (a workspace root runs `craft check` from each member's own hook)",
    )?;
    let git_line = |args: &[&str]| {
        process::git(&top, args, 4096)
            .ok()
            .flatten()
            .and_then(|output| String::from_utf8(output).ok())
            .map(|text| text.trim_end_matches(['\n', '\r']).to_owned())
            .filter(|text| !text.is_empty())
    };
    let hooks_path = git_line(&["config", "--get", "core.hooksPath"]);
    let hook = git_line(&["rev-parse", "--git-path", "hooks/pre-commit"])
        .map(|path| top.join(path))
        .ok_or("Git names no hooks directory for this checkout")?;
    let managers: Vec<&str> = gates::MANAGERS
        .iter()
        .filter(|(_, names)| {
            names
                .iter()
                .any(|name| std::fs::symlink_metadata(top.join(name)).is_ok())
        })
        .map(|(manager, _)| *manager)
        .collect();
    let launcher = launcher()?;
    let command = gates::hook_command(&launcher, &prefix)?;
    let script = gates::hook_script(&launcher, &prefix)?;
    let current = existing(&hook)?;
    let mut plan = gates::hook_plan(
        &managers,
        hooks_path.as_deref(),
        current.as_deref(),
        &script,
        &command,
    );
    let shown = hook.strip_prefix(&top).map_or_else(
        |_| hook.display().to_string(),
        |path| path.display().to_string(),
    );
    plan["path"] = json!(shown.replace('\\', "/"));
    plan["command"] = json!(command);
    let writes = matches!(plan["action"].as_str(), Some("write" | "update"));
    if writes && apply {
        write_hook(&hook, &script)?;
    }
    Ok(json!({"applied": writes && apply, "hook": plan}))
}

/// `craft check --ci-snippet github|gitlab`: the CI job for the project at `root`, pinned to
/// this Craft's release.
pub fn ci_snippet(root: &Path, provider: &str) -> Result<Value, Failure> {
    let root = state::canonical_root(root)?;
    let prefix = checkout(&root)
        .map(|(_, prefix)| prefix)
        .unwrap_or_default();
    let version = crate::telemetry::pack_version().unwrap_or_default();
    let snippet = gates::ci_snippet(provider, &version, &prefix)?;
    Ok(json!({"provider": provider, "version": version, "ciSnippet": snippet}))
}
