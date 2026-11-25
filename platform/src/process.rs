//! Shell-free Git and process-table observations with a wall-clock and captured-output
//! bound, and whether recorded owner processes still run.
use craft_engine::scratch::Liveness;
#[cfg(any(target_os = "macos", all(test, unix)))]
use std::{ffi::OsStr, path::PathBuf};
use std::{
    ffi::OsString,
    io::Read,
    path::Path,
    process::{Child, Command, Stdio},
    sync::{Mutex, OnceLock, mpsc},
    thread,
    time::{Duration, Instant},
};

/// Start `command`; every Craft child process starts here. On macOS the standard library
/// creates a child's pipes and only then marks them close-on-exec, so a child another
/// thread starts in between inherits them; the pipe then reports end-of-file only when that
/// unrelated child exits, and a bounded read of a fast Git query times out. Starting one
/// child at a time closes that window for every child Craft starts.
pub(crate) fn spawn(command: &mut Command) -> std::io::Result<Child> {
    static SPAWNING: Mutex<()> = Mutex::new(());
    let _one = SPAWNING
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    command.spawn()
}

/// Start `program` in `dir` and leave it running after this process exits, with no standard
/// streams and its own process group, so a host waiting for hook output never waits for it.
/// Windows starts children with every inheritable handle, including the host's output pipe,
/// and marking those handles needs `unsafe`; there it is unsupported.
pub fn spawn_detached(program: &Path, args: &[&str], dir: &Path) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        let mut command = Command::new(program);
        command
            .args(args)
            .current_dir(dir)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        std::os::unix::process::CommandExt::process_group(&mut command, 0);
        // Not waited for: the child is reparented when this short-lived process exits.
        spawn(&mut command).map(drop)
    }
    #[cfg(not(unix))]
    {
        let _ = (program, args, dir);
        Err(std::io::ErrorKind::Unsupported.into())
    }
}

/// Git run in `dir`, the one way every Craft Git command starts: no file-system monitor, no
/// optional index lock (an inventory never rewrites the index), no credential prompt and no
/// standard input. Callers add the subcommand and decide how to bound its output.
///
/// On macOS, `/usr/bin/git` is an `xcrun` forwarder that looks up the developer directory on
/// every call (~10 ms each, several per hook); when it is the Git on `PATH` and the active
/// developer directory is unambiguous, the Git inside it starts directly. Any other first Git
/// on `PATH`, or any doubt, keeps the plain `git` lookup.
pub(crate) fn git_command(dir: &Path) -> Command {
    static PROGRAM: OnceLock<OsString> = OnceLock::new();
    let mut command = Command::new(PROGRAM.get_or_init(|| {
        #[cfg(target_os = "macos")]
        if let Some(git) = developer_git(
            std::env::var_os("PATH").as_deref(),
            std::env::var_os("DEVELOPER_DIR").as_deref(),
            std::fs::read_link("/var/db/xcode_select_link").ok(),
            executable,
        ) {
            return git.into_os_string();
        }
        OsString::from("git")
    }));
    command
        .args(["-c", "core.fsmonitor=false", "-C"])
        .arg(dir)
        .env("GIT_OPTIONAL_LOCKS", "0")
        .env("GIT_TERMINAL_PROMPT", "0")
        .stdin(Stdio::null());
    command
}

#[cfg(target_os = "macos")]
fn executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(path)
        .is_ok_and(|meta| meta.is_file() && meta.permissions().mode() & 0o111 != 0)
}

#[cfg(any(target_os = "macos", all(test, unix)))]
const FORWARDER: &str = "/usr/bin/git";
#[cfg(any(target_os = "macos", all(test, unix)))]
const XCODE: &str = "/Applications/Xcode.app/Contents/Developer";
#[cfg(any(target_os = "macos", all(test, unix)))]
const COMMAND_LINE_TOOLS: &str = "/Library/Developer/CommandLineTools";

