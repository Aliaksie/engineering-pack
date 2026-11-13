//! Test runs: whether a shell command runs a test suite, and what its output says happened.
//!
//! The result is read from the runner's own summary line, never guessed from other output.
//! A run whose exit code says it failed is failed; a summary Craft does not recognise is
//! `unknown`, never `passed`; a run in which no test executed is `failed`, because a filter
//! that matches nothing exits 0 on most runners. Output a pipe cut short (`| tail -5`) may show
//! only the last of several summaries, so from it only a failure it shows is decisive.

use serde_json::{Value, json};

/// Counts read from a runner summary.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
struct Counts {
    executed: u64,
    failed: u64,
    skipped: u64,
}

impl Counts {
    fn add(&mut self, other: Counts) {
        self.executed += other.executed;
        self.failed += other.failed;
        self.skipped += other.skipped;
    }
}

/// What a runner's summary states: counts, or that the runner itself reported no tests.
enum Summary {
    Counts(Counts),
    NoTests,
}

/// Whether one command in a shell line runs tests. The command line may chain several
/// commands; each is looked at after environment assignments and runner wrappers.
pub fn is_test_command(command: &str) -> bool {
    command
        .split(['|', ';', '&', '\n', '(', ')'])
        .any(|segment| runs_tests(segment.split_whitespace().collect()))
}

fn runs_tests(words: Vec<&str>) -> bool {
    let mut words: Vec<&str> = words
        .into_iter()
        .skip_while(|w| w.contains('=') && !w.starts_with('-'))
        .collect();
    // Wrappers that run another command: `npx jest`, `uv run pytest`, `python -m pytest`,
    // `py -3 -m pytest`.
    loop {
        let head = words.first().map_or_else(String::new, |word| program(word));
        let skip = match (head.as_str(), words.get(1).copied()) {
            ("npx" | "bunx" | "env" | "time" | "nice" | "exec", _) => 1,
            ("pnpm" | "yarn" | "uv" | "poetry" | "pdm" | "hatch", Some("exec" | "run")) => 2,
            (python, Some("-m")) if python.starts_with("python") => 2,
            ("py", _) => match words.iter().position(|w| *w == "-m") {
                Some(at) if words[1..at].iter().all(|w| w.starts_with('-')) => at + 1,
                _ => 0,
            },
            _ => 0,
        };
        if skip == 0 || words.len() <= skip {
            break;
        }
        words.drain(..skip);
        while words.first().is_some_and(|w| w.starts_with('-')) {
            words.remove(0);
        }
    }
    let Some((first, args)) = words.split_first() else {
        return false;
    };
    let program = program(first);
    let has = |arg: &str| args.contains(&arg);
    let script = |arg: &&str| *arg == "test" || arg.starts_with("test:");
    match program.as_str() {
        "cargo" => has("test") || has("nextest"),
        "go" | "dotnet" | "bun" | "deno" => args.first() == Some(&"test"),
        "node" => args
            .iter()
            .any(|a| *a == "--test" || a.starts_with("--test=")),
        "npm" | "pnpm" | "yarn" => {
            matches!(args.first(), Some(&"test" | &"t"))
                || (args.first() == Some(&"run") && args.get(1).is_some_and(script))
                || args.first().is_some_and(script)
        }
        "jest" | "vitest" | "pytest" | "py.test" | "unittest" | "mocha" | "rspec" => true,
        "mvn" | "mvnw" | "gradle" | "gradlew" => args
            .iter()
            .any(|a| matches!(*a, "test" | "verify" | "check" | "build" | "install")),
        "make" | "just" => args.iter().any(|a| *a == "test" || *a == "check"),
        _ => false,
    }
}

/// A command word as the program it names: its file name without a Windows executable
/// extension, so `./gradlew`, `.\gradlew.bat` and `C:\bin\cargo.exe` name `gradlew` and `cargo`.
fn program(word: &str) -> String {
    let base = word.rsplit(['/', '\\']).next().unwrap_or(word);
    let lower = base.to_ascii_lowercase();
    [".exe", ".cmd", ".bat"]
        .iter()
        .find_map(|ext| lower.strip_suffix(ext))
        .map_or_else(|| base.to_string(), str::to_string)
}

