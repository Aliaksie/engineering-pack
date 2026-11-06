//! The imports a written source file adds, resolved to project paths for the module
//! dependency check. Only imports absent from the file's `HEAD` version count, so the check
//! reports what the change introduced, not existing debt. Resolution is lexical or reads
//! the declared modules' own manifests (`package.json`, `Cargo.toml`, `go.mod`); an import it
//! cannot place in the project stays unresolved and is never a finding.
use craft_engine::standards::{self, Import};
use serde_json::Value;
use std::collections::HashSet;
use std::path::Path;

/// How large a manifest or a `HEAD` version this reads; larger ones are left unread.
const MAX_BYTES: usize = 512 * 1024;
/// How many new imports one written file has resolved; the rest are left unchecked.
const MAX_IMPORTS: usize = 64;

/// Module dependency results for `file`, just written with `bytes`; empty when no standard
/// asks for the check there or every new import is allowed.
pub fn verdicts(root: &Path, settings: &Value, file: &str, bytes: &[u8]) -> Vec<Value> {
    let checks = standards::import_checks(settings, file);
    if checks.is_empty() || !standards::guards_dependencies(settings, file) {
        return Vec::new();
    }
    let current = imports(file, bytes);
    if current.is_empty() {
        return Vec::new();
    }
    let base =
        match craft_platform::process::git(root, &["show", &format!("HEAD:./{file}")], MAX_BYTES) {
            Ok(Some(old)) => imports(file, &old)
                .into_iter()
                .map(|(_, target, _)| target)
                .collect(),
            // Not in `HEAD` (a new file) or no Git: every import is new.
            Ok(None) => HashSet::new(),
            // Too large to compare: unchecked rather than reported as new.
            Err(_) => return Vec::new(),
        };
    let mut seen = HashSet::new();
    let mut manifests = None;
    let added: Vec<Import> = current
        .into_iter()
        .filter(|(_, target, _)| !base.contains(target) && seen.insert(target.clone()))
        .take(MAX_IMPORTS)
        .map(|(line, target, provider)| {
            let manifests = manifests.get_or_insert_with(|| Manifests::read(root, settings));
            let resolved = resolve(root, settings, manifests, file, &provider, &target);
            Import {
                line,
                target,
                resolved,
            }
        })
        .collect();
    standards::dependency_verdicts(settings, &checks, file, &added)
}

/// `(line, target, provider)` for each module import in the source: not the names a named
/// import binds, and nothing for a file the syntax providers do not read.
fn imports(file: &str, bytes: &[u8]) -> Vec<(usize, String, String)> {
    let Ok(extraction) = craft_platform::providers::extract(file, bytes) else {
        return Vec::new();
    };
    extraction
        .evidence
        .into_iter()
        .filter_map(|evidence| {
            let (provider, kind) = evidence.kind.split_once(':')?;
            if !matches!(kind, "imports" | "imports-static" | "dynamic-import") {
                return None;
            }
            Some((evidence.line, evidence.target?, provider.to_owned()))
        })
        .collect()
}

/// Package names the declared modules publish, each with the module path.
struct Manifests {
    /// `package.json` `name` → directory.
    packages: Vec<(String, String)>,
    /// `Cargo.toml` package name, with `-` as `_` → directory.
    crates: Vec<(String, String)>,
    /// `go.mod` module path → directory holding it.
    go: Vec<(String, String)>,
}

impl Manifests {
    fn read(root: &Path, settings: &Value) -> Self {
        let mut manifests = Self {
            packages: Vec::new(),
            crates: Vec::new(),
            go: Vec::new(),
        };
        let paths = settings["modules"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|module| module["path"].as_str())
            .filter_map(|path| normalize(&[], path));
        for path in paths {
            let dir = |name: &str| {
                if path.is_empty() {
                    name.to_owned()
                } else {
                    format!("{path}/{name}")
                }
            };
            if let Some(name) = read(root, &dir("package.json"))
                .and_then(|text| serde_json::from_str::<Value>(&text).ok())
                .and_then(|json| json["name"].as_str().map(str::to_owned))
            {
                manifests.packages.push((name, path.clone()));
            }
            if let Some(name) = read(root, &dir("Cargo.toml")).and_then(|text| cargo_name(&text)) {
                manifests
                    .crates
                    .push((name.replace('-', "_"), path.clone()));
            }
            // The nearest `go.mod` at or above the module path.
            let mut at = path.clone();
            loop {
                let file = if at.is_empty() {
                    "go.mod".to_owned()
                } else {
                    format!("{at}/go.mod")
                };
                if let Some(name) = read(root, &file).and_then(|text| go_module(&text)) {
                    if !manifests.go.iter().any(|(_, dir)| *dir == at) {
                        manifests.go.push((name, at));
                    }
                    break;
                }
                match at.rfind('/') {
                    Some(index) => at.truncate(index),
                    None if !at.is_empty() => at.clear(),
                    None => break,
                }
            }
        }
        manifests
    }
}