/// The developer-directory Git that the `/usr/bin/git` forwarder would start, when that
/// forwarder is the first Git on `path`. The directory is `DEVELOPER_DIR` (an Xcode bundle or
/// its `Contents/Developer`), else the `xcode-select` choice, else the Command Line Tools
/// when no default Xcode is installed; with a default Xcode and no choice the answer is left
/// to `xcrun`.
#[cfg(any(target_os = "macos", all(test, unix)))]
fn developer_git(
    path: Option<&OsStr>,
    developer_dir: Option<&OsStr>,
    selected: Option<PathBuf>,
    executable: impl Fn(&Path) -> bool,
) -> Option<PathBuf> {
    let first = std::env::split_paths(path?)
        .map(|dir| dir.join("git"))
        .find(|git| executable(git))?;
    if first != Path::new(FORWARDER) {
        return None;
    }
    let candidates = match developer_dir.filter(|dir| !dir.is_empty()) {
        Some(dir) => vec![
            Path::new(dir).join("usr/bin/git"),
            Path::new(dir).join("Contents/Developer/usr/bin/git"),
        ],
        None => match selected {
            Some(dir) => vec![dir.join("usr/bin/git")],
            None if executable(&Path::new(XCODE).join("usr/bin/git")) => return None,
            None => vec![Path::new(COMMAND_LINE_TOOLS).join("usr/bin/git")],
        },
    };
    candidates.into_iter().find(|git| executable(git))
}

/// Whether a program named `name` is a file in one of the `path` directories, the way a shell
/// would find it, without starting it. Windows also finds the `.exe`, `.cmd` and `.bat` forms.
pub fn on_path(name: &str, path: Option<&OsStr>) -> bool {
    let Some(path) = path else {
        return false;
    };
    let names: Vec<String> = if cfg!(windows) {
        ["", ".exe", ".cmd", ".bat"]
            .iter()
            .map(|extension| format!("{name}{extension}"))
            .collect()
    } else {
        vec![name.to_string()]
    };
    std::env::split_paths(path)
        .filter(|dir| dir.is_absolute())
        .any(|dir| names.iter().any(|name| runnable(&dir.join(name))))
}

#[cfg(unix)]
fn runnable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(path)
        .is_ok_and(|meta| meta.is_file() && meta.permissions().mode() & 0o111 != 0)
}

#[cfg(not(unix))]
fn runnable(path: &Path) -> bool {
    std::fs::metadata(path).is_ok_and(|meta| meta.is_file())
}

pub fn git(root: &Path, args: &[&str], max_bytes: usize) -> Result<Option<Vec<u8>>, String> {
    git_within(
        "Git inventory",
        root,
        args,
        max_bytes,
        Duration::from_secs(5),
    )
}

/// Git output when it exits successfully, `None` when it fails or is not installed, and an
/// error naming `what` when it overflows `max_bytes` or outlives `timeout`.
pub fn git_within(
    what: &str,
    root: &Path,
    args: &[&str],
    max_bytes: usize,
    timeout: Duration,
) -> Result<Option<Vec<u8>>, String> {
    let mut command = git_command(root);
    command.args(args);
    capture(what, command, max_bytes, timeout)
}

/// Paths of files under `root` that contain any of `terms` as a case-sensitive literal, as
/// Git's own text search finds them: tracked and untracked files outside ignores, binary
/// files excluded, NUL-separated and relative to `root` (also below the top level). `pathspecs` narrow the search;
/// none means the whole worktree. Empty output when nothing matches, `None` when Git cannot
/// search here.
pub fn git_grep_files(
    root: &Path,
    terms: &[String],
    pathspecs: &[String],
    max_bytes: usize,
) -> Result<Option<Vec<u8>>, String> {
    let mut command = git_command(root);
    // Literal, case-sensitive, text files only, names only; never a pager, an external
    // grep or a configured pattern type.
    command.args([
        "-c",
        "grep.patternType=fixed",
        "-c",
        "grep.fallbackToNoIndex=false",
        "grep",
        "--untracked",
        "--no-recurse-submodules",
        "-F",
        "-I",
        "-l",
        "-z",
    ]);
    for term in terms {
        command.arg("-e").arg(term);
    }
    command.arg("--");
    command.args(pathspecs.iter().map(|spec| format!(":(literal){spec}")));
    // Exit status 1 means no file matched: an answer, not a failure.
    capture_status(
        "Git text search",
        command,
        max_bytes,
        Duration::from_secs(5),
        |code| code == Some(0) || code == Some(1),
    )
}