/// The runner's verdict on a test command's output: `{result, runner, executed, failed,
/// skipped, reason, exitCode}`. `exit` is the shell exit code when the host reports one.
pub fn classify(exit: Option<i64>, output: &str) -> Value {
    let found = summary(output);
    let runner = found.as_ref().map(|(runner, _)| *runner);
    let counts = match &found {
        Some((_, Summary::Counts(counts))) => Some(*counts),
        Some((_, Summary::NoTests)) => Some(Counts::default()),
        None => None,
    };
    // A summary of no executed and no failed test: the runner checked nothing, whether it
    // then exited 0 or, as some do for an empty selection, non-zero.
    let empty = counts
        .filter(|counts| counts.executed == 0 && counts.failed == 0)
        .map(|counts| {
            if counts.skipped > 0 {
                "all-skipped"
            } else {
                "zero-tests"
            }
        });
    let (result, reason) = match (exit, counts, empty) {
        (_, _, Some(empty)) => ("failed", empty),
        (Some(code), _, _) if code != 0 => ("failed", "exit-code"),
        (_, None, _) => ("unknown", "unrecognized-output"),
        (_, Some(counts), _) if counts.failed > 0 => ("failed", "failures"),
        (_, Some(_), _) => ("passed", "summary"),
    };
    let mut verdict =
        json!({"result": result, "reason": reason, "runner": runner, "exitCode": exit});
    if let Some(counts) = counts {
        verdict["executed"] = json!(counts.executed);
        verdict["failed"] = json!(counts.failed);
        verdict["skipped"] = json!(counts.skipped);
    }
    verdict
}

/// The verdict on a shell command's run of tests, as [`classify`], for output the command
/// may have cut: after a filter such as `tail` or `grep` the summary seen can be the last of
/// several (one per test binary or module), so an empty or passing one is `unknown` with the
/// reason `cut-output`. A failure the cut output shows still fails the run.
pub fn classify_command(exit: Option<i64>, output: &str, command: &str) -> Value {
    let mut verdict = classify(exit, output);
    let decisive = verdict["result"] == "failed"
        && matches!(verdict["reason"].as_str(), Some("failures" | "exit-code"));
    if !decisive && cut_output(command) {
        verdict["result"] = json!("unknown");
        verdict["reason"] = json!("cut-output");
    }
    verdict
}

/// Whether a test command's output passes through a later stage of its pipeline that can
/// drop lines: any program but `tee` and `cat`.
fn cut_output(command: &str) -> bool {
    command
        .split(['\n', ';'])
        .flat_map(|line| line.split("&&"))
        .flat_map(|list| list.split("||"))
        .any(|pipeline| {
            let stages: Vec<&str> = pipeline
                .split('|')
                .map(|stage| stage.trim_start_matches('&'))
                .collect();
            stages.iter().enumerate().any(|(at, stage)| {
                runs_tests(stage.split_whitespace().collect())
                    && stages[at + 1..].iter().any(|later| {
                        let program = later
                            .split_whitespace()
                            .find(|word| !(word.contains('=') && !word.starts_with('-')))
                            .map(program)
                            .unwrap_or_default();
                        !program.is_empty() && program != "tee" && program != "cat"
                    })
            })
        })
}

type Parser = fn(&[&str]) -> Option<Summary>;

/// The first runner whose summary the output carries.
fn summary(output: &str) -> Option<(&'static str, Summary)> {
    let lines: Vec<&str> = output.lines().map(str::trim_end).collect();
    const RUNNERS: [(&str, Parser); 7] = [
        ("cargo", cargo),
        ("maven", maven),
        ("jest", jest),
        ("vitest", vitest),
        ("pytest", pytest),
        ("unittest", unittest),
        ("node", node),
    ];
    RUNNERS
        .iter()
        .find_map(|(runner, parse)| parse(&lines).map(|summary| (*runner, summary)))
}

