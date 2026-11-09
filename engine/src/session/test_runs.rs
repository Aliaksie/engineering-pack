//! Test runs: whether a shell command runs a test suite, and what its output says happened.
//!
//! The result is read from the runner's own summary line, never guessed from other output.
//! A run whose exit code says it failed is failed; a summary Craft does not recognise is
//! `unknown`, never `passed`; a run in which no test executed is `failed`, because a filter
//! that matches nothing exits 0 on most runners.

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
    // Wrappers that run another command: `npx jest`, `uv run pytest`, `python -m pytest`.
    loop {
        let skip = match words.as_slice() {
            ["npx" | "bunx" | "env" | "time" | "nice" | "exec", ..] => 1,
            [
                "pnpm" | "yarn" | "uv" | "poetry" | "pdm" | "hatch",
                "exec" | "run",
                ..,
            ] => 2,
            [python, "-m", ..] if python.starts_with("python") => 2,
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
    let Some((program, args)) = words.split_first() else {
        return false;
    };
    let program = program.rsplit('/').next().unwrap_or(program);
    let has = |arg: &str| args.contains(&arg);
    let script = |arg: &&str| *arg == "test" || arg.starts_with("test:");
    match program {
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
    let (result, reason) = match (exit, counts) {
        (Some(code), _) if code != 0 => ("failed", "exit-code"),
        (_, None) => ("unknown", "unrecognized-output"),
        (_, Some(counts)) if counts.failed > 0 => ("failed", "failures"),
        (_, Some(counts)) if counts.executed == 0 && counts.skipped > 0 => {
            ("failed", "all-skipped")
        }
        (_, Some(counts)) if counts.executed == 0 => ("failed", "zero-tests"),
        (_, Some(_)) => ("passed", "summary"),
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
    fn output_cut_before_the_summary_is_unknown() {
        let verdict = classify(Some(0), "running 3 tests\ntest a ... ok\n");
        assert_eq!(verdict["result"], "unknown");
    }
}
