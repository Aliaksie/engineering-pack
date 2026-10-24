use crate::syntax::MAX_SOURCE_BYTES;
use craft_engine::valid_path;
use std::{
    fs::{self, File, OpenOptions},
    io::{BufRead, BufReader, Read},
    path::Path,
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

pub fn read_source(root: &Path, relative: &str) -> Result<Vec<u8>, String> {
    if !valid_path(relative) {
        return Err("invalid source path".into());
    }
    let mut full = root.to_path_buf();
    for component in relative.split('/') {
        full.push(component);
        if fs::symlink_metadata(&full)
            .map_err(|e| e.to_string())?
            .file_type()
            .is_symlink()
        {
            return Err("source symlinks are unsupported".into());
        }
    }
    let file = File::open(&full).map_err(|e| e.to_string())?;
    let metadata = file.metadata().map_err(|e| e.to_string())?;
    if !metadata.is_file() || metadata.len() > MAX_SOURCE_BYTES as u64 {
        return Err("source exceeds file/byte bounds".into());
    }
    let mut bytes = Vec::new();
    file.take(MAX_SOURCE_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() > MAX_SOURCE_BYTES {
        return Err("source grew beyond byte bound".into());
    }
    Ok(bytes)
}

pub fn database_guard(path: &Path) -> Result<File, String> {
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
        let mut builder = fs::DirBuilder::new();
        #[cfg(unix)]
        {
            use std::os::unix::fs::DirBuilderExt;
            builder.mode(0o700);
        }
        match builder.create(directory) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(error.to_string()),
        }
        let metadata = fs::symlink_metadata(directory).map_err(|e| e.to_string())?;
        if !metadata.is_dir() || metadata.file_type().is_symlink() {
            return Err("unsafe state directory".into());
        }
    }
    let file = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(format!("{}.lock", path.display()))
        .map_err(|e| e.to_string())?;
    file.try_lock().map_err(|_| "project evidence is busy")?;
    Ok(file)
}

pub struct Inventory {
    child: Child,
    reader: BufReader<std::process::ChildStdout>,
    finished: bool,
}
impl Inventory {
    pub fn start(root: &Path) -> Result<Self, String> {
        let mut child = Command::new("git")
            .args(["-C"])
            .arg(root)
            .args([
                "ls-files",
                "-z",
                "--cached",
                "--others",
                "--exclude-standard",
            ])
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .stdin(Stdio::null())
            .spawn()
            .map_err(|e| e.to_string())?;
        let reader = BufReader::new(child.stdout.take().ok_or("missing Git output")?);
        Ok(Self {
            child,
            reader,
            finished: false,
        })
    }
    pub fn next_path(&mut self) -> Result<Option<String>, String> {
        let mut bytes = Vec::new();
        let count = self
            .reader
            .by_ref()
            .take(4098)
            .read_until(0, &mut bytes)
            .map_err(|e| e.to_string())?;
        if count == 0 {
            let status = self.child.wait().map_err(|e| e.to_string())?;
            self.finished = true;
            return if status.success() {
                Ok(None)
            } else {
                Err("Git inventory failed; unseen sources retained".into())
            };
        }
        if bytes.pop() != Some(0) {
            return Err("inventory path exceeds bound or is incomplete".into());
        }
        String::from_utf8(bytes)
            .map(Some)
            .map_err(|_| "non-UTF-8 inventory path unsupported".into())
    }
}
impl Drop for Inventory {
    fn drop(&mut self) {
        if !self.finished {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }
}

pub fn eligible(path: &str) -> bool {
    valid_path(path)
        && !path.split('/').any(|p| {
            matches!(
                p,
                ".git" | ".craft" | ".plan" | "node_modules" | "dist" | "target"
            )
        })
        && crate::providers::for_path(path).is_some()
}
