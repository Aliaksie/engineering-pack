//! `task-step` as an agent runs it after each task: the packages the host installed beside the
//! runtime are found without `--bundle`, a finished task needs no next step, and a missing
//! option is named by its option.
mod support;
use std::{fs, path::Path, process::Command};
use support::Repo;

const SESSION: &str = "S1";

/// Runs the built `craft` as a host's launcher does, with `package` as its package root.
fn craft(repo: &Repo, package: Option<&Path>, args: &[&str]) -> (i32, String, String) {
    let mut command = Command::new(env!("CARGO_BIN_EXE_craft"));
    command
        .arg(args[0])
        .args(["--root", repo.path()])
        .args(&args[1..]);
    command.env_remove("CRAFT_PACKAGE_ROOT");
    if let Some(package) = package {
        command.env("CRAFT_PACKAGE_ROOT", package);
    }
    let output = command.output().unwrap();
    (
        output.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&output.stdout).into_owned(),
        String::from_utf8_lossy(&output.stderr).into_owned(),
    )
}

/// Two packages installed side by side, as an export's plugins folder holds them.
fn installed(repo: &Repo) -> std::path::PathBuf {
    let plugins = Path::new(repo.path()).with_extension("plugins");
    for name in ["craft-delivery", "craft-engineering"] {
        let root = plugins.join(name);
        support::installed_bundle(&root, "0.0.0");
        fs::write(
            root.join("source-capability.json"),
            format!(r#"{{"name":"{name}"}}"#),
        )
        .unwrap();
    }
    plugins.join("craft-delivery")
}

const STEP: &[&str] = &[
    "task-step",
    "--session",
    SESSION,
    "--stage",
    "verify",
    "--target",
    "acceptance:value",
    "--approach",
    "bounded-check",
    "--evidence",
    "checks:value",
    "--outcome",
    "passed",
    "--result",
    "Value checked",
];

#[test]
fn a_finished_task_is_recorded_without_bundle_or_next_step() {
    let repo = Repo::new("task-step-installed", &[("value.txt", "one\n")]);
    let package = installed(&repo);
    let (code, _, stderr) = craft(
        &repo,
        Some(&package),
        &[
            "task-start",
            "--session",
            SESSION,
            "--task",
            "t1",
            "--goal",
            "Fix the value",
        ],
    );
    assert_eq!(code, 0, "{stderr}");
    let mut finished = STEP.to_vec();
    finished.push("--complete");
    let (code, stdout, stderr) = craft(&repo, Some(&package), &finished);
    assert_eq!(code, 0, "{stderr}");
    assert!(stdout.contains("Task: complete"), "{stdout}");
    assert!(!stdout.contains("stale"), "{stdout}");
    let _ = fs::remove_dir_all(package.parent().unwrap());
}

#[test]
fn missing_options_are_named_by_their_option() {
    let repo = Repo::new("task-step-missing", &[("value.txt", "one\n")]);
    let package = installed(&repo);
    let (code, _, stderr) = craft(
        &repo,
        Some(&package),
        &[
            "task-start",
            "--session",
            SESSION,
            "--task",
            "t1",
            "--goal",
            "Fix the value",
        ],
    );
    assert_eq!(code, 0, "{stderr}");
    let (code, _, stderr) = craft(&repo, Some(&package), STEP);
    assert_ne!(code, 0);
    assert!(
        stderr.contains(
            "task-step requires --next; a finished task takes --complete instead of --next"
        ),
        "{stderr}"
    );
    let partial: Vec<&str> = STEP
        .iter()
        .copied()
        .filter(|a| !["--target", "acceptance:value"].contains(a))
        .collect();
    let (_, _, stderr) = craft(&repo, Some(&package), &partial);
    assert!(
        stderr.contains("task-step requires --target, --next"),
        "{stderr}"
    );
    // Outside any installed package, the option to pass is named.
    let mut next = STEP.to_vec();
    next.extend(["--next", "Review"]);
    let (code, _, stderr) = craft(&repo, None, &next);
    assert_ne!(code, 0);
    assert!(
        stderr.contains("found no installed package beside this runtime; pass --bundle"),
        "{stderr}"
    );
    let _ = fs::remove_dir_all(package.parent().unwrap());
}

/// A host that runs the bundled executable directly, without the launcher, as Codex does:
/// the package it was bundled in is found from the executable's own place.
#[cfg(unix)]
#[test]
fn the_bundled_executable_finds_its_package_without_the_launcher() {
    let repo = Repo::new("task-step-bundled", &[("value.txt", "one\n")]);
    let package = installed(&repo);
    let executable = package.join("support/native-bin/test-target/native-bin/craft");
    fs::copy(env!("CARGO_BIN_EXE_craft"), &executable).unwrap();
    let run = |args: &[&str]| {
        let output = Command::new(&executable)
            .arg(args[0])
            .args(["--root", repo.path()])
            .args(&args[1..])
            .env_remove("CRAFT_PACKAGE_ROOT")
            .output()
            .unwrap();
        (
            output.status.code(),
            String::from_utf8_lossy(&output.stderr).into_owned(),
        )
    };
    let started = run(&[
        "task-start",
        "--session",
        SESSION,
        "--task",
        "t1",
        "--goal",
        "Fix it",
    ]);
    assert_eq!(started.0, Some(0), "{}", started.1);
    let mut step = STEP.to_vec();
    step.extend(["--next", "Review"]);
    let stepped = run(&step);
    assert_eq!(stepped.0, Some(0), "{}", stepped.1);
    let _ = fs::remove_dir_all(package.parent().unwrap());
}