/// `N word` pairs in a comma- or pipe-separated summary: `1 failed, 2 passed`.
fn tokens(text: &str) -> Vec<(u64, &str)> {
    text.split([',', '|'])
        .filter_map(|part| {
            let mut words = part.split_whitespace();
            let count = words.next()?.parse().ok()?;
            Some((count, words.next()?))
        })
        .collect()
}

fn sum(tokens: &[(u64, &str)], names: &[&str]) -> u64 {
    tokens
        .iter()
        .filter(|(_, name)| names.contains(name))
        .map(|(count, _)| count)
        .sum()
}

/// The number after `label` up to the next non-digit.
fn number_after(text: &str, label: &str) -> Option<u64> {
    let rest = &text[text.find(label)? + label.len()..];
    let digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
    digits.parse().ok()
}

/// `test result: ok. 3 passed; 0 failed; 1 ignored; …`, summed over every test binary.
fn cargo(lines: &[&str]) -> Option<Summary> {
    let mut total: Option<Counts> = None;
    for line in lines {
        let Some(rest) = line.trim_start().strip_prefix("test result: ") else {
            continue;
        };
        let parts: Vec<(u64, &str)> = rest
            .split(['.', ';'])
            .filter_map(|part| {
                let mut words = part.split_whitespace();
                Some((words.next()?.parse().ok()?, words.next()?))
            })
            .collect();
        let passed = sum(&parts, &["passed"]);
        let failed = sum(&parts, &["failed"]);
        total.get_or_insert_default().add(Counts {
            executed: passed + failed,
            failed,
            skipped: sum(&parts, &["ignored"]),
        });
    }
    total.map(Summary::Counts)
}

/// Surefire's per-module aggregate `Tests run: 3, Failures: 0, Errors: 0, Skipped: 0` (the
/// per-class lines end with `-- in <class>`), or `Tests are skipped.`
fn maven(lines: &[&str]) -> Option<Summary> {
    let mut total: Option<Counts> = None;
    for line in lines {
        let Some(at) = line.find("Tests run: ") else {
            continue;
        };
        if !line.starts_with('[') || line.contains("Time elapsed") || line.contains(" -- in ") {
            continue;
        }
        let text = &line[at..];
        let (Some(run), Some(failures), Some(errors), Some(skipped)) = (
            number_after(text, "Tests run: "),
            number_after(text, "Failures: "),
            number_after(text, "Errors: "),
            number_after(text, "Skipped: "),
        ) else {
            continue;
        };
        total.get_or_insert_default().add(Counts {
            executed: run.saturating_sub(skipped),
            failed: failures + errors,
            skipped,
        });
    }
    if let Some(total) = total {
        return Some(Summary::Counts(total));
    }
    lines
        .iter()
        .any(|line| line.ends_with("Tests are skipped.") || line.contains("were executed!"))
        .then_some(Summary::NoTests)
}

/// `Tests:       1 failed, 2 passed, 3 total`, or `No tests found`.
fn jest(lines: &[&str]) -> Option<Summary> {
    if let Some(line) = lines.iter().rev().find_map(|l| l.strip_prefix("Tests:")) {
        let found = tokens(line);
        let failed = sum(&found, &["failed"]);
        return Some(Summary::Counts(Counts {
            executed: sum(&found, &["passed"]) + failed,
            failed,
            skipped: sum(&found, &["skipped", "todo", "pending"]),
        }));
    }
    lines
        .iter()
        .any(|line| line.starts_with("No tests found"))
        .then_some(Summary::NoTests)
}

