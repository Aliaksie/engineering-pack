use super::*;

#[cfg(unix)]
fn script(text: &str, timeout: Duration, keep: usize) -> Ran {
    run_bounded(
        "/bin/sh",
        &["-c".to_string(), text.to_string()],
        Path::new("."),
        timeout,
        keep,
    )
}

#[cfg(unix)]
#[test]
fn a_bounded_run_reports_exit_code_and_both_streams() {
    let ran = script(
        "echo out; echo err >&2; exit 3",
        Duration::from_secs(10),
        1024,
    );
    assert!(ran.started && !ran.timed_out);
    assert_eq!(ran.exit, Some(3));
    let output = String::from_utf8(ran.output).unwrap();
    assert!(output.contains("out") && output.contains("err"), "{output}");
}

#[cfg(unix)]
#[test]
fn a_bounded_run_keeps_the_end_of_long_output() {
    let ran = script(
        "i=0; while [ $i -lt 2000 ]; do echo line-$i; i=$((i+1)); done",
        Duration::from_secs(20),
        64,
    );
    assert_eq!(ran.exit, Some(0));
    assert!(ran.truncated && ran.output.len() == 64);
    assert!(String::from_utf8_lossy(&ran.output).ends_with("line-1999\n"));
}

#[cfg(unix)]
#[test]
fn a_run_past_its_bound_is_ended_with_what_it_started() {
    let begun = Instant::now();
    let ran = script("sleep 30 & sleep 30", Duration::from_millis(300), 1024);
    assert!(ran.timed_out && ran.exit.is_none());
    assert!(begun.elapsed() < Duration::from_secs(20));
}

#[test]
fn a_missing_program_is_not_started() {
    let ran = run_bounded(
        "craft-no-such-program",
        &[],
        Path::new("."),
        Duration::from_secs(5),
        64,
    );
    assert!(!ran.started && ran.exit.is_none());
    assert!(ran.error.unwrap().contains("craft-no-such-program"));
}

#[test]
fn descendants_are_found_through_every_generation() {
    let table = "10 1\n11 10\n12 11\n13 1\n14 12\nbad row\n";
    assert_eq!(descendants(10, table), [11, 12, 14]);
    assert!(descendants(13, table).is_empty());
}

#[test]
fn a_program_named_with_a_directory_is_looked_up_where_it_runs() {
    let dir = Path::new("project");
    assert_eq!(program_in("./gradlew", dir), dir.join("./gradlew"));
    assert_eq!(program_in("tools/check", dir), dir.join("tools/check"));
    assert_eq!(program_in("npm", dir), Path::new("npm"));
}

#[cfg(unix)]
fn resolve(
    path: &str,
    developer_dir: Option<&str>,
    selected: Option<&str>,
    present: &[&str],
) -> Option<PathBuf> {
    developer_git(
        Some(OsStr::new(path)),
        developer_dir.map(OsStr::new),
        selected.map(PathBuf::from),
        |git| present.iter().any(|file| git == Path::new(file)),
    )
}