fn read(root: &Path, file: &str) -> Option<String> {
    let path = root.join(file);
    let meta = std::fs::symlink_metadata(&path).ok()?;
    if !meta.is_file() || meta.len() > MAX_BYTES as u64 {
        return None;
    }
    std::fs::read_to_string(path).ok()
}

/// `name` under `[package]` in a `Cargo.toml`.
fn cargo_name(text: &str) -> Option<String> {
    let mut package = false;
    for line in text.lines().map(str::trim) {
        if line.starts_with('[') {
            package = line == "[package]";
        } else if package
            && let Some((key, value)) = line.split_once('=')
            && key.trim() == "name"
        {
            return Some(value.trim().trim_matches('"').to_owned()).filter(|n| !n.is_empty());
        }
    }
    None
}

/// The `module` directive of a `go.mod`.
fn go_module(text: &str) -> Option<String> {
    text.lines()
        .map(str::trim)
        .find_map(|line| line.strip_prefix("module "))
        .map(|name| name.trim().trim_matches('"').to_owned())
        .filter(|name| !name.is_empty())
}

/// `relative` joined onto the directory `base` (components), `.` and `..` applied; `None` when
/// it leaves the project or is absolute.
fn normalize(base: &[&str], relative: &str) -> Option<String> {
    if relative.starts_with('/') || relative.contains('\\') || relative.contains(':') {
        return None;
    }
    let mut parts: Vec<&str> = base.to_vec();
    for part in relative.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop()?;
            }
            part => parts.push(part),
        }
    }
    Some(parts.join("/"))
}

fn directory(file: &str) -> Vec<&str> {
    let mut parts: Vec<&str> = file.split('/').collect();
    parts.pop();
    parts
}

/// The project path `target`, imported from `file`, names; `None` when it cannot be placed.
fn resolve(
    root: &Path,
    settings: &Value,
    manifests: &Manifests,
    file: &str,
    provider: &str,
    target: &str,
) -> Option<String> {
    match provider {
        "js" | "ts" => {
            if target.starts_with('.') {
                return normalize(&directory(file), target);
            }
            manifests.packages.iter().find_map(|(name, path)| {
                let rest = target.strip_prefix(name.as_str())?;
                (rest.is_empty() || rest.starts_with('/')).then(|| path.clone())
            })
        }
        "python" => {
            let dots = target.bytes().take_while(|b| *b == b'.').count();
            let rest = target[dots..].replace('.', "/");
            if dots > 0 {
                let mut base = directory(file);
                for _ in 1..dots {
                    base.pop()?;
                }
                return normalize(&base, &rest);
            }
            let candidates = module_paths(settings)
                .into_iter()
                .flat_map(|module| {
                    ["", "src/"].map(|source| {
                        let prefix = if module.is_empty() {
                            source.to_owned()
                        } else {
                            format!("{module}/{source}")
                        };
                        format!("{prefix}{rest}")
                    })
                })
                .flat_map(|path| [format!("{path}.py"), path])
                .filter(|path| root.join(path).exists())
                .collect();
            unique(settings, candidates)
        }
        "go" => manifests.go.iter().find_map(|(name, dir)| {
            let rest = target.strip_prefix(name.as_str())?;
            if !rest.is_empty() && !rest.starts_with('/') {
                return None;
            }
            let base: Vec<&str> = dir.split('/').filter(|p| !p.is_empty()).collect();
            normalize(&base, rest.trim_start_matches('/'))
        }),
        "rust" => {
            let first = target.trim_start_matches("::").split("::").next()?;
            manifests
                .crates
                .iter()
                .find(|(name, _)| name == first)
                .map(|(_, path)| path.clone())
        }
        "java" => {
            let segments: Vec<&str> = target.split('.').collect();
            let pattern = match segments.split_last() {
                Some((&"*", package)) if !package.is_empty() => {
                    format!(":(glob)**/{}/*.java", package.join("/"))
                }
                _ if segments.len() > 1 => format!(":(glob)**/{}.java", segments.join("/")),
                _ => return None,
            };
            // A static import names a member: its class is one segment up.
            let class = (segments.len() > 2).then(|| {
                format!(
                    ":(glob)**/{}.java",
                    segments[..segments.len() - 1].join("/")
                )
            });
            let mut args = vec![
                "ls-files",
                "--cached",
                "--others",
                "--exclude-standard",
                "-z",
                "--",
            ];
            args.push(&pattern);
            if let Some(class) = &class {
                args.push(class);
            }
            let output = craft_platform::process::git(root, &args, 64 * 1024).ok()??;
            let files = String::from_utf8_lossy(&output)
                .split('\0')
                .filter(|path| !path.is_empty())
                .map(str::to_owned)
                .collect();
            unique(settings, files)
        }
        _ => None,
    }
}

