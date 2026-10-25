//! Shell-free Git and process-table observations with a wall-clock and captured-output
//! bound.
use std::{
    io::Read,
    path::Path,
    process::{Command, Stdio},
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};

pub fn git(root: &Path, args: &[&str], max_bytes: usize) -> Result<Option<Vec<u8>>, String> {
    let mut command = Command::new("git");
    command
        .args(["-c", "core.fsmonitor=false", "-C"])
        .arg(root)
        .args(args)
        .env("GIT_OPTIONAL_LOCKS", "0");
    capture("Git inventory", command, max_bytes, Duration::from_secs(5))
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

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    fn shell(script: &str) -> Command {
        let mut command = Command::new("/bin/sh");
        command.args(["-c", script]);
        command
    }
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
}