#[test]
#[cfg(unix)]
fn the_forwarder_is_bypassed_only_for_an_unambiguous_developer_directory() {
    const CLT: &str = "/Library/Developer/CommandLineTools/usr/bin/git";
    const XCODE_GIT: &str = "/Applications/Xcode.app/Contents/Developer/usr/bin/git";
    let path = "/opt/none:/usr/bin/:/bin";
    // No choice and no default Xcode: the Command Line Tools.
    assert_eq!(
        resolve(path, None, None, &[FORWARDER, CLT]),
        Some(CLT.into())
    );
    // No choice with a default Xcode installed: left to xcrun.
    assert_eq!(
        resolve(path, None, None, &[FORWARDER, CLT, XCODE_GIT]),
        None
    );
    // The xcode-select choice wins over the default search.
    let beta = "/Applications/Xcode-beta.app/Contents/Developer";
    let beta_git = "/Applications/Xcode-beta.app/Contents/Developer/usr/bin/git";
    assert_eq!(
        resolve(path, None, Some(beta), &[FORWARDER, CLT, beta_git]),
        Some(beta_git.into())
    );
    // DEVELOPER_DIR wins over the choice, as a bundle or its Contents/Developer.
    assert_eq!(
        resolve(
            path,
            Some("/Applications/Xcode-beta.app"),
            Some(COMMAND_LINE_TOOLS),
            &[FORWARDER, CLT, beta_git]
        ),
        Some(beta_git.into())
    );
    assert_eq!(
        resolve(path, Some(beta), None, &[FORWARDER, CLT, beta_git]),
        Some(beta_git.into())
    );
    // A named directory without Git, or a missing Git, falls back to the forwarder.
    assert_eq!(
        resolve(path, Some("/nowhere"), None, &[FORWARDER, CLT]),
        None
    );
    assert_eq!(resolve(path, None, Some(beta), &[FORWARDER, CLT]), None);
    assert_eq!(resolve(path, None, None, &[FORWARDER]), None);
}

#[test]
#[cfg(unix)]
fn another_git_first_on_path_is_kept() {
    let clt = "/Library/Developer/CommandLineTools/usr/bin/git";
    let brew = "/opt/homebrew/bin/git";
    assert_eq!(
        resolve(
            "/opt/homebrew/bin:/usr/bin",
            None,
            None,
            &[brew, FORWARDER, clt]
        ),
        None
    );
    // An earlier directory without Git does not count; no Git at all keeps the lookup.
    assert_eq!(
        resolve("/opt/homebrew/bin:/usr/bin", None, None, &[FORWARDER, clt]),
        Some(clt.into())
    );
    assert_eq!(resolve("/opt/homebrew/bin", None, None, &[clt]), None);
    assert_eq!(developer_git(None, None, None, |_| true), None);
}
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
/// A capture never waits for a long-lived child another thread starts meanwhile: its
/// pipe reaches end-of-file when its own child exits. An inherited pipe would hold it
/// until the bound, since every sleeper outlives the bound; a merely slow start would not.
/// Captures run while a fixed number of sleepers start, so a slow machine runs fewer
/// captures instead of starting more processes than a user may run.
#[cfg(unix)]
#[test]
fn captures_do_not_wait_for_children_other_threads_start() {
    use std::sync::atomic::{AtomicBool, Ordering};
    const SLEEPERS: usize = 400;
    let (started, finished) = (AtomicBool::new(false), AtomicBool::new(false));
    thread::scope(|scope| {
        scope.spawn(|| {
            let mut sleepers = Vec::new();
            for _ in 0..SLEEPERS {
                let mut sleeper = Command::new("sleep");
                sleeper.arg("30").stdin(Stdio::null()).stdout(Stdio::null());
                sleepers.push(spawn(&mut sleeper).unwrap());
                thread::sleep(Duration::from_millis(5));
            }
            started.store(true, Ordering::Relaxed);
            while !finished.load(Ordering::Relaxed) {
                thread::sleep(Duration::from_millis(10));
            }
            for mut sleeper in sleepers {
                let _ = sleeper.kill();
                let _ = sleeper.wait();
            }
        });
        let captures: Vec<_> = (0..6)
            .map(|_| {
                scope.spawn(|| {
                    let mut runs = 0;
                    while !started.load(Ordering::Relaxed) {
                        runs += 1;
                        let ran = capture("Test", shell("printf abc"), 3, Duration::from_secs(10));
                        if ran.is_err() {
                            return (runs, Some(ran));
                        }
                    }
                    (runs, None)
                })
            })
            .collect();
        let results: Vec<_> = captures.into_iter().map(|c| c.join().unwrap()).collect();
        finished.store(true, Ordering::Relaxed);
        let failed = results.iter().find_map(|(_, failed)| failed.clone());
        assert_eq!(failed, None, "a capture waited for an unrelated child");
        assert!(results.iter().all(|(runs, _)| *runs > 0));
    });
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
