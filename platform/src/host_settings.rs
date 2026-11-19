//! Consumer-owned Claude settings that Craft writes only on an explicit request: the display
//! field, and deny rules merged into the project's local settings. Allow and ask rules are
//! never written here.
use crate::{Error, documents, files, state};
use serde_json::{Value, json};
use std::{
    fs,
    io::ErrorKind,
    path::{Path, PathBuf},
};
const BYTES: usize = 65_536;

fn target(root: &Path, create: bool) -> Result<PathBuf, Error> {
    let root = state::canonical_root(root)?;
    let directory = root.join(".claude");
    if create {
        files::create_private_directory(&directory)?;
    }
    match fs::symlink_metadata(&directory) {
        Ok(meta) if !meta.is_dir() || meta.file_type().is_symlink() => {
            return Err("Claude settings directory must be a real directory".into());
        }
        Err(error) if error.kind() != ErrorKind::NotFound => return Err(error.into()),
        _ => {}
    }
    Ok(directory.join("settings.local.json"))
}

fn read(file: &Path) -> Result<Value, Error> {
    let value = match documents::input(file) {
        Ok(value) => value,
        Err(error) if error.io_kind() == Some(ErrorKind::NotFound) => json!({}),
        Err(error) => return Err(error.to_string().into()),
    };
    if !value.is_object() {
        return Err("Claude settings must be a JSON object".into());
    }
    Ok(value)
}

// Highest-precedence persistent consumer settings first. Session flags and managed
// host policy cannot be inferred by this standalone setup command.
fn snapshot(file: &Path) -> Result<Vec<(PathBuf, Value)>, Error> {
    let directory = file.parent().expect("settings directory");
    let mut paths = vec![file.to_path_buf(), directory.join("settings.json")];
    let user_directory = std::env::var_os("CLAUDE_CONFIG_DIR")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .or_else(|| std::env::home_dir().map(|home| home.join(".claude")));
    if let Some(user_directory) = user_directory {
        paths.push(user_directory.join("settings.json"));
    }
    paths
        .into_iter()
        .map(|path| {
            let settings = read(&path)?;
            Ok((path, settings))
        })
        .collect()
}

fn effective_display(settings: &[(PathBuf, Value)]) -> Option<(&Path, &Value)> {
    settings.iter().find_map(|(path, value)| {
        value
            .get("statusLine")
            .map(|display| (path.as_path(), display))
    })
}

pub fn inspect(root: &Path) -> Result<Value, Error> {
    let file = target(root, false)?;
    let settings = snapshot(&file)?;
    let current = effective_display(&settings);
    Ok(
        json!({"file":file,"statusLine":current.map(|(_, value)| value),
        "source":current.map(|(path, _)| path)}),
    )
}

/// Explicit application of the display field only. Refuse replacement of a custom display.
/// The lock serializes Craft writers; the snapshot check also detects intervening edits.
pub fn apply(root: &Path, display: &Value) -> Result<Value, Error> {
    let file = target(root, true)?;
    let Some(_lock) = state::lock(&file)? else {
        return Err(Error::busy("Claude settings writer is busy"));
    };
    let before = snapshot(&file)?;
    if let Some((_, existing)) = effective_display(&before) {
        if existing == display {
            return Ok(json!({"status":"already-configured","file":file}));
        }
        return Err(
            "Claude already has a statusLine; merge the preview manually to preserve your display"
                .into(),
        );
    }
    let mut after = before[0].1.clone();
    after["statusLine"] = display.clone();
    if snapshot(&file)? != before {
        return Err("Claude settings changed during setup; preview again".into());
    }
    state::write(&file, &after, BYTES, true)?;
    Ok(
        json!({"status":"configured-unverified","file":file,"next":"Run an ordinary Claude turn and verify the context display; configuration alone does not prove native dispatch."}),
    )
}

