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

The **Performance** workflow runs on pull requests, pushes to `main` and manual dispatch on
Linux, macOS and Windows. [Hyperfine](https://github.com/sharkdp/hyperfine) measures the
installed release `craft`: startup, the prompt, tool and stop hooks, and warm and cold Project
Intelligence refresh. Each hook runs in three layers: the native binary (`native`), the package
launcher (`launcher`) and the launcher through the shell hosts use (`host`). Every run records
wall time, user and system CPU, peak memory and, where the platform allows, instructions and
cycles; installed binary and plugin sizes are tracked too. Hook outputs are checked outside the
timed runs, and only failed commands or checks fail the job.

[Bencher](https://bencher.dev) keeps the history and compares a pull request with `main`,
reporting through a check and a pull request comment; timings never fail the job. Raw Hyperfine
JSON stays in the run artifacts for 90 days. Locally, with Hyperfine installed:

```sh
npm run build:claude && npm run build:native
node tooling/native-install.mjs dist/project-evidence dist/claude --current-target
node .github/benchmarks/suite.mjs --export dist/claude --fixture . --output <new-results-directory>
```

`--runs 3 --cold-runs 2` gives a quick integration run. Stock Hyperfine reports no peak memory
on Windows, and its hardware counters cover only directly started processes, so counters are
tracked for `native` rows only.

### Codex hook discovery

```sh
CRAFT_CODEX_SMOKE=1 node --test tests/codex-hook-discovery.test.mjs
```

This optional test installs the generated packages into an isolated Codex home and checks the
host's hook inventory. It makes no model requests and does not trust or run hooks.