/// `      Tests  1 failed | 2 passed (3)` or `Tests  no tests`, or `No test files found`.
fn vitest(lines: &[&str]) -> Option<Summary> {
    let summary = lines.iter().rev().find_map(|line| {
        let rest = line.trim_start().strip_prefix("Tests  ")?;
        (rest.ends_with(')') || rest.trim() == "no tests").then_some(rest)
    });
    if let Some(rest) = summary {
        let rest = rest.rsplit_once('(').map_or(rest, |(counts, _)| counts);
        let found = tokens(rest);
        let failed = sum(&found, &["failed"]);
        return Some(Summary::Counts(Counts {
            executed: sum(&found, &["passed"]) + failed,
            failed,
            skipped: sum(&found, &["skipped", "todo"]),
        }));
    }
    lines
        .iter()
        .any(|line| line.starts_with("No test files found"))
        .then_some(Summary::NoTests)
}

/// `===== 1 failed, 2 passed in 0.03s =====`, its `-q` form without the rule, or `no tests ran`.
fn pytest(lines: &[&str]) -> Option<Summary> {
    let line = lines.iter().rev().find_map(|line| {
        let text = line.trim_matches('=').trim();
        let (counts, time) = text.rsplit_once(" in ")?;
        let time = time.split_whitespace().next()?;
        let seconds = time.strip_suffix('s')?;
        seconds.parse::<f64>().ok()?;
        (counts == "no tests ran" || !tokens(counts).is_empty()).then_some(counts)
    })?;
    if line == "no tests ran" {
        return Some(Summary::NoTests);
    }
    let found = tokens(line);
    const KNOWN: [&str; 10] = [
        "passed",
        "failed",
        "error",
        "errors",
        "skipped",
        "deselected",
        "xfailed",
        "xpassed",
        "warning",
        "warnings",
    ];
    if found.iter().any(|(_, name)| !KNOWN.contains(name)) {
        return None;
    }
    let failed = sum(&found, &["failed", "error", "errors"]);
    Some(Summary::Counts(Counts {
        executed: sum(&found, &["passed", "xfailed", "xpassed"]) + failed,
        failed,
        skipped: sum(&found, &["skipped", "deselected"]),
    }))
}

/// `Ran 3 tests in 0.001s` then `OK (skipped=1)` / `FAILED (failures=1, errors=1)`.
fn unittest(lines: &[&str]) -> Option<Summary> {
    let at = lines
        .iter()
        .rposition(|line| line.starts_with("Ran ") && line.contains(" in "))?;
    let ran = number_after(lines[at], "Ran ")?;
    let verdict = lines[at + 1..]
        .iter()
        .find(|line| !line.trim().is_empty())
        .copied()
        .unwrap_or("");
    let skipped = number_after(verdict, "skipped=").unwrap_or(0);
    let failed = number_after(verdict, "failures=").unwrap_or(0)
        + number_after(verdict, "errors=").unwrap_or(0)
        + number_after(verdict, "unexpected successes=").unwrap_or(0);
    Some(Summary::Counts(Counts {
        executed: ran.saturating_sub(skipped),
        failed,
        skipped,
    }))
}

/// `ℹ tests 3` (spec), `# tests 3` (TAP) or `<!-- tests 3 -->` (JUnit). With process
/// isolation a test file in which no test ran is itself reported as one passing test, so
/// passing top-level entries that name a test file are not counted as tests.
fn node(lines: &[&str]) -> Option<Summary> {
    let value = |name: &str| {
        lines.iter().rev().find_map(|line| {
            let line = line.trim();
            let rest = line
                .strip_prefix("ℹ ")
                .or_else(|| line.strip_prefix("# "))
                .or_else(|| {
                    line.strip_prefix("<!-- ")
                        .and_then(|l| l.strip_suffix(" -->"))
                })?;
            rest.strip_prefix(name)?
                .strip_prefix(' ')?
                .trim()
                .parse::<u64>()
                .ok()
        })
    };
    value("tests")?;
    let pass = value("pass")?;
    let failed = value("fail").unwrap_or(0) + value("cancelled").unwrap_or(0);
    let files = lines.iter().filter(|line| file_entry(line)).count() as u64;
    Some(Summary::Counts(Counts {
        executed: pass.saturating_sub(files) + failed,
        failed,
        skipped: value("skipped").unwrap_or(0) + value("todo").unwrap_or(0),
    }))
}

