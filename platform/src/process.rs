//! Shell-free Git and process-table observations with a wall-clock and captured-output
//! bound, and whether recorded owner processes still run.
use craft_engine::scratch::Liveness;
use std::{
    io::Read,
    path::Path,
    process::{Command, Stdio},
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};

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
    let mut command = Command::new("git");
    command
        .args(["-c", "core.fsmonitor=false", "-C"])
        .arg(root)
        .args(args)
        .env("GIT_OPTIONAL_LOCKS", "0");
    capture(what, command, max_bytes, timeout)
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

fn capture(
    what: &str,
    mut command: Command,
    max_bytes: usize,
    timeout: Duration,
) -> Result<Option<Vec<u8>>, String> {
    let mut child = match command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
    {
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
            match rx.try_recv() {
                Ok(Ok(bytes)) if bytes.len() <= max_bytes => output = Some(bytes),
                Ok(Ok(_)) => break Err(format!("{what} failed: output exceeds bound")),
                Ok(Err(error)) => break Err(format!("{what} failed: {error}")),
                Err(mpsc::TryRecvError::Disconnected) => {
                    break Err(format!("{what} reader failed"));
                }
                Err(mpsc::TryRecvError::Empty) => {}
            }
        }
        match child.try_wait() {
            Ok(Some(status)) if output.is_some() => {
                break Ok(if status.success() {
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

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(unix)]
    fn shell(script: &str) -> Command {
        let mut command = Command::new("/bin/sh");
        command.args(["-c", script]);
        command
    }
    #[cfg(unix)]
    #[test]
    fn captured_output_limits_and_failure_are_distinct() {
        assert_eq!(
            capture("Test", shell("printf abc"), 3, Duration::from_secs(2)).unwrap(),
            Some(b"abc".to_vec())
        );
        assert!(
            capture("Test", shell("printf abcd"), 3, Duration::from_secs(2))
                .unwrap_err()
                .contains("output exceeds bound")
        );
        assert_eq!(
            capture("Test", shell("exit 1"), 3, Duration::from_secs(2)).unwrap(),
            None
        );
    }
    #[cfg(unix)]
    #[test]
    fn hanging_inventory_is_killed_and_reaped() {
        let start = Instant::now();
        assert!(
            capture(
                "Test",
                shell("exec sleep 30"),
                10,
                Duration::from_millis(20)
            )
            .unwrap_err()
            .contains("did not complete")
        );
        assert!(start.elapsed() < Duration::from_secs(2));
    }
    #[test]
    fn liveness_knows_this_process_and_observes_others_on_every_platform() {
        let observed = |pid: i64| Processes::default().liveness(pid);
        assert_eq!(observed(i64::from(std::process::id())), Liveness::Live);
        assert_eq!(observed(2_147_483_647), Liveness::Dead);
        #[cfg(unix)]
        assert_eq!(
            observed(1),
            Liveness::Live,
            "a process of another user is live"
        );
        assert_eq!(
            observed(i64::from(u32::MAX) + 1),
            Liveness::Live,
            "an unaddressable pid is never reclaimed"
        );
        // A recorded owner that exits is reclaimable once observed again.
        #[cfg(unix)]
        let mut command = std::process::Command::new("sleep");
        #[cfg(unix)]
        command.arg("30");
        #[cfg(windows)]
        let mut command = std::process::Command::new("ping");
        #[cfg(windows)]
        command.args(["-n", "30", "127.0.0.1"]);
        let mut child = command.stdout(std::process::Stdio::null()).spawn().unwrap();
        let owner = i64::from(child.id());
        assert_eq!(observed(owner), Liveness::Live);
        child.kill().unwrap();
        child.wait().unwrap();
        assert_eq!(observed(owner), Liveness::Dead);
    }

    #[test]
    fn process_listing_accepts_only_well_formed_tasklist_rows() {
        let pids = listed(
            b"\"System Idle Process\",\"0\",\"Services\",\"0\",\"8 K\"\r\n\"a,b.exe\",\"4120\",\"Console\",\"1\",\"12,345 K\"\r\n",
        )
        .unwrap();
        assert_eq!(pids.len(), 2);
        assert!(
            pids.contains(&0) && pids.contains(&4120),
            "commas inside quoted names"
        );
        assert_eq!(listed(b""), None, "no rows is no snapshot");
        assert_eq!(listed(b"INFO: No tasks are running.\r\n"), None);
        assert_eq!(
            listed(b"\"a.exe\",\"12\",\"Console\",\"1\",\"8 K\"\r\n\"b.exe\",\"-4\",\"x\"\r\n"),
            None,
            "one malformed row rejects the snapshot"
        );
        assert_eq!(
            listed(b"\"a.exe\",\"99999999999\",\"x\",\"1\",\"8 K\""),
            None
        );
        assert_eq!(
            listed(b"\"a\"x\"12\",\"x\""),
            None,
            "fields are comma-separated"
        );
        assert_eq!(listed(b"x\"a\",\"12\",\"x\""), None, "rows start quoted");
    }
}
