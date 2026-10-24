//! One bounded literal scan over a Git inventory or explicit paths.
use crate::{process, state};
use craft_engine::{
    discovery::{self, MAX_FILE_BYTES, Search},
    routing::private_path,
};
use serde_json::{Value, json};
use std::{
    collections::BTreeSet,
    fs::{self, File},
    io::Read,
    path::{Path, PathBuf},
};

#[derive(Debug)]
pub enum SearchError {
    Io(std::io::Error),
    Invalid(String),
}
impl From<std::io::Error> for SearchError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}
impl From<String> for SearchError {
    fn from(message: String) -> Self {
        Self::Invalid(message)
    }
}
impl From<&str> for SearchError {
    fn from(message: &str) -> Self {
        Self::Invalid(message.into())
    }
}
impl std::fmt::Display for SearchError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(error) => error.fmt(f),
            Self::Invalid(message) => f.write_str(message),
        }
    }
}
impl SearchError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::Io(error) => match error.kind() {
                std::io::ErrorKind::NotFound => "ENOENT",
                std::io::ErrorKind::PermissionDenied => "EACCES",
                std::io::ErrorKind::NotADirectory => "ENOTDIR",
                _ => "SEARCH_FAILED",
            },
            Self::Invalid(_) => "INVALID_INPUT",
        }
    }
}

const OMIT: &[&str] = &[".git", "node_modules", "dist"];
const LIMITATIONS: &[&str] = &[
    "Case-sensitive literal matches are navigation evidence, not semantic definitions.",
    "No match is not proof of absence: ignores, scopes, skips and bounds restrict coverage.",
    "Symlinks, generated directories and binary or oversized files are excluded; this is not a sandbox against concurrent filesystem changes.",
];

fn safe_file(root: &Path, relative: &str) -> Result<Option<PathBuf>, SearchError> {
    let mut current = root.to_path_buf();
    for part in relative.split('/') {
        current.push(part);
        if fs::symlink_metadata(&current)?.file_type().is_symlink() {
            return Ok(None);
        }
    }
    Ok(Some(current))
}

/// Refuse a final symlink or special-file replacement at open. Directory replacement
/// races remain outside this path-based traversal contract.
fn open_source(path: &Path) -> std::io::Result<File> {
    #[cfg(unix)]
    let file = {
        use rustix::fs::{Mode, OFlags, open};
        File::from(open(
            path,
            OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::NONBLOCK | OFlags::CLOEXEC,
            Mode::empty(),
        )?)
    };
    #[cfg(windows)]
    let file = {
        use std::os::windows::fs::OpenOptionsExt;
        fs::OpenOptions::new()
            .read(true)
            .custom_flags(0x00200000)
            .open(path)?
    };
    #[cfg(not(any(unix, windows)))]
    let file = File::open(path)?;
    let opened = file.metadata()?;
    if !opened.is_file() || opened.file_type().is_symlink() {
        return Err(std::io::Error::other("source is not a regular file"));
    }
    Ok(file)
}

struct Scan<'a> {
    request: &'a Search,
    files: BTreeSet<String>,
    truncation: Vec<&'static str>,
    skipped: Value,
    entries: usize,
}
impl Scan<'_> {
    fn truncate(&mut self, reason: &'static str) {
        if !self.truncation.contains(&reason) {
            self.truncation.push(reason);
        }
    }
    fn skip(&mut self, reason: &str) {
        self.skipped[reason] = json!(self.skipped[reason].as_u64().unwrap_or(0) + 1);
    }
    fn add(&mut self, name: String) {
        if private_path(&name)
            || !self.request.scopes.iter().any(|entry| {
                entry == "." || name == *entry || name.starts_with(&format!("{entry}/"))
            }) && !self.request.scopes.is_empty()
            || name.split('/').any(|part| OMIT.contains(&part))
        {
            return;
        }
        if self.files.len() >= self.request.max_files && !self.files.contains(&name) {
            self.truncate("maxFiles");
            return;
        }
        self.files.insert(name);
    }
    fn visit(&mut self, root: &Path, name: &str) -> Result<(), SearchError> {
        if private_path(name) {
            return Ok(());
        }
        self.entries += 1;
        if self.entries > self.request.max_files * 10 {
            self.truncate("traversalEntries");
            return Ok(());
        }
        let Some(absolute) = safe_file(root, name)? else {
            self.skip("symlink");
            return Ok(());
        };
        let stat = fs::metadata(&absolute)?;
        if stat.is_file() {
            self.add(name.to_string());
            return Ok(());
        }
        if !stat.is_dir() {
            return Ok(());
        }
        let mut children = Vec::new();
        for child in fs::read_dir(&absolute)? {
            let child = child?;
            if children.len() >= self.request.max_files * 10 - self.entries {
                self.truncate("traversalEntries");
                break;
            }
            children.push(child.file_name().to_string_lossy().into_owned());
        }
        children.sort_by(|a, b| a.encode_utf16().cmp(b.encode_utf16()));
        for child in children {
            if OMIT.contains(&child.as_str()) {
                self.skip("excludedDirectory");
                continue;
            }
            if self.entries >= self.request.max_files * 10 {
                self.truncate("traversalEntries");
                break;
            }
            self.visit(
                root,
                &if name == "." {
                    child
                } else {
                    format!("{name}/{child}")
                },
            )?;
        }
        Ok(())
    }
}

fn inventory(root: &Path) -> Result<Option<Vec<u8>>, String> {
    process::git(
        root,
        &[
            "ls-files",
            "--cached",
            "--others",
            "--exclude-standard",
            "-z",
        ],
        2_097_152,
    )
}

