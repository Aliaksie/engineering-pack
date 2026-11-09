# Contributing

You need Node 24 (22 minimum), Rust 1.90+ with Cargo (CI pins 1.99.1) and a C compiler for the
bundled SQLite and Tree-sitter sources. The locked Tree-sitter crates require Rust 1.90.
There is no npm install.

```sh
npm run check        # validate, build exports, check architecture, lint, test
npm test             # build exports and the debug craft executable, then run the tests
cargo test --workspace --locked
```

Tests use the debug `craft` build, or `CRAFT_NATIVE_BINARY` when set. Generated output in
`dist/` is reproducible; never edit it by hand.

## Where changes go

| Change | Location |
|---|---|
| Guidance, workflows, standards, evals | `packages/<name>/`, registered in its `capability.json` ([package format](PACKAGE-FORMAT.md)) |
| Decisions over explicit facts (no I/O, no clock) | `engine/` |
| Use cases, CLI, hook handling | `application/` |
| Filesystem, Git, processes, network | `platform/` |
| Host hook mapping and feature tables | `hosts/` |
| Build, export, validation, release | `tooling/` |

Runtime behavior is Rust; JavaScript stays in tooling, tests and the small build-time host
tables and hook declarations ([ADR 0001](adr/0001-rust-core-runtime.md)). New top-level
modules follow [ADR 0002](adr/0002-module-map-and-boundaries.md).

- Keep common principles, language, application-layer and provider guidance independent; a
  stack example does not impose that stack.
- Change the craft bar in `packages/craft-engineering/standards/craft-bar.md`, then run
  `npm run sync:craft-bar`.
