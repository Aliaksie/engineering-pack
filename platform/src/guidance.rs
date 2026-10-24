//! Guidance files of installed capability packages, measured from explicit bundle roots only.
//! No plugin cache is searched and no symlink is followed; every read is bounded.
use crate::documents::DocumentError;
use crate::files::{ReadError, read_bounded};
use craft_engine::context::Request;
use craft_engine::contracts::kebab;
use craft_engine::routing::private_path;
use serde_json::{Map, Value, json};
use std::collections::HashMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

const MAX_FILE_BYTES: usize = 1024 * 1024;
const MAX_ROOTS: usize = 16;

type Result<T> = std::result::Result<T, DocumentError>;

fn invalid(message: impl Into<String>) -> DocumentError {
    DocumentError::Invalid(message.into())
}

/// `package:skill` with an optional `/references/[dir/]*name.md`.
fn target(text: &str) -> Option<(&str, &str, Option<&str>)> {
    let (package, rest) = text.split_once(':')?;
    let (name, reference) = match rest.find('/') {
        Some(slash) => (&rest[..slash], Some(&rest[slash..])),
        None => (rest, None),
    };
    let reference_valid = reference.is_none_or(|reference| {
        reference
            .strip_prefix("/references/")
            .and_then(|path| path.strip_suffix(".md"))
            .is_some_and(|path| path.split('/').all(kebab))
    });
    (kebab(package) && kebab(name) && reference_valid).then_some((package, name, reference))
}

/// A regular file at `relative` below `root`, or `None` when a component is missing. A symlink
/// anywhere on the path is refused; a component below a regular file is not a directory.
fn local_file(root: &Path, relative: &str) -> Result<Option<PathBuf>> {
    let mut current = root.to_path_buf();
    let mut parts = relative.split('/').peekable();
    while let Some(part) = parts.next() {
        current.push(part);
        let kind = match fs::symlink_metadata(&current) {
            Ok(metadata) => metadata.file_type(),
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(DocumentError::Io(error)),
        };
        if kind.is_symlink() {
            return Err(invalid(format!("symlink in context path: {relative}")));
        }
        // Windows reports a path below a regular file as missing; name it as Unix does.
        if !kind.is_dir() && parts.peek().is_some() {
            return Err(DocumentError::Io(io::ErrorKind::NotADirectory.into()));
        }
    }
    Ok(Some(current))
}

fn read(file: &Path, relative: &str) -> Result<Vec<u8>> {
    read_bounded(file, MAX_FILE_BYTES).map_err(|error| match error {
        ReadError::Io(error) => DocumentError::Io(error),
        ReadError::Bounds => invalid(format!(
            "context file is not a bounded regular file: {relative}"
        )),
        ReadError::Grew => invalid(format!("context file exceeds read limit: {relative}")),
    })
}

#[derive(Clone)]
enum Location {
    Found {
        root: usize,
        prefix: String,
        generated: bool,
    },
    Ambiguous,
    Absent,
}

struct Measure<'a> {
    roots: &'a [PathBuf],
    packages: HashMap<String, Location>,
    entries: Vec<(PathBuf, Map<String, Value>)>,
    missing: Vec<Value>,
}