/// A top-level passing node test result whose name is a test file path.
fn file_entry(line: &str) -> bool {
    let name = if let Some(rest) = line.strip_prefix("✔ ") {
        rest.rsplit_once(" (").map_or(rest, |(name, _)| name)
    } else if let Some(rest) = line.strip_prefix("ok ") {
        rest.split_once(" - ").map_or("", |(_, name)| name)
    } else if let Some(rest) = line.trim_start().strip_prefix("<testcase name=\"") {
        if line.contains("<failure") {
            return false;
        }
        rest.split_once('"').map_or("", |(name, _)| name)
    } else {
        return false;
    };
    let name = name.trim();
    !name.contains(' ')
        && [".js", ".mjs", ".cjs", ".ts", ".mts", ".cts", ".jsx", ".tsx"]
            .iter()
            .any(|ext| name.ends_with(ext))
}

/// How many observed runs a session keeps, newest last.
pub const KEPT_RUNS: usize = 20;
/// How much of a command an observation keeps.
const COMMAND_CHARS: usize = 256;

/// One observed run: the verdict, the command (cut to its first characters), when, on which
/// host, and the source digest the run saw (`null` when the tree could not be fingerprinted).
pub fn observation(verdict: &Value, command: &str, host: &str, at: &str, source: &Value) -> Value {
    let mut run = verdict.clone();
    run["command"] = json!(command.chars().take(COMMAND_CHARS).collect::<String>());
    run["host"] = json!(host);
    run["at"] = json!(at);
    run["source"] = source.clone();
    run
}

/// The session's run history with `run` added, oldest runs dropped beyond [`KEPT_RUNS`].
pub fn observed(history: &Value, run: Value) -> Value {
    let mut runs: Vec<Value> = history["runs"].as_array().cloned().unwrap_or_default();
    runs.push(run);
    let excess = runs.len().saturating_sub(KEPT_RUNS);
    runs.drain(..excess);
    json!({"formatVersion": 1, "runs": runs})
}

/// Whether a run decides anything about the code: it passed, or tests failed or the runner
/// exited with a failure. A run that executed no test, or whose output Craft could not read,
/// checked nothing that could confirm or contradict an earlier run.
fn decisive(run: &Value) -> bool {
    match run["result"].as_str() {
        Some("passed") => true,
        Some("failed") => !matches!(run["reason"].as_str(), Some("zero-tests" | "all-skipped")),
        _ => false,
    }
}

/// What the tests say about `source`, the current tree: the latest decisive run observed on
/// it, or when none was decisive the latest run at all. A run on an earlier tree, or on a
/// tree that could not be fingerprinted, says nothing about it.
pub fn current<'a>(history: &'a Value, source: &Value) -> Option<&'a Value> {
    if source.is_null() {
        return None;
    }
    let mut on_tree = history["runs"]
        .as_array()?
        .iter()
        .rev()
        .filter(|run| &run["source"] == source);
    let latest = on_tree.clone().next()?;
    Some(on_tree.find(|run| decisive(run)).unwrap_or(latest))
}

/// A ledger record that asks for the observed run (`"tests": "observed"`), completed from
/// `run`, the run [`current`] picks on the current tree: its counts, verdict, tree and
/// time replace whatever the record declared about them.
pub fn evidence(record: &Value, run: &Value) -> Result<Value, String> {
    let execution = match run["result"].as_str() {
        Some("passed") => "passed",
        Some("failed") => "failed",
        _ => {
            return Err("the latest test run observed on the current tree has no summary Craft recognizes; record it as declared evidence".into());
        }
    };
    let mut record = record.clone();
    record["tests"] = observed_tests(run);
    record["execution"] = json!(execution);
    record["support"] = json!("verified");
    record["binding"] = json!({"kind": "repository", "identity": run["source"]});
    record["observedAt"] = run["at"].clone();
    for (field, value) in [("artifact", &run["command"]), ("basis", &run["command"])] {
        if record.get(field).is_none() {
            record[field] = value.clone();
        }
    }
    if record.get("reporter").is_none() {
        record["reporter"] = json!(format!(
            "craft observed on {}",
            run["host"].as_str().unwrap_or("host")
        ));
    }
    Ok(record)
}

