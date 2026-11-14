//! The project's safety policy at a hook: the engine judges a shell command against the
//! project root under each spelling it has on disk and the temporary directories.
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