/// The Windows process table as `tasklist` CSV, or `None` when it cannot be listed.
#[cfg(windows)]
pub fn tasks(max_bytes: usize) -> Option<Vec<u8>> {
    let root = std::env::var_os("SystemRoot")?;
    let mut command = Command::new(Path::new(&root).join("System32").join("tasklist.exe"));
    command.args(["/NH", "/FO", "CSV"]);
    capture(
        "Process listing",
        command,
        max_bytes,
        Duration::from_secs(5),
    )
    .ok()
    .flatten()
}

/// Observes whether recorded owner processes run. Unix asks the kernel per pid; a process
/// that exists but belongs to another user counts as live. Stable Rust cannot ask Windows
/// without `unsafe`, so the first question takes one bounded `tasklist` snapshot that later
/// questions reuse; create the observer only once no new owner can be recorded (under the
/// workspace lock). A missing or unreadable snapshot reports `Unknown`.
#[derive(Default)]
pub struct Processes {
    #[cfg(windows)]
    listed: std::cell::OnceCell<Option<std::collections::HashSet<u32>>>,
}

impl Processes {
    pub fn liveness(&self, pid: i64) -> Liveness {
        if pid == i64::from(std::process::id()) {
            return Liveness::Live;
        }
        #[cfg(unix)]
        {
            let Some(pid) = i32::try_from(pid)
                .ok()
                .and_then(rustix::process::Pid::from_raw)
            else {
                return Liveness::Live;
            };
            match rustix::process::test_kill_process(pid) {
                Err(rustix::io::Errno::SRCH) => Liveness::Dead,
                _ => Liveness::Live,
            }
        }
        #[cfg(windows)]
        {
            let Ok(pid) = u32::try_from(pid) else {
                return Liveness::Live;
            };
            let listed = self
                .listed
                .get_or_init(|| tasks(TASKS_BYTES).and_then(|csv| listed(&csv)));
            match listed {
                Some(pids) if pids.contains(&pid) => Liveness::Live,
                Some(_) => Liveness::Dead,
                None => Liveness::Unknown,
            }
        }
        #[cfg(not(any(unix, windows)))]
        Liveness::Unknown
    }
}

#[cfg(windows)]
const TASKS_BYTES: usize = 4 << 20;

/// Process ids from `tasklist /NH /FO CSV` rows (`"image","pid",…`). Image names cannot
/// contain quotes. Empty output or any malformed row is not a snapshot.
#[cfg(any(windows, test))]
fn listed(csv: &[u8]) -> Option<std::collections::HashSet<u32>> {
    let mut pids = std::collections::HashSet::new();
    for line in csv.split(|byte| *byte == b'\n') {
        let line = line.strip_suffix(b"\r").unwrap_or(line);
        if line.is_empty() {
            continue;
        }
        let fields: Vec<&[u8]> = line.split(|byte| *byte == b'"').collect();
        // `"a","b",…` splits into "", a, ",", b, …
        let pid = (line.starts_with(b"\"") && fields.len() > 4 && fields[2] == b",")
            .then(|| std::str::from_utf8(fields[3]).ok())
            .flatten()
            .filter(|digits| digits.bytes().all(|b| b.is_ascii_digit()))
            .and_then(|digits| digits.parse().ok())?;
        pids.insert(pid);
    }
    (!pids.is_empty()).then_some(pids)
}

const OVERFLOW: &str = "output exceeds bound";

/// Whether a capture error says the output outgrew its bound: an answer that repeats, unlike
/// a process that could not start or did not complete in time.
pub fn overflowed(error: &str) -> bool {
    error.ends_with(OVERFLOW)
}

fn capture(
    what: &str,
    command: Command,
    max_bytes: usize,
    timeout: Duration,
) -> Result<Option<Vec<u8>>, String> {
    capture_status(what, command, max_bytes, timeout, |code| code == Some(0))
}