/// The `tests` an observed run gives a ledger record. Counts the runner's summary did not
/// report (a failure exit with no recognized summary) stay out: unknown, not zero.
fn observed_tests(run: &Value) -> Value {
    let mut tests = json!({"runner": run["runner"], "source": "observed"});
    for count in ["executed", "failed", "skipped"] {
        if let Some(n) = run[count].as_u64() {
            tests[count] = json!(n);
        }
    }
    tests
}

/// Whether a submitted record's `tests` may say `source: observed`: Craft gave it, either now
/// from a run in `history` (same tree, time and counts) or in the ledger already `saved`.
/// A record that declares its own counts says `declared`.
pub fn vouched(record: &Value, history: &Value, saved: &Value) -> bool {
    if record["tests"]["source"] != "observed" {
        return true;
    }
    let kept = saved["records"]
        .as_array()
        .is_some_and(|records| records.contains(record));
    kept || history["runs"].as_array().is_some_and(|runs| {
        runs.iter().any(|run| {
            !run["source"].is_null()
                && record["binding"]["identity"] == run["source"]
                && record["observedAt"] == run["at"]
                && record["tests"] == observed_tests(run)
        })
    })
}

/// Why a declared passing verification is refused: the run [`current`] picks on the current
/// tree failed. A tree with no observed run leaves the declaration as it was.
pub fn refutes_pass(status: &Value) -> Option<String> {
    (status["status"] == "failed").then(|| {
        format!(
            "verification passed contradicts the test run that decides the current tree ({} {}, `{}`); rerun the tests or record a quality-failure",
            status["runner"].as_str().unwrap_or("unknown runner"),
            status["reason"].as_str().unwrap_or("failed"),
            status["command"].as_str().unwrap_or_default(),
        )
    })
}