fn selected_inventory(root: &Path, repositories: &[String]) -> Result<Vec<u8>, SearchError> {
    let mut paths = Vec::new();
    let mut seen = BTreeSet::new();
    for scope in repositories {
        let candidate = root.join(scope);
        let Some(_) = safe_file(root, scope)? else {
            return Err("repository scope must not traverse symlinks".into());
        };
        let canonical = state::canonical_root(&candidate)?;
        if !seen.insert(canonical.clone()) {
            return Err("duplicate repository root".into());
        }
        let top = process::git(&canonical, &["rev-parse", "--show-toplevel"], 16384)?
            .ok_or_else(|| format!("repository scope is not a Git worktree root: {scope}"))?;
        let top = String::from_utf8_lossy(&top);
        if state::canonical_root(Path::new(top.trim()))? != canonical {
            return Err(format!("repository scope is not a Git worktree root: {scope}").into());
        }
        let output = inventory(&canonical)?
            .ok_or_else(|| format!("Selected repository inventory failed: {scope}"))?;
        for name in output.split(|b| *b == 0).filter(|p| !p.is_empty()) {
            if scope != "." {
                paths.extend_from_slice(scope.as_bytes());
                paths.push(b'/');
            }
            paths.extend_from_slice(name);
            paths.push(0);
        }
    }
    Ok(paths)
}

pub fn scan(
    root: &Path,
    request: &Search,
    repositories: Option<&[String]>,
) -> Result<Value, SearchError> {
    let root = fs::canonicalize(root)?;
    for scope in &request.scopes {
        if safe_file(&root, scope)?.is_none() {
            return Err(format!("Symlink scope is not supported: {scope}").into());
        }
    }
    let output = match repositories {
        Some(roots) => Some(selected_inventory(&root, roots)?),
        None => inventory(&root)?,
    };
    let git_inventory = output.is_some();
    if !git_inventory && request.scopes.is_empty() {
        return Err("Without a Git inventory, explicit paths are required".into());
    }
    let mut scan = Scan {
        request,
        files: BTreeSet::new(),
        truncation: Vec::new(),
        skipped: json!({"symlink":0,"binary":0,"oversized":0,"unreadable":0,"excludedDirectory":0}),
        entries: 0,
    };
    if let Some(output) = output {
        let text = String::from_utf8_lossy(&output);
        let mut names: Vec<_> = text.split('\0').filter(|p| !p.is_empty()).collect();
        names.sort_by(|a, b| a.encode_utf16().cmp(b.encode_utf16()));
        for name in names {
            scan.add(name.to_string());
        }
    } else {
        for scope in &request.scopes {
            scan.visit(&root, scope)?;
        }
    }
    let mut matches = Vec::new();
    let mut bytes_read = 0;
    let mut files_scanned = 0;
    let mut files: Vec<_> = scan.files.iter().cloned().collect();
    files.sort_by(|a, b| a.encode_utf16().cmp(b.encode_utf16()));
    for name in files {
        if matches.len() >= request.max_matches {
            scan.truncate("maxMatches");
            break;
        }
        let read = (|| -> Result<Option<Vec<u8>>, String> {
            let Some(absolute) = safe_file(&root, &name).map_err(|e| e.to_string())? else {
                scan.skip("symlink");
                return Ok(None);
            };
            let stat = fs::metadata(&absolute).map_err(|e| e.to_string())?;
            if !stat.is_file() {
                return Ok(None);
            }
            if stat.len() > MAX_FILE_BYTES as u64 {
                scan.skip("oversized");
                return Ok(None);
            }
            if stat.len() > (request.max_bytes - bytes_read) as u64 {
                scan.truncate("maxBytes");
                return Ok(None);
            }
            let file = open_source(&absolute).map_err(|e| e.to_string())?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::MetadataExt;
                let opened = file.metadata().map_err(|e| e.to_string())?;
                if stat.dev() != opened.dev() || stat.ino() != opened.ino() {
                    return Err("source changed at open".into());
                }
            }
            let mut bytes = Vec::new();
            file.take(stat.len())
                .read_to_end(&mut bytes)
                .map_err(|e| e.to_string())?;
            Ok(Some(bytes))
        })();
        let bytes = match read {
            Ok(Some(bytes)) => bytes,
            Ok(None) => continue,
            Err(_) => {
                scan.skip("unreadable");
                continue;
            }
        };
        bytes_read += bytes.len();
        if bytes.contains(&0) {
            scan.skip("binary");
            continue;
        }
        files_scanned += 1;
        matches.extend(discovery::matches(
            &String::from_utf8_lossy(&bytes),
            &name,
            &request.terms,
            request.max_matches - matches.len(),
        ));
        if matches.len() >= request.max_matches {
            scan.truncate("maxMatches");
        }
    }
    Ok(
        json!({"inventory":if git_inventory { "git" } else { "explicit-paths" },"terms":request.terms,"paths":request.scopes,"bounds":request.bounds(),"filesScanned":files_scanned,"bytesRead":bytes_read,"matches":matches,"skipped":scan.skipped,"truncated":!scan.truncation.is_empty(),"truncation":scan.truncation,"limitations":LIMITATIONS}),
    )
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    #[test]
    fn opened_symlinks_and_special_files_are_refused_without_blocking() {
        let root = std::env::temp_dir().join(format!("craft-search-open-{}", std::process::id()));
        fs::create_dir_all(&root).unwrap();
        let target = root.join("target");
        fs::write(&target, "outside").unwrap();
        let link = root.join("link");
        std::os::unix::fs::symlink(&target, &link).unwrap();
        assert!(open_source(&link).is_err());
        let fifo = root.join("fifo");
        assert!(
            std::process::Command::new("mkfifo")
                .arg(&fifo)
                .status()
                .unwrap()
                .success()
        );
        assert!(open_source(&fifo).is_err());
        fs::remove_dir_all(root).unwrap();
    }
}
