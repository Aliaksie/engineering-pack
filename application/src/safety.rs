//! The project's safety policy at a hook: the engine judges a shell command against the
//! project root under each spelling it has on disk and the temporary directories, and a
//! tool call that names a secret file.
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
