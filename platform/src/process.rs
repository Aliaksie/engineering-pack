//! Shell-free Git observations with a wall-clock and captured-output bound.
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
    capture(command, max_bytes, Duration::from_secs(5))
}

fn capture(
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
        Err(error) => return Err(format!("Git inventory failed: {error}")),
    };
    let pipe = child.stdout.take().ok_or("missing Git output")?;
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
                Ok(Ok(_)) => break Err("Git inventory failed: output exceeds bound".into()),
                Ok(Err(error)) => break Err(format!("Git inventory failed: {error}")),
                Err(mpsc::TryRecvError::Disconnected) => {
                    break Err("Git inventory reader failed".into());
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
            Err(error) => break Err(format!("Git inventory failed: {error}")),
        }
        if start.elapsed() >= timeout {
            break Err("Git inventory did not complete".into());
        }
        thread::sleep(Duration::from_millis(2));
    };
    // Never leave a timed-out or overflowing inventory process behind.
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
            capture(shell("printf abc"), 3, Duration::from_secs(2)).unwrap(),
            Some(b"abc".to_vec())
        );
        assert!(
            capture(shell("printf abcd"), 3, Duration::from_secs(2))
                .unwrap_err()
                .contains("output exceeds bound")
        );
        assert_eq!(
            capture(shell("exit 1"), 3, Duration::from_secs(2)).unwrap(),
            None
        );
    }
    #[test]
    fn hanging_inventory_is_killed_and_reaped() {
        let start = Instant::now();
        assert!(
            capture(shell("exec sleep 30"), 10, Duration::from_millis(20))
                .unwrap_err()
                .contains("did not complete")
        );
        assert!(start.elapsed() < Duration::from_secs(2));
    }
}