/// Explicit merge of `rules` into `permissions.deny` of the project's local Claude settings.
/// Every other key and rule is kept and a rule already listed is not added again; a
/// `permissions` that is not an object or a `deny` that is not a list is refused unchanged.
pub fn deny(root: &Path, rules: &[&str]) -> Result<Value, Error> {
    let file = target(root, true)?;
    let Some(_lock) = state::lock(&file)? else {
        return Err(Error::busy("Claude settings writer is busy"));
    };
    let before = read(&file)?;
    let mut after = before.clone();
    let permissions = after
        .as_object_mut()
        .expect("settings object")
        .entry("permissions")
        .or_insert_with(|| json!({}));
    let Some(permissions) = permissions.as_object_mut() else {
        return Err(format!(
            "{}: `permissions` is not an object; add the rules by hand",
            file.display()
        )
        .into());
    };
    let deny = permissions.entry("deny").or_insert_with(|| json!([]));
    let Some(deny) = deny.as_array_mut() else {
        return Err(format!(
            "{}: `permissions.deny` is not a list; add the rules by hand",
            file.display()
        )
        .into());
    };
    let added: Vec<&str> = rules
        .iter()
        .copied()
        .filter(|rule| !deny.iter().any(|kept| kept == rule))
        .collect();
    if added.is_empty() {
        return Ok(json!({"status":"already-configured","file":file,"added":[]}));
    }
    deny.extend(added.iter().map(|rule| json!(rule)));
    if read(&file)? != before {
        return Err("Claude settings changed while doctor wrote them; run it again".into());
    }
    state::write(&file, &after, BYTES, true)?;
    Ok(json!({"status":"written","file":file,"added":added}))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(0);
    const RULES: [&str; 2] = ["Read(**/.env)", "Read(**/*.key)"];

    struct Fixture(PathBuf);
    impl Fixture {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!(
                "craft-host-settings-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir(&path).unwrap();
            Self(state::canonical_root(&path).unwrap())
        }
        fn local(&self) -> PathBuf {
            self.0.join(".claude/settings.local.json")
        }
        fn settings(&self) -> Value {
            serde_json::from_slice(&fs::read(self.local()).unwrap()).unwrap()
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn deny_rules_are_merged_keeping_every_other_key_and_rule() {
        let fixture = Fixture::new();
        fs::create_dir(fixture.0.join(".claude")).unwrap();
        let kept = json!({
            "statusLine": {"type": "command", "command": "x"},
            "permissions": {"allow": ["Bash(npm test)"], "ask": ["Bash(git push:*)"],
                "deny": ["Bash(rm:*)", "Read(**/.env)"], "defaultMode": "acceptEdits"}
        });
        fs::write(fixture.local(), kept.to_string()).unwrap();

        let written = deny(&fixture.0, &RULES).unwrap();
        assert_eq!(written["status"], "written");
        assert_eq!(written["added"], json!(["Read(**/*.key)"]));
        let mut expected = kept.clone();
        expected["permissions"]["deny"] = json!(["Bash(rm:*)", "Read(**/.env)", "Read(**/*.key)"]);
        assert_eq!(fixture.settings(), expected);

        let again = deny(&fixture.0, &RULES).unwrap();
        assert_eq!(again["status"], "already-configured");
        assert_eq!(fixture.settings(), expected);
    }

    #[test]
    fn deny_rules_create_the_local_settings_when_there_are_none() {
        let fixture = Fixture::new();
        assert_eq!(deny(&fixture.0, &RULES).unwrap()["status"], "written");
        assert_eq!(fixture.settings(), json!({"permissions": {"deny": RULES}}));
    }

    #[test]
    fn deny_rules_refuse_settings_of_another_shape_and_leave_them_as_they_were() {
        let fixture = Fixture::new();
        fs::create_dir(fixture.0.join(".claude")).unwrap();
        for shape in [
            json!({"permissions": {"deny": "Read(**/.env)"}}),
            json!({"permissions": ["Read(**/.env)"]}),
        ] {
            fs::write(fixture.local(), shape.to_string()).unwrap();
            let before = fs::read(fixture.local()).unwrap();
            assert!(deny(&fixture.0, &RULES).is_err());
            assert_eq!(fs::read(fixture.local()).unwrap(), before);
        }
        fs::write(fixture.local(), "[]").unwrap();
        assert!(deny(&fixture.0, &RULES).is_err());
        assert_eq!(fs::read(fixture.local()).unwrap(), b"[]");
    }
}