/// Captured output when `accepted` holds for the exit code, `None` otherwise.
fn capture_status(
    what: &str,
    mut command: Command,
    max_bytes: usize,
    timeout: Duration,
    accepted: fn(Option<i32>) -> bool,
) -> Result<Option<Vec<u8>>, String> {
    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    let mut child = match spawn(&mut command) {
        Ok(child) => child,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(format!("{what} failed: {error}")),
    };
    let pipe = child
        .stdout
        .take()
        .ok_or(format!("missing {what} output"))?;
    let (tx, rx) = mpsc::sync_channel(1);
    thread::spawn(move || {
        let mut bytes = Vec::new();
        let result = pipe
            .take(max_bytes as u64 + 1)
            .read_to_end(&mut bytes)
            .map(|_| bytes);
        let _ = tx.send(result);
    });
    let start = Instant::now();
    let mut output = None;
    let result = loop {
        if output.is_none() {
            // Output ends when the child closes its pipe, normally as it exits: wait for
            // that instead of polling for it.
            match rx.recv_timeout(timeout.saturating_sub(start.elapsed())) {
                Ok(Ok(bytes)) if bytes.len() <= max_bytes => output = Some(bytes),
                Ok(Ok(_)) => break Err(format!("{what} failed: {OVERFLOW}")),
                Ok(Err(error)) => break Err(format!("{what} failed: {error}")),
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    break Err(format!("{what} reader failed"));
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {
                    break Err(format!("{what} did not complete"));
                }
            }
        }
        match child.try_wait() {
            Ok(Some(status)) if output.is_some() => {
                break Ok(if accepted(status.code()) {
                    output.take()
                } else {
                    None
                });
            }
            Ok(_) => {}
            Err(error) => break Err(format!("{what} failed: {error}")),
        }
        if start.elapsed() >= timeout {
            break Err(format!("{what} did not complete"));
        }
        thread::sleep(Duration::from_millis(2));
    };
    // Never leave a timed-out or overflowing process behind.
    let _ = child.kill();
    let _ = child.wait();
    result
}

/// What happened to one consumer command run by [`run_bounded`].
#[derive(Debug)]
pub struct Ran {
    /// False when the program could not be started; `error` says why.
    pub started: bool,
    /// The exit code; `None` when it did not start, was killed or ended by a signal.
    pub exit: Option<i32>,
    pub timed_out: bool,
    pub elapsed: Duration,
    /// The end of standard output and standard error, in the order their chunks were read.
    pub output: Vec<u8>,
    /// Whether earlier output was dropped to keep the end.
    pub truncated: bool,
    pub error: Option<String>,
}

/// The newest bytes of a stream, at most `keep`.
struct Tail {
    bytes: std::collections::VecDeque<u8>,
    keep: usize,
    dropped: bool,
}

impl Tail {
    fn push(&mut self, chunk: &[u8]) {
        self.bytes.extend(chunk);
        let extra = self.bytes.len().saturating_sub(self.keep);
        if extra > 0 {
            self.bytes.drain(..extra);
            self.dropped = true;
        }
    }
}

/// How long output is still read after the command itself exited: a process it left behind
/// may hold the pipe open, and the result does not wait for that one.
const DRAIN: Duration = Duration::from_millis(500);

/// A program named with a directory is looked up from `dir`, where the command runs; a bare
/// name is left to `PATH`.
fn program_in(program: &str, dir: &Path) -> std::path::PathBuf {
    let path = Path::new(program);
    if path.is_relative() && path.components().count() > 1 {
        dir.join(path)
    } else {
        path.to_path_buf()
    }
}

/// Start `program` with `args` in `dir`. Windows finds only `.exe` for a bare name, while
/// package-manager launchers there are `.cmd` files, so a name that is not found is retried
/// with those script extensions.
fn start(program: &str, args: &[String], dir: &Path) -> std::io::Result<Child> {
    let begin = |program: &Path| {
        let mut command = Command::new(program);
        command
            .args(args)
            .current_dir(dir)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        spawn(&mut command)
    };
    let named = program_in(program, dir);
    let first = begin(&named);
    #[cfg(windows)]
    if matches!(&first, Err(error) if error.kind() == std::io::ErrorKind::NotFound)
        && named.extension().is_none()
    {
        for extension in ["cmd", "bat"] {
            let next = begin(&named.with_extension(extension));
            if !matches!(&next, Err(error) if error.kind() == std::io::ErrorKind::NotFound) {
                return next;
            }
        }
    }
    first
}