fn module_paths(settings: &Value) -> Vec<String> {
    settings["modules"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|module| module["path"].as_str())
        .filter_map(|path| normalize(&[], path))
        .collect()
}

/// The first of `paths` when all of them lie in one declared module.
fn unique(settings: &Value, paths: Vec<String>) -> Option<String> {
    let first = paths.first()?.clone();
    let owners = craft_engine::routing::matching_modules(settings, &paths);
    (owners.len() == 1).then_some(first)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// A fresh directory outside any Git worktree, holding `files`.
    fn project(name: &str, files: &[(&str, &str)]) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("craft-deps-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        for (path, text) in files {
            let path = dir.join(path);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, text).unwrap();
        }
        dir
    }

    fn settings(modules: Value) -> Value {
        json!({
            "modules": modules,
            "standards": [{"id": "rings", "anchor": "hexagonal-architecture"}],
        })
    }

    fn rules(verdicts: &[Value]) -> Vec<(u64, String)> {
        verdicts
            .iter()
            .flat_map(|verdict| verdict["findings"].as_array().unwrap().clone())
            .map(|f| {
                (
                    f["line"].as_u64().unwrap(),
                    f["message"].as_str().unwrap().to_owned(),
                )
            })
            .collect()
    }

    #[test]
    fn relative_typescript_import_into_an_undeclared_module_is_reported() {
        let dir = project("ts", &[]);
        let settings = settings(json!([
            {"name": "domain", "path": "src/domain", "dependsOn": []},
            {"name": "adapters", "path": "src/adapters", "dependsOn": ["domain"]},
        ]));
        let source = b"import { Order } from './order'\nimport { Db } from '../adapters/db'\nimport x from 'lodash'\n";
        let found = verdicts(&dir, &settings, "src/domain/service.ts", source);
        assert_eq!(
            rules(&found),
            [(
                2,
                "imports ../adapters/db from module adapters; module domain does not declare dependsOn adapters".to_owned()
            )]
        );
        // The adapter may import the domain.
        let inward = b"import { Order } from '../domain/order'\n";
        assert!(verdicts(&dir, &settings, "src/adapters/db.ts", inward).is_empty());
    }

    #[test]
    fn modules_without_depends_on_are_not_checked() {
        let dir = project("partial", &[]);
        let settings = settings(json!([
            {"name": "domain", "path": "src/domain"},
            {"name": "adapters", "path": "src/adapters"},
        ]));
        let source = b"import { Db } from '../adapters/db'\n";
        assert!(verdicts(&dir, &settings, "src/domain/service.ts", source).is_empty());
    }

    #[test]
    fn rust_crates_resolve_by_package_name() {
        let dir = project(
            "rust",
            &[
                ("core/Cargo.toml", "[package]\nname = \"shop-core\"\n"),
                (
                    "web/Cargo.toml",
                    "[package]\nname = \"shop-web\"\n[dependencies]\nname = \"x\"\n",
                ),
            ],
        );
        let settings = settings(json!([
            {"name": "core", "path": "core", "dependsOn": []},
            {"name": "web", "path": "web", "dependsOn": ["core"]},
        ]));
        let source = b"use crate::model;\nuse std::io;\nuse shop_web::routes::Router;\n";
        assert_eq!(
            rules(&verdicts(&dir, &settings, "core/src/lib.rs", source)).len(),
            1
        );
        let inward = b"use shop_core::model::Order;\n";
        assert!(verdicts(&dir, &settings, "web/src/main.rs", inward).is_empty());
    }

    #[test]
    fn go_and_python_resolve_within_the_project() {
        let dir = project(
            "gopy",
            &[
                ("go.mod", "module example.com/shop\n\ngo 1.22\n"),
                ("py/infra/db.py", ""),
            ],
        );
        let settings = settings(json!([
            {"name": "domain", "path": "internal/domain", "dependsOn": []},
            {"name": "infra", "path": "internal/infra", "dependsOn": ["domain"]},
            {"name": "pydomain", "path": "py/domain", "dependsOn": []},
            {"name": "pyinfra", "path": "py/infra"},
        ]));
        let go =
            b"package domain\n\nimport (\n\t\"fmt\"\n\t\"example.com/shop/internal/infra\"\n)\n";
        assert_eq!(
            rules(&verdicts(&dir, &settings, "internal/domain/order.go", go)).len(),
            1
        );
        let python = b"from ..infra import db\nimport os\n";
        assert_eq!(
            rules(&verdicts(&dir, &settings, "py/domain/order.py", python)).len(),
            1
        );
    }

    #[test]
    fn unresolved_imports_and_paths_outside_the_project_are_quiet() {
        assert_eq!(normalize(&["src"], "../../x"), None);
        assert_eq!(normalize(&["src", "a"], "../b/./c"), Some("src/b/c".into()));
        assert_eq!(cargo_name("[workspace]\nname = \"x\"\n"), None);
        assert_eq!(
            go_module("module   example.com/m\n"),
            Some("example.com/m".into())
        );
    }
}
