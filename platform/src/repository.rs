use crate::{Error, syntax::MAX_SOURCE_BYTES};

pub mod comparison;
pub mod snapshot;
pub mod worktree;
use craft_engine::valid_path;
use std::{
    fs::{self, File, OpenOptions},
    io::{BufRead, BufReader, Read},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    time::{SystemTime, UNIX_EPOCH},
};

pub fn now() -> String {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .to_string()
}

pub fn read_source(root: &Path, relative: &str) -> Result<Vec<u8>, Error> {
    if !valid_path(relative) {
        return Err("invalid source path".into());
    }
    let mut full = root.to_path_buf();
    for component in relative.split('/') {
        full.push(component);
        if fs::symlink_metadata(&full)?.file_type().is_symlink() {
            return Err("source symlinks are unsupported".into());
        }
    }
    crate::files::read_bounded(&full, MAX_SOURCE_BYTES).map_err(|error| match error {
        crate::files::ReadError::Bounds => "source exceeds file/byte bounds".into(),
        crate::files::ReadError::Grew => "source grew beyond byte bound".into(),
        crate::files::ReadError::Io(error) => error.into(),
    })
}

pub fn database_guard(path: &Path) -> Result<File, Error> {
    for ancestor in path.ancestors() {
        if let Ok(meta) = fs::symlink_metadata(ancestor)
            && meta.file_type().is_symlink()
        {
            return Err("database path contains symlink".into());
        }
    }
    for suffix in ["", "-journal", "-wal", "-shm", ".lock"] {
        let file = format!("{}{suffix}", path.display());
        if let Ok(meta) = fs::symlink_metadata(&file) {
            if !meta.is_file() || meta.file_type().is_symlink() {
                return Err("unsafe database companion".into());
            }
            #[cfg(unix)]
            {
                use std::os::unix::fs::MetadataExt;
                if meta.nlink() != 1 {
                    return Err("database companion is hard linked".into());
                }
            }
        }
    }
    let parent = path.parent().ok_or("database parent required")?;
    let mut missing = Vec::new();
    let mut current = parent;
    while !current.exists() {
        missing.push(current.to_path_buf());
        current = current.parent().ok_or("invalid state path")?;
    }
    for directory in missing.iter().rev() {
        crate::files::create_private_directory(directory)?;
        let metadata = fs::symlink_metadata(directory)?;
        if !metadata.is_dir() || metadata.file_type().is_symlink() {
            return Err("unsafe state directory".into());
        }
    }
    let mut options = OpenOptions::new();
    options.create(true).truncate(false).read(true).write(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
    let file = options.open(format!("{}.lock", path.display()))?;
    file.try_lock()
        .map_err(|_| Error::busy("project evidence is busy"))?;
    Ok(file)
}

/// Directories never inventoried: VCS metadata, Craft state, plans, dependencies and build output.
const GENERATED: &[&str] = &[".git", ".craft", ".plan", "node_modules", "dist", "target"];

/// Directory entries one filesystem inventory may visit before it stops as incomplete.
const MAX_INVENTORY_ENTRIES: usize = 1_000_000;

/// Untracked-file pathspecs one Git listing may pass; more falls back to a full walk.
const MAX_PATHSPECS: usize = 256;
const MAX_PATHSPEC_BYTES: usize = 16_384;

/// Paths under `root`, relative and `/`-separated. A Git worktree (or a directory inside one)
/// is listed by Git, tracked and unignored. Any other directory is walked without following
/// symlinks; each nested Git worktree found there is listed by its own Git, so its ignore
/// rules apply, and its paths keep the directory as a prefix. A walk reads no ignore files.
/// Git lists every tracked path but does not walk excluded directories for untracked files,
/// so untracked files there are not listed.
pub struct Inventory {
    root: PathBuf,
    exclusions: Vec<String>,
    listing: Option<Listing>,
    directories: Vec<String>,
    files: Vec<String>,
    entries: usize,
    repositories: Vec<String>,
    walked: bool,
}

/// Tracked paths, then untracked unignored ones within `untracked` pathspecs (`None`: all).
struct Listing {
    root: PathBuf,
    child: Child,
    reader: BufReader<std::process::ChildStdout>,
    finished: bool,
    prefix: String,
    untracked: Option<Option<Vec<String>>>,
}

impl Inventory {
    /// `exclusions` are root-relative path prefixes, as `contracts::excluded` matches them.
    pub fn start(root: &Path, exclusions: &[String]) -> Result<Self, Error> {
        let mut inventory = Self {
            root: root.to_path_buf(),
            exclusions: exclusions.to_vec(),
            listing: None,
            directories: Vec::new(),
            files: Vec::new(),
            entries: 0,
            repositories: Vec::new(),
            walked: false,
        };
        if crate::process::git(root, &["rev-parse", "--show-toplevel"], 16384)?.is_some() {
            inventory.listing = Some(Listing::start(root, String::new(), exclusions)?);
        } else if root
            .ancestors()
            .any(|p| fs::symlink_metadata(p.join(".git")).is_ok())
        {
            return Err("Git is unavailable for a Git worktree; unseen sources retained".into());
        } else {
            inventory.walked = true;
            inventory.directories.push(String::new());
        }
        Ok(inventory)
    }

    /// How sources were found: `git`, or `filesystem` with the nested worktrees listed by Git.
    pub fn describe(&self) -> serde_json::Value {
        if self.walked {
            serde_json::json!({"kind": "filesystem", "repositories": self.repositories})
        } else {
            serde_json::json!({"kind": "git"})
        }
    }

    pub fn next_path(&mut self) -> Result<Option<String>, Error> {
        loop {
            if let Some(listing) = &mut self.listing {
                match listing.next_path()? {
                    Some(path) => return Ok(Some(path)),
                    None => self.listing = None,
                }
            }
            if let Some(file) = self.files.pop() {
                return Ok(Some(file));
            }
            let Some(directory) = self.directories.pop() else {
                return Ok(None);
            };
            self.visit(directory)?;
        }
    }

    /// Queue a directory's files and unexcluded subdirectories in name order, or hand a nested
    /// worktree to Git. Symlinks and special files are not sources.
    fn visit(&mut self, directory: String) -> Result<(), Error> {
        let absolute = if directory.is_empty() {
            self.root.clone()
        } else {
            self.root.join(&directory)
        };
        if !directory.is_empty() && fs::symlink_metadata(absolute.join(".git")).is_ok() {
            let prefix = format!("{directory}/");
            let exclusions = self
                .exclusions
                .iter()
                .filter_map(|path| {
                    if prefix.starts_with(&format!("{path}/")) {
                        Some(String::new())
                    } else {
                        path.strip_prefix(&prefix).map(str::to_string)
                    }
                })
                .collect::<Vec<_>>();
            self.listing = Some(Listing::start(&absolute, prefix, &exclusions)?);
            self.repositories.push(directory);
            return Ok(());
        }
        let mut children = Vec::new();
        for entry in fs::read_dir(&absolute)? {
            let entry = entry?;
            self.entries += 1;
            if self.entries > MAX_INVENTORY_ENTRIES {
                return Err(format!(
                    "filesystem inventory exceeds {MAX_INVENTORY_ENTRIES} entries; unseen sources retained"
                )
                .into());
            }
            let name = entry
                .file_name()
                .into_string()
                .map_err(|_| "non-UTF-8 inventory path unsupported")?;
            let kind = entry.file_type()?;
            if kind.is_dir() && GENERATED.contains(&name.as_str()) {
                continue;
            }
            if kind.is_dir() || kind.is_file() {
                children.push((name, kind.is_dir()));
            }
        }
        children.sort();
        for (name, is_directory) in children.into_iter().rev() {
            let path = if directory.is_empty() {
                name
            } else {
                format!("{directory}/{name}")
            };
            if is_directory {
                // An excluded directory is not walked, nor a worktree inside it listed.
                if !craft_engine::contracts::excluded(&path, &self.exclusions) {
                    self.directories.push(path);
                }
            } else {
                self.files.push(path);
            }
        }
        Ok(())
    }
}

impl Listing {
    /// `exclusions` are relative to `root`; an empty one excludes the whole worktree.
    fn start(root: &Path, prefix: String, exclusions: &[String]) -> Result<Self, Error> {
        let untracked = if exclusions.iter().any(String::is_empty) {
            None
        } else {
            Some(untracked_scope(root, exclusions)?)
        };
        let (child, reader) = Self::spawn(root, &["--cached"], &[])?;
        Ok(Self {
            root: root.to_path_buf(),
            child,
            reader,
            finished: false,
            prefix,
            untracked,
        })
    }

    fn spawn(
        root: &Path,
        mode: &[&str],
        pathspecs: &[String],
    ) -> Result<(Child, BufReader<std::process::ChildStdout>), Error> {
        let mut child = Command::new("git")
            .args(["-C"])
            .arg(root)
            .args(["ls-files", "-z"])
            .args(mode)
            .arg("--")
            .args(pathspecs)
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .stdin(Stdio::null())
            .spawn()?;
        let reader = BufReader::new(child.stdout.take().ok_or("missing Git output")?);
        Ok((child, reader))
    }

    fn next_path(&mut self) -> Result<Option<String>, Error> {
        let mut bytes = Vec::new();
        let count = self.reader.by_ref().take(4098).read_until(0, &mut bytes)?;
        if count == 0 {
            let status = self.child.wait()?;
            self.finished = true;
            if !status.success() {
                return Err("Git inventory failed; unseen sources retained".into());
            }
            let pathspecs = match self.untracked.take() {
                None => return Ok(None),
                Some(None) => Vec::new(),
                // Every entry is excluded: an empty pathspec list would mean everything.
                Some(Some(pathspecs)) if pathspecs.is_empty() => return Ok(None),
                Some(Some(pathspecs)) => pathspecs,
            };
            (self.child, self.reader) =
                Self::spawn(&self.root, &["--others", "--exclude-standard"], &pathspecs)?;
            self.finished = false;
            return self.next_path();
        }
        if bytes.pop() != Some(0) {
            return Err("inventory path exceeds bound or is incomplete".into());
        }
        String::from_utf8(bytes)
            .map(|path| Some(format!("{}{path}", self.prefix)))
            .map_err(|_| "non-UTF-8 inventory path unsupported".into())
    }
}

/// Literal pathspecs covering `root` except the excluded prefixes, so Git's untracked walk
/// skips excluded directories: the entries of each directory that holds an exclusion, minus
/// the excluded ones. `None` lists everything: no exclusion applies, or the expansion would
/// exceed its bounds. An empty list means every entry is excluded.
fn untracked_scope(root: &Path, exclusions: &[String]) -> Result<Option<Vec<String>>, Error> {
    if exclusions.is_empty() {
        return Ok(None);
    }
    let (mut pathspecs, mut bytes, mut directories) = (Vec::new(), 0, vec![String::new()]);
    while let Some(directory) = directories.pop() {
        let mut names = fs::read_dir(root.join(&directory))?
            .map(|entry| {
                let entry = entry?;
                let name = entry.file_name().into_string().ok();
                Ok((name, entry.file_type()?.is_dir()))
            })
            .collect::<Result<Vec<_>, std::io::Error>>()?;
        names.sort();
        for (name, is_directory) in names {
            // Git cannot list a non-UTF-8 name as a source either; leave it to the full walk.
            let Some(name) = name else {
                return Ok(None);
            };
            if directory.is_empty() && name == ".git" {
                continue;
            }
            let path = if directory.is_empty() {
                name
            } else {
                format!("{directory}/{name}")
            };
            if exclusions.contains(&path) {
                continue;
            }
            if is_directory
                && exclusions
                    .iter()
                    .any(|e| e.starts_with(&format!("{path}/")))
            {
                directories.push(path);
                continue;
            }
            bytes += path.len() + ":(literal)".len();
            pathspecs.push(format!(":(literal){path}"));
            if pathspecs.len() > MAX_PATHSPECS || bytes > MAX_PATHSPEC_BYTES {
                return Ok(None);
            }
        }
    }
    Ok(Some(pathspecs))
}

impl Drop for Listing {
    fn drop(&mut self) {
        if !self.finished {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }
}

pub fn eligible(path: &str) -> bool {
    valid_path(path)
        && !path.split('/').any(|p| GENERATED.contains(&p))
        && crate::providers::for_path(path).is_some()
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Fixture(std::path::PathBuf);
    impl Fixture {
        fn new(name: &str) -> Self {
            let path = std::env::temp_dir()
                .join(format!("craft-repository-{}-{name}", std::process::id()));
            fs::create_dir(&path).unwrap();
            Self(path)
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn source_paths_cannot_escape_the_selected_root() {
        let fixture = Fixture::new("escape");
        fs::write(fixture.0.join("source.mjs"), b"export const x = 1;").unwrap();
        for path in [
            "../source.mjs",
            "/source.mjs",
            "a/../source.mjs",
            "a\\source.mjs",
        ] {
            assert!(read_source(&fixture.0, path).is_err(), "{path}");
        }
        assert!(read_source(&fixture.0, "source.mjs").is_ok());
    }

    fn inventory(root: &Path) -> (Vec<String>, serde_json::Value) {
        let mut inventory = Inventory::start(root, &[]).unwrap();
        let mut paths = Vec::new();
        while let Some(path) = inventory.next_path().unwrap() {
            paths.push(path);
        }
        (paths, inventory.describe())
    }

    #[test]
    fn plain_directories_are_walked_and_nested_worktrees_listed_by_git() {
        let fixture = Fixture::new("walk");
        let root = fixture.0.canonicalize().unwrap();
        for file in [
            "b.js",
            "a/z.mjs",
            "a/node_modules/dep.js",
            "a/.craft/state.js",
            "repo/kept.js",
            "repo/ignored.js",
            "repo/.gitignore",
        ] {
            let path = root.join(file);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(
                &path,
                if file.ends_with(".gitignore") {
                    "ignored.js\n"
                } else {
                    ""
                },
            )
            .unwrap();
        }
        let status = Command::new("git")
            .args(["init", "--quiet"])
            .arg(root.join("repo"))
            .status()
            .unwrap();
        assert!(status.success());
        let (paths, described) = inventory(&root);
        assert_eq!(
            paths,
            ["b.js", "a/z.mjs", "repo/.gitignore", "repo/kept.js"]
        );
        assert_eq!(
            described,
            serde_json::json!({"kind": "filesystem", "repositories": ["repo"]})
        );
        // Inside a worktree, Git lists it: tracked and unignored only, nested in no prefix.
        let (paths, described) = inventory(&root.join("repo"));
        assert_eq!(paths, [".gitignore", "kept.js"]);
        assert_eq!(described, serde_json::json!({"kind": "git"}));
    }

    #[test]
    fn excluded_directories_list_tracked_but_not_untracked_paths() {
        let fixture = Fixture::new("excluded");
        let root = fixture.0.canonicalize().unwrap();
        let repo = root.join("repo");
        for file in [
            "kept.js",
            "new.js",
            "samples/tracked.js",
            "samples/new.js",
            "a/b/gen/new.js",
            "a/b/new.js",
            "a/new.js",
        ] {
            let path = repo.join(file);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(&path, "").unwrap();
        }
        let git = |args: &[&str]| {
            let status = Command::new("git").arg("-C").arg(&repo).args(args).status();
            assert!(status.unwrap().success());
        };
        git(&["init", "--quiet"]);
        git(&["add", "kept.js", "samples/tracked.js"]);
        let list = |root: &Path, exclusions: &[&str]| {
            let exclusions: Vec<_> = exclusions.iter().map(|e| e.to_string()).collect();
            let mut inventory = Inventory::start(root, &exclusions).unwrap();
            let mut paths = Vec::new();
            while let Some(path) = inventory.next_path().unwrap() {
                paths.push(path);
            }
            paths
        };
        assert_eq!(
            list(&repo, &["samples", "a/b/gen"]),
            [
                "kept.js",
                "samples/tracked.js",
                "a/b/new.js",
                "a/new.js",
                "new.js"
            ]
        );
        assert_eq!(list(&repo, &[]).len(), 7);
        // A plain root maps its exclusions into each nested worktree, and does not walk an
        // excluded directory or list a worktree inside it.
        assert_eq!(
            list(&root, &["repo/samples", "repo/a"]),
            ["repo/kept.js", "repo/samples/tracked.js", "repo/new.js"]
        );
        fs::create_dir_all(root.join("plain/gen")).unwrap();
        fs::write(root.join("plain/gen/a.js"), "").unwrap();
        fs::write(root.join("plain/b.js"), "").unwrap();
        assert_eq!(list(&root, &["repo", "plain/gen"]), ["plain/b.js"]);
        let mut inventory = Inventory::start(&root, &["repo".into()]).unwrap();
        while inventory.next_path().unwrap().is_some() {}
        assert_eq!(
            inventory.describe(),
            serde_json::json!({"kind": "filesystem", "repositories": []})
        );
    }

    #[test]
    fn untracked_scope_falls_back_to_a_full_walk_beyond_its_bounds() {
        let fixture = Fixture::new("pathspecs");
        for index in 0..=MAX_PATHSPECS {
            fs::write(fixture.0.join(format!("f{index}.js")), "").unwrap();
        }
        fs::create_dir(fixture.0.join("gen")).unwrap();
        assert_eq!(untracked_scope(&fixture.0, &[]).unwrap(), None);
        assert_eq!(untracked_scope(&fixture.0, &["gen".into()]).unwrap(), None);
        fs::remove_file(fixture.0.join("f0.js")).unwrap();
        let scope = untracked_scope(&fixture.0, &["gen".into()])
            .unwrap()
            .unwrap();
        assert_eq!(scope.len(), MAX_PATHSPECS);
        assert!(!scope.iter().any(|spec| spec.ends_with("gen")));
    }

    #[cfg(unix)]
    #[test]
    fn filesystem_inventory_skips_symlinks_and_special_files() {
        use std::os::unix::fs::symlink;
        let fixture = Fixture::new("walk-links");
        let root = fixture.0.canonicalize().unwrap();
        fs::create_dir(root.join("real")).unwrap();
        fs::write(root.join("real/source.js"), "").unwrap();
        symlink(root.join("real"), root.join("alias")).unwrap();
        symlink("real/source.js", root.join("link.js")).unwrap();
        assert!(
            Command::new("mkfifo")
                .arg(root.join("pipe.js"))
                .status()
                .unwrap()
                .success()
        );
        assert_eq!(inventory(&root).0, ["real/source.js"]);
    }

    #[cfg(unix)]
    #[test]
    fn evidence_database_lock_and_backup_are_private() {
        use std::os::unix::fs::PermissionsExt;
        let fixture = Fixture::new("modes");
        let directory = fixture.0.canonicalize().unwrap().join("evidence");
        let database = directory.join("project.sqlite");
        let backup = database.with_extension("sqlite.backup");
        let _guard = database_guard(&database).unwrap();
        let store = crate::store::Store::open(&database).unwrap();
        let _backup_guard = database_guard(&backup).unwrap();
        store.backup(&backup).unwrap();
        assert!(store.backup(&backup).is_err());
        for file in [
            database.clone(),
            backup.clone(),
            directory.join("project.sqlite.lock"),
            directory.join("project.sqlite.backup.lock"),
        ] {
            let mode = fs::metadata(&file).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode, 0o600, "{}", file.display());
        }
        assert!(crate::store::Store::open(&backup).unwrap().status().is_ok());
    }

    #[cfg(unix)]
    #[test]
    fn source_symlinks_and_special_files_are_rejected() {
        use std::os::unix::fs::symlink;
        let fixture = Fixture::new("links");
        fs::write(fixture.0.join("source.mjs"), b"source").unwrap();
        symlink("source.mjs", fixture.0.join("alias.mjs")).unwrap();
        symlink("missing", fixture.0.join("dangling.mjs")).unwrap();
        symlink(&fixture.0, fixture.0.join("directory")).unwrap();
        for path in ["alias.mjs", "dangling.mjs", "directory/source.mjs"] {
            assert!(read_source(&fixture.0, path).is_err());
        }
        let fifo = fixture.0.join("pipe.mjs");
        assert!(
            Command::new("mkfifo")
                .arg(&fifo)
                .status()
                .unwrap()
                .success()
        );
        assert_eq!(
            read_source(&fixture.0, "pipe.mjs").unwrap_err().to_string(),
            "source exceeds file/byte bounds"
        );
    }
}