/// Process ids started, directly or not, by `parent`, children before their own children,
/// from `pid ppid` rows.
#[cfg(any(unix, test))]
fn descendants(parent: u32, table: &str) -> Vec<u32> {
    let rows: Vec<(u32, u32)> = table
        .lines()
        .filter_map(|line| {
            let mut fields = line.split_whitespace();
            Some((fields.next()?.parse().ok()?, fields.next()?.parse().ok()?))
        })
        .collect();
    let mut found = vec![parent];
    let mut index = 0;
    while index < found.len() {
        let current = found[index];
        for (pid, ppid) in &rows {
            if *ppid == current && !found.contains(pid) {
                found.push(*pid);
            }
        }
        index += 1;
    }
    found.split_off(1)
}

/// End `child` and what it started. The processes are found from the process table rather
/// than a process group of their own, which would outlive an interrupt of the caller.
fn kill_tree(child: &mut Child) {
    #[cfg(unix)]
    {
        let mut listing = Command::new("ps");
        listing.args(["-axo", "pid=,ppid="]);
        let table = capture("Process listing", listing, 4 << 20, Duration::from_secs(5))
            .ok()
            .flatten()
            .unwrap_or_default();
        let started = descendants(child.id(), &String::from_utf8_lossy(&table));
        let _ = child.kill();
        for pid in started {
            if let Some(pid) = i32::try_from(pid)
                .ok()
                .and_then(rustix::process::Pid::from_raw)
            {
                let _ = rustix::process::kill_process(pid, rustix::process::Signal::KILL);
            }
        }
    }
    #[cfg(windows)]
    {
        if let Some(root) = std::env::var_os("SystemRoot") {
            let mut command = Command::new(Path::new(&root).join("System32").join("taskkill.exe"));
            command.args(["/T", "/F", "/PID", &child.id().to_string()]);
            let _ = capture("Process tree end", command, 65_536, Duration::from_secs(5));
        }
        let _ = child.kill();
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = child.kill();
    }
}

/// Run a consumer command without a shell: `program` with `args` in `dir`, no standard
/// input, at most `timeout` long, keeping the last `keep` bytes of its output. A command
/// that outlives the bound is ended together with the processes it started.
pub fn run_bounded(
    program: &str,
    args: &[String],
    dir: &Path,
    timeout: Duration,
    keep: usize,
) -> Ran {
    let begun = Instant::now();
    let mut child = match start(program, args, dir) {
        Ok(child) => child,
        Err(error) => {
            return Ran {
                started: false,
                exit: None,
                timed_out: false,
                elapsed: begun.elapsed(),
                output: Vec::new(),
                truncated: false,
                error: Some(format!("cannot start {program}: {error}")),
            };
        }
    };
    let tail = std::sync::Arc::new(Mutex::new(Tail {
        bytes: std::collections::VecDeque::new(),
        keep,
        dropped: false,
    }));
    let (done, finished) = mpsc::channel();
    let mut readers = 0;
    let mut read = |pipe: Option<Box<dyn Read + Send>>| {
        let Some(mut pipe) = pipe else { return };
        readers += 1;
        let (tail, done) = (tail.clone(), done.clone());
        thread::spawn(move || {
            let mut chunk = [0u8; 8192];
            while let Ok(count @ 1..) = pipe.read(&mut chunk) {
                tail.lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner())
                    .push(&chunk[..count]);
            }
            let _ = done.send(());
        });
    };
    read(child.stdout.take().map(|pipe| Box::new(pipe) as _));
    read(child.stderr.take().map(|pipe| Box::new(pipe) as _));
    let mut timed_out = false;
    let mut error = None;
    let exit = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status.code(),
            Ok(None) => {}
            Err(failure) => {
                error = Some(format!("cannot wait for {program}: {failure}"));
                kill_tree(&mut child);
                let _ = child.wait();
                break None;
            }
        }
        if begun.elapsed() >= timeout {
            timed_out = true;
            kill_tree(&mut child);
            let _ = child.wait();
            break None;
        }
        thread::sleep(Duration::from_millis(10));
    };
    let elapsed = begun.elapsed();
    let drained = Instant::now();
    for _ in 0..readers {
        if finished
            .recv_timeout(DRAIN.saturating_sub(drained.elapsed()))
            .is_err()
        {
            break;
        }
    }
    let tail = tail.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    Ran {
        started: true,
        exit,
        timed_out,
        elapsed,
        output: tail.bytes.iter().copied().collect(),
        truncated: tail.dropped,
        error,
    }
}

#[cfg(test)]
mod tests;