impl Measure<'_> {
    fn package(&mut self, package: &str) -> Result<Location> {
        if let Some(location) = self.packages.get(package) {
            return Ok(location.clone());
        }
        let mut found = Vec::new();
        for (index, root) in self.roots.iter().enumerate() {
            for prefix in [
                String::new(),
                format!("packages/{package}/"),
                format!("plugins/{package}/"),
            ] {
                for manifest in ["capability.json", "source-capability.json"] {
                    let relative = format!("{prefix}{manifest}");
                    let Some(file) = local_file(root, &relative)? else {
                        continue;
                    };
                    let document: Value = serde_json::from_slice(&read(&file, &relative)?)
                        .map_err(|error| invalid(error.to_string()))?;
                    if document["name"] == package {
                        found.push(Location::Found {
                            root: index,
                            prefix: prefix.clone(),
                            generated: manifest == "source-capability.json",
                        });
                        break;
                    }
                }
            }
        }
        let location = match found.len() {
            0 => Location::Absent,
            1 => found.remove(0),
            _ => Location::Ambiguous,
        };
        self.packages.insert(package.into(), location.clone());
        Ok(location)
    }

    fn record(
        &mut self,
        root: usize,
        relative: &str,
        target: &str,
        selected_by: &str,
    ) -> Result<()> {
        let Some(file) = local_file(&self.roots[root], relative)? else {
            self.missing
                .push(json!({ "target": target, "reason": "file not present in this bundle" }));
            return Ok(());
        };
        if let Some((_, entry)) = self.entries.iter_mut().find(|(path, _)| *path == file) {
            entry["selections"] = json!(entry["selections"].as_u64().unwrap_or(0) + 1);
            let selections = entry["selectedBy"].as_array_mut().expect("selection list");
            if !selections.iter().any(|item| item == selected_by) {
                selections.push(json!(selected_by));
            }
            return Ok(());
        }
        let content = read(&file, relative)?;
        let mut entry = Map::new();
        entry.insert("target".into(), json!(target));
        entry.insert("path".into(), json!(relative));
        if self.roots.len() > 1 {
            entry.insert(
                "bundleRoot".into(),
                json!(self.roots[root].to_string_lossy()),
            );
        }
        entry.insert("bytes".into(), json!(content.len()));
        entry.insert("sha256".into(), json!(craft_engine::hash(&content)));
        entry.insert("estimatedTokens".into(), json!(content.len().div_ceil(4)));
        entry.insert("selections".into(), json!(1));
        entry.insert("selectedBy".into(), json!([selected_by]));
        self.entries.push((file, entry));
        Ok(())
    }
}