/// The test status of the current tree: `passed`, `failed` or `unknown` by the run
/// [`current`] picks on it; `not-run` when none was observed.
pub fn status(history: &Value, source: &Value) -> Value {
    match current(history, source) {
        Some(run) => json!({
            "status": run["result"], "reason": run["reason"], "runner": run["runner"],
            "executed": run["executed"], "at": run["at"], "command": run["command"],
        }),
        None => json!({"status": "not-run"}),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_commands_are_recognised_through_wrappers_and_chains() {
        for command in [
            "cargo test -q -p craft-engine --lib standards",
            "cd app && npm test",
            "npm run test:unit",
            "pnpm test",
            "CI=1 npx --no-install vitest run",
            "uv run pytest -k orders",
            "python3 -m pytest tests/",
            "python -m unittest discover",
            "node --test tests/*.test.mjs 2>&1 | tail -5",
            "./mvnw -B verify",
            "./gradlew test",
            "go test ./...",
            ".\\gradlew.bat test",
            "gradlew test",
            "mvnw.cmd -B verify",
            "npm.cmd test",
            "C:\\Users\\dev\\.cargo\\bin\\cargo.exe test",
            "py -m pytest",
            "py -3.12 -m pytest -q",
        ] {
            assert!(is_test_command(command), "{command}");
        }
        for command in [
            "cargo build",
            "git commit -m test",
            "grep -rn test src",
            "npm install",
            "node scripts/build.mjs",
            "ls tests",
            "py -c test",
            "cargo.exe build",
        ] {
            assert!(!is_test_command(command), "{command}");
        }
    }

    #[test]
    fn a_success_text_from_an_unknown_runner_is_unknown_and_its_failure_failed() {
        let ok = classify(Some(0), "All good! 12 checks fine.\n");
        assert_eq!(ok["result"], "unknown");
        assert_eq!(ok["runner"], Value::Null);
        assert_eq!(classify(Some(1), "boom\n")["result"], "failed");
    }

    #[test]
    fn a_summary_with_failures_fails_even_when_the_exit_code_was_lost_in_a_pipe() {
        let output = "test result: FAILED. 2 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s\n";
        let verdict = classify(Some(0), output);
        assert_eq!(verdict["result"], "failed");
        assert_eq!(verdict["reason"], "failures");
        // No exit code from the host: the summary alone decides.
        assert_eq!(classify(None, output)["result"], "failed");
    }

    #[test]
    fn output_a_pipe_cut_says_nothing_about_tests_it_does_not_show() {
        // Cargo ran a library with 10 tests; `tail` kept only the last binary's summary.
        let last = "running 0 tests\n\ntest result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 1 filtered out; finished in 0.00s\n";
        let cut = "cargo test -q -p craft-engine layers 2>&1 | tail -4";
        assert_eq!(classify(Some(0), last)["reason"], "zero-tests");
        let verdict = classify_command(Some(0), last, cut);
        assert_eq!(
            (verdict["result"].as_str(), verdict["reason"].as_str()),
            (Some("unknown"), Some("cut-output"))
        );
        let passed = "test result: ok. 3 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s\n";
        assert_eq!(classify_command(Some(0), passed, cut)["result"], "unknown");
        let failed = "test result: FAILED. 9 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s\n";
        assert_eq!(classify_command(Some(0), failed, cut)["reason"], "failures");
        // Whole output: kept by `tee` and `cat`, or a pipe that comes before the runner.
        for whole in [
            "cargo test 2>&1 | tee test.log",
            "cargo test | cat",
            "printf 'y' | cargo test",
            "cargo test && git status | head -3",
            "cargo test || echo failed | tail -1",
        ] {
            assert_eq!(
                classify_command(Some(0), last, whole)["reason"],
                "zero-tests",
                "{whole}"
            );
        }
        for cut in [
            "npm test |& tail -5",
            "pytest -q | grep -v PASSED",
            "cargo test 2>&1 | LC_ALL=C sed -n '1,40p'",
        ] {
            assert_eq!(
                classify_command(Some(0), last, cut)["reason"],
                "cut-output",
                "{cut}"
            );
        }
    }

    #[test]
    fn output_cut_before_the_summary_is_unknown() {
        let verdict = classify(Some(0), "running 3 tests\ntest a ... ok\n");
        assert_eq!(verdict["result"], "unknown");
    }

    #[test]
    fn only_a_run_on_the_current_tree_speaks_for_it() {
        let pass = classify(
            Some(0),
            "test result: ok. 3 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s\n",
        );
        let zero = classify(
            Some(0),
            "test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 3 filtered out; finished in 0.00s\n",
        );
        let mut history = Value::Null;
        history = observed(
            &history,
            observation(&pass, "cargo test", "claude", "t1", &json!("A")),
        );
        history = observed(
            &history,
            observation(&zero, "cargo test nomatch", "claude", "t2", &json!("B")),
        );
        assert_eq!(status(&history, &json!("A"))["status"], "passed");
        assert_eq!(status(&history, &json!("B"))["status"], "failed");
        assert_eq!(status(&history, &json!("B"))["reason"], "zero-tests");
        // An edit since the run: the tree has no run yet.
        assert_eq!(status(&history, &json!("C"))["status"], "not-run");
        // An unfingerprinted tree is never matched.
        let blind = observed(
            &Value::Null,
            observation(&pass, "cargo test", "codex", "t3", &Value::Null),
        );
        assert_eq!(status(&blind, &Value::Null)["status"], "not-run");
    }

    #[test]
    fn a_run_that_checked_nothing_leaves_an_earlier_verdict_on_the_tree_standing() {
        let pass = classify(
            Some(0),
            "test result: ok. 3 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s\n",
        );
        let fail = classify(
            Some(101),
            "test result: FAILED. 2 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s\n",
        );
        let zero = classify(Some(0), "[INFO] Tests are skipped.\n");
        let unread = classify(Some(0), "");
        let empty_exit = classify(Some(1), "No tests found, exiting with code 1\n");
        assert_eq!(empty_exit["reason"], "zero-tests");
        let at = |history: &Value, verdict: &Value, command: &str| {
            observed(
                history,
                observation(verdict, command, "claude", "t", &json!("A")),
            )
        };
        let mut history = at(&Value::Null, &pass, "cargo test");
        for (verdict, command) in [
            (&zero, "mvn -DskipTests install"),
            (&unread, "cargo test --no-run"),
            (&empty_exit, "npx jest nomatch"),
        ] {
            history = at(&history, verdict, command);
            assert_eq!(
                status(&history, &json!("A"))["status"],
                "passed",
                "{command}"
            );
            assert_eq!(refutes_pass(&status(&history, &json!("A"))), None);
        }
        // A failure is decisive and stands over the pass before it, whatever follows it.
        history = at(&history, &fail, "cargo test");
        history = at(&history, &zero, "cargo test nomatch");
        assert_eq!(status(&history, &json!("A"))["reason"], "exit-code");
        assert!(refutes_pass(&status(&history, &json!("A"))).is_some());
        // With no decisive run, the latest run is what the tree has.
        let alone = at(&Value::Null, &zero, "cargo test nomatch");
        assert_eq!(status(&alone, &json!("A"))["reason"], "zero-tests");
    }

    #[test]
    fn observed_counts_are_vouched_for_only_by_a_run_or_the_saved_ledger() {
        let fail = classify(Some(2), "boom\n");
        let pass = classify(Some(0), "Ran 2 tests in 0.0s\n\nOK\n");
        let mut history = Value::Null;
        for (verdict, at) in [(&fail, "t1"), (&pass, "t2")] {
            history = observed(
                &history,
                observation(verdict, "pytest", "codex", at, &json!("A")),
            );
        }
        let asked = json!({"id": "r", "tests": "observed"});
        let filled = evidence(&asked, current(&history, &json!("A")).unwrap()).unwrap();
        assert_eq!(filled["tests"]["executed"], 2);
        assert!(vouched(&filled, &history, &Value::Null));
        // A failure without a summary has no counts: unknown, not zero.
        let failed = evidence(&asked, &history["runs"][0]).unwrap();
        assert_eq!(
            failed["tests"],
            json!({"runner": null, "source": "observed"})
        );
        // Counts changed by hand, or a run Craft never saw, are not vouched for...
        let mut forged = filled.clone();
        forged["tests"]["executed"] = json!(9);
        assert!(!vouched(&forged, &history, &Value::Null));
        // ...unless the saved ledger already holds the record as it is.
        let saved = json!({"formatVersion": 2, "records": [forged.clone()]});
        assert!(vouched(&forged, &Value::Null, &saved));
        let declared = json!({"tests": {"source": "declared", "runner": "x", "executed": 1}});
        assert!(vouched(&declared, &Value::Null, &Value::Null));
    }

    #[test]
    fn the_history_keeps_the_latest_runs_and_short_commands() {
        let pass = classify(Some(0), "Ran 1 test in 0.0s\n\nOK\n");
        let mut history = Value::Null;
        for n in 0..KEPT_RUNS + 5 {
            let run = observation(
                &pass,
                &format!("pytest {n} {}", "x".repeat(400)),
                "codex",
                "t",
                &json!(n),
            );
            history = observed(&history, run);
        }
        let runs = history["runs"].as_array().unwrap();
        assert_eq!(runs.len(), KEPT_RUNS);
        assert_eq!(runs[0]["source"], 5);
        assert_eq!(
            runs[0]["command"].as_str().unwrap().chars().count(),
            COMMAND_CHARS
        );
    }
}