- Keep the current version until a release is requested ([releasing](RELEASING.md)).
- Test behavior, not prose. Use real Git fixtures and hostile hook payloads; service tests
  never contact a real tracker or code host. Workflow and instruction changes need
  [eval cases](EVALUATION.md#package-scenarios) with recorded responses.
- `npm run validate:claude` runs the official Claude validator when Claude Code is installed.

## CI

The **CI** workflow runs on pushes, pull requests and manual dispatch:

| Job | Does |
|---|---|
| Fast gate / Sources and tooling | Source and guidance validation, script lint, exports and architecture |
| Fast gate / Workflows | actionlint 1.7.12 and ShellCheck, including rejection of invalid workflows |
| Fast gate / Rust format | `cargo fmt` |
| Fast gate / Minimum Rust (1.90) | Workspace and test-target compilation on the supported minimum |
| Fast gate / Dependencies | RustSec advisories, dependency licenses and sources with `cargo-deny` |
| Linux test runtime | One executable shared by the Linux Node compatibility jobs |
| Check (linux, Node 22/24/26) | Tooling/export compatibility and the full CLI test suite |
| Security / CodeQL | Rust, JavaScript and GitHub Actions security analysis |
| Native (linux/macos/windows, Node 24) | Clippy, Rust tests, release bundle, full CLI suite and relocated-install tests |
| Package | Bundles with all native runtimes, uploaded as `craft-capabilities-<sha>` |
| Publish | `distribution-next`, on `main` only ([releasing](RELEASING.md)) |

The fast gate must pass before Node compatibility, native platform checks and CodeQL start.
Those checks then run in parallel, with the Node jobs waiting for their shared Linux executable.
Package waits for all of them. Native jobs own the Node 24 platform coverage and hook reports;
separate macOS/Windows core jobs are unnecessary. Node versions remain Linux-only because
Node supports build tooling and the test harness; consumers run the bundled Rust executable.

Actions are pinned by SHA and permissions default to read-only. Rust builds are cached per OS
and `Cargo.lock`. Locally, run `npm run lint:workflows` and `npm run test:workflows` with
actionlint and ShellCheck installed. When updating actionlint, change its version and checksum
together in `workflows.yml`. The same workflow validation and security checks gate manual
releases as well as CI publishing.

Dependabot opens weekly grouped minor/patch updates for Cargo dependencies and GitHub
Actions; major updates remain separate PRs. Dependency alerts and security updates are
repository settings. Secret scanning and push protection are enabled on this public repository.
No paid Copilot review or external scanning service is configured.

The **Security** workflow also runs weekly and can be dispatched manually. Run its dependency
check locally with cargo-deny 0.20.2:

```sh
cargo deny --locked --config .github/deny.toml check
```

The policy covers all three shipped native targets. Known vulnerabilities, disallowed licenses
and untrusted sources fail the gate; duplicate dependency versions are warnings. When updating
cargo-deny, change its version and archive checksum together in `dependencies.yml`. CodeQL runs on
standard Linux runners using the free public-repository feature and publishes security alerts.
Blocking merges based on alert severity requires a repository code-scanning ruleset.

### Hook diagnostics

`npm run hooks:report -- --output <new-file>` samples the generated read-only hook launcher
against a small hand-labeled corpus and records exported source hashes. Exit 0 means the
report was collected, even if cases miss. Outcomes other than `allow` or `deny` are counted
as `unexpectedDecisions`. Timings include process start and are not production rates. CI
uploads one report per OS. See the [guard limits](SECURITY.md#read-only-review-guard).

### Performance

The **Performance** workflow runs after merge, on pushes to `main`, and by manual dispatch for
any branch, on Linux, macOS and Windows; it gates neither pull requests nor releases. [Hyperfine](https://github.com/sharkdp/hyperfine) measures the
installed release `craft`: startup, the prompt, tool and stop hooks, warm and cold Project
Intelligence refresh, and the background warm-up a session start launches. A second fixture with
declared standards, a started session, a warm index and an uncommitted edit times the hooks a
session at work fires: session start, after a read, after a search (the answer), after a test
run (the recorded run), subagent start and a stop with changed files (the stop review). Each of
these is also checked to deliver its feature, so a hook that stays silent fails the job. Per supported language, a generated fixture of 100 files (`languages.mjs`)
times a cold refresh and one `--declaration` lookup on the warm index, and is checked to declare
and find the caller of one function. Each hook runs in three layers: the native binary (`native`), the package
launcher (`launcher`) and the registered hook command under the shell hosts use (`host`). Every run records
wall time (median, range and 95th percentile), user and system CPU, peak memory and, where the
platform allows, instructions and cycles; installed binary and plugin sizes are tracked too. Hook outputs are checked outside the
timed runs, and only failed commands or checks fail the job.

[Bencher](https://bencher.dev) keeps the history, alerts on a `main` regression and compares a
dispatched branch with `main`, reporting through a check; timings never fail the job. Raw Hyperfine
JSON stays in the run artifacts for 90 days. Locally, with Hyperfine installed:

```sh
npm run build:claude && npm run build:native
node tooling/native-install.mjs dist/project-evidence dist/claude --current-target
node .github/benchmarks/suite.mjs --export dist/claude --fixture . --output <new-results-directory>
```

`--runs 3 --cold-runs 2` gives a quick integration run. Stock Hyperfine reports no peak memory
on Windows, so `peak-memory.mjs` measures, through PowerShell 7, the peak working set of each `native`
cell there, from the same prepared state. Windows charges CPU time in 15.6 ms scheduler ticks,
so its CPU measures are the mean over the runs rather than the median. Hardware counters cover
only directly started processes and are tracked for `native` rows only. One runner label is
served by several CPU models, so the Bencher testbed is the label plus the CPU model
(`windows-latest-amd-epyc-7763`): history and thresholds compare like hardware.

### Overhead budget

`node tooling/overhead.mjs` measures, for every export, what Craft adds to a host session without
running a model:

- **always**: the skill listing the host shows on every model call (formatted as each host
  lists skills, per `listing` in `hosts/hooks.json`) plus the instruction files it always loads;
- **hooks**: bytes the context-injecting hooks return across a fixed session of prompts and
  edits, from small talk to a routed fix, with machine paths counted as placeholders;
- **guidance**: bytes of the skill files each routed intent selects.

`tests/overhead-budget.test.mjs` fails when a number exceeds its ceiling in
`tests/fixtures/overhead-budget.json`, or when a hook adds context to a prompt Craft has nothing
for. Lower a ceiling when a change reduces overhead; raise one only with the reason in the pull
request. Listing formats marked `assumed` follow host documentation, not a recorded session.

### Installed hosts

The **Installed hosts** workflow runs before every release, on manual dispatch and on pull
requests that change it, on Linux, macOS and Windows. Each host export gets the release `craft`, and each host's registered
routing hook (prompt, shell tool, stop) runs from that installed layout under every shell that
host uses there: `sh`, `bash` and `zsh` where installed on Linux and macOS; on Windows, Git
Bash for Claude and Cursor (Cursor's Windows hook shell is undocumented) and Windows PowerShell
and `pwsh` for Codex `commandWindows`, Copilot `powershell` and Gemini. A command fails the job
when its exit code or output differs from the runtime called directly, or when it writes to
stderr. Hyperfine then times each command; it subtracts the shell's start, which is reported in
its own `shell/<name>/start` row. Results stay in the run artifacts for 90 days. Locally, after
installing the runtime into every export:

```sh
npm run build && npm run build:native
for host in claude codex cursor copilot gemini; do node tooling/native-install.mjs dist/project-evidence dist/$host --current-target; done
node .github/benchmarks/hosts.mjs --dist dist --output <new-results-directory>
```

### Engineering eval

The **Engineering eval** workflow runs real Codex sessions against the tasks in `evals/` with
[Harbor](https://docs.harborframework.com/) and grades them with hidden checks and Craft's own
recorded state. Pull requests that change `evals/` only qualify the tasks without a model: the
reference solution must pass and an idle agent must fail. Model-backed jobs run on manual
dispatch. `evals/README.md` explains running, reading and adding tasks.

### Codex hook discovery

```sh
CRAFT_CODEX_SMOKE=1 node --test tests/codex-hook-discovery.test.mjs
```

This optional test installs the generated packages into an isolated Codex home and checks the
host's hook inventory. It makes no model requests and does not trust or run hooks.