/// Measured guidance entries and missing targets for `requests`, from 1 to 16 bundle roots.
pub fn measure(roots: &[String], requests: &[Request]) -> Result<(Vec<Value>, Vec<Value>)> {
    if roots.is_empty()
        || roots.len() > MAX_ROOTS
        || roots.iter().any(|root| root.trim().is_empty())
    {
        return Err(invalid(
            "context reporting requires 1 to 16 explicit bundle roots",
        ));
    }
    let mut physical: Vec<PathBuf> = Vec::new();
    for root in roots {
        let root = crate::state::physical(Path::new(root)).map_err(DocumentError::Io)?;
        if !physical.contains(&root) {
            physical.push(root);
        }
    }
    if physical
        .iter()
        .any(|root| private_path(&root.to_string_lossy().replace('\\', "/")))
    {
        return Err(invalid("private workspace cannot supply guidance"));
    }
    let mut measure = Measure {
        roots: &physical,
        packages: HashMap::new(),
        entries: Vec::new(),
        missing: Vec::new(),
    };
    for request in requests {
        let (package, name, reference) = target(&request.target)
            .ok_or_else(|| invalid(format!("invalid context target: {}", request.target)))?;
        let (root, prefix, generated) = match measure.package(package)? {
            Location::Found {
                root,
                prefix,
                generated,
            } => (root, prefix, generated),
            Location::Absent => {
                measure.missing.push(json!({ "target": request.target, "reason": "package not present in supplied bundle roots" }));
                continue;
            }
            Location::Ambiguous => {
                measure.missing.push(json!({ "target": request.target, "reason": "package present in multiple supplied roots; select one installation" }));
                continue;
            }
        };
        let tail = reference.unwrap_or("/SKILL.md");
        let plain = format!("skills/{name}{tail}");
        let relative = if generated {
            let namespaced = format!("skills/{package}-{name}{tail}");
            if local_file(&physical[root], &format!("{prefix}{namespaced}"))?.is_some() {
                namespaced
            } else {
                plain
            }
        } else {
            match name.strip_prefix("workflow-") {
                Some(workflow) if reference.is_none() => format!("workflows/{workflow}.md"),
                _ => plain,
            }
        };
        measure.record(
            root,
            &format!("{prefix}{relative}"),
            &request.target,
            &request.selected_by,
        )?;
        if reference.is_none() {
            measure.record(
                root,
                &format!("{prefix}AGENTS.md"),
                &format!("{package}:instructions"),
                &request.selected_by,
            )?;
        }
    }
    let entries = measure
        .entries
        .into_iter()
        .map(|(_, entry)| Value::Object(entry))
        .collect();
    Ok((entries, measure.missing))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(0);

    struct Bundle(PathBuf);
    impl Bundle {
        fn new(files: &[(&str, &str)]) -> Self {
            let root = std::env::temp_dir().join(format!(
                "craft-guidance-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            for (file, text) in files {
                let path = root.join(file);
                fs::create_dir_all(path.parent().unwrap()).unwrap();
                fs::write(path, text).unwrap();
            }
            Self(root)
        }
        fn root(&self) -> Vec<String> {
            vec![self.0.display().to_string()]
        }
    }
    impl Drop for Bundle {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn request(target: &str, selected_by: &str) -> Request {
        Request {
            target: target.into(),
            selected_by: selected_by.into(),
        }
    }

    const PACKAGE: (&str, &str) = ("packages/p/capability.json", r#"{"name":"p"}"#);

    #[test]
    fn one_package_directory_with_both_manifests_is_one_installation() {
        let bundle = Bundle::new(&[
            PACKAGE,
            ("packages/p/source-capability.json", r#"{"name":"p"}"#),
            ("packages/p/skills/a/SKILL.md", "a"),
            ("packages/p/AGENTS.md", "i"),
        ]);
        let (entries, missing) = measure(&bundle.root(), &[request("p:a", "workflow:x")]).unwrap();
        assert_eq!(entries.len(), 2);
        assert!(missing.is_empty());
    }

    #[test]
    fn shared_instructions_count_each_selection_once_per_selector() {
        let bundle = Bundle::new(&[
            PACKAGE,
            ("packages/p/skills/a/SKILL.md", "a"),
            ("packages/p/skills/b/SKILL.md", "b"),
            ("packages/p/AGENTS.md", "i"),
        ]);
        let (entries, _) = measure(
            &bundle.root(),
            &[
                request("p:a", "practice:x"),
                request("p:b", "practice:x"),
                request("p:b", "practice:y"),
            ],
        )
        .unwrap();
        let instructions = entries
            .iter()
            .find(|entry| entry["target"] == "p:instructions")
            .unwrap();
        assert_eq!(instructions["selections"], 3);
        assert_eq!(
            instructions["selectedBy"],
            json!(["practice:x", "practice:y"])
        );
    }

    #[test]
    fn workflow_skills_map_to_workflow_files_but_their_references_stay_under_skills() {
        let bundle = Bundle::new(&[
            PACKAGE,
            ("packages/p/workflows/run.md", "w"),
            ("packages/p/skills/workflow-run/references/r.md", "r"),
            ("packages/p/AGENTS.md", "i"),
        ]);
        let (entries, missing) = measure(
            &bundle.root(),
            &[
                request("p:workflow-run", "workflow:run"),
                request("p:workflow-run/references/r.md", "workflow:run"),
            ],
        )
        .unwrap();
        let paths: Vec<&Value> = entries.iter().map(|entry| &entry["path"]).collect();
        assert_eq!(
            paths,
            [
                &json!("packages/p/workflows/run.md"),
                &json!("packages/p/AGENTS.md"),
                &json!("packages/p/skills/workflow-run/references/r.md")
            ]
        );
        assert!(missing.is_empty());
    }

    #[test]
    fn private_roots_and_invalid_targets_are_refused() {
        let bundle = Bundle::new(&[(
            ".craft/state/x/packages/p/capability.json",
            r#"{"name":"p"}"#,
        )]);
        let private = vec![bundle.0.join(".craft/state/x").display().to_string()];
        assert!(
            measure(&private, &[])
                .unwrap_err()
                .to_string()
                .contains("private workspace")
        );
        let bundle = Bundle::new(&[PACKAGE]);
        assert!(
            measure(&bundle.root(), &[request("p:../secrets", "x")])
                .unwrap_err()
                .to_string()
                .contains("invalid context target")
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_symlinked_path_component_is_refused() {
        let bundle = Bundle::new(&[
            PACKAGE,
            ("outside/SKILL.md", "o"),
            ("packages/p/AGENTS.md", "i"),
        ]);
        fs::create_dir_all(bundle.0.join("packages/p/skills")).unwrap();
        std::os::unix::fs::symlink(
            bundle.0.join("outside"),
            bundle.0.join("packages/p/skills/a"),
        )
        .unwrap();
        let error = measure(&bundle.root(), &[request("p:a", "x")]).unwrap_err();
        assert!(error.to_string().contains("symlink in context path"));
    }
}
