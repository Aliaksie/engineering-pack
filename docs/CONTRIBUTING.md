# Contributing

You need Node 24 (22 minimum), Rust 1.89+ with Cargo (CI pins 1.99.1) and a C compiler for the
bundled SQLite and Tree-sitter sources. There is no npm install.

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
| Workflows | actionlint 1.7.12 and ShellCheck, including rejection of invalid workflows |
| Check (linux/macos/windows, Node 24; linux Node 22 and 26) | Validation, exports, architecture and the test suite |
| Native / Rust format | `cargo fmt` |
| Native (linux/macos/windows) | Clippy, Rust tests, release bundle and relocated-install tests |
| Package | Bundles with all native runtimes, uploaded as `craft-capabilities-<sha>` |
| Publish | `distribution-next`, on `main` only ([releasing](RELEASING.md)) |

Actions are pinned by SHA and permissions default to read-only. Rust builds are cached per OS
and `Cargo.lock`. Locally, run `npm run lint:workflows` and `npm run test:workflows` with
actionlint and ShellCheck installed. When updating actionlint, change its version and checksum
together in `ci.yml`.

### Hook diagnostics

`npm run hooks:report -- --output <new-file>` samples the generated read-only hook launcher
against a small hand-labeled corpus and records exported source hashes. Exit 0 means the
report was collected, even if cases miss. Outcomes other than `allow` or `deny` are counted
as `unexpectedDecisions`. Timings include process start and are not production rates. CI
uploads one report per OS. See the [guard limits](SECURITY.md#read-only-review-guard).

### Performance

The **Performance** workflow runs on pushes to `main` and manual dispatch. It measures a
release `craft` build on Linux, macOS and Windows, covering process and Git operations,
filesystem workloads, synthetic hook turns and a grounded task lifecycle, and keeps raw
results for 14 days. Timing is diagnostic, with no thresholds; only incorrect or incomplete
measurements fail the job. Locally:

```sh
cargo build --release --locked -p craft-application --bin craft
CRAFT_NATIVE_BINARY=$PWD/target/release/craft node .github/benchmarks/run.mjs <clean-checkout> <new-results-directory>
```

`PERFORMANCE_ROUNDS=1` gives a quick integration run instead of the default 30 rounds.

### Codex hook discovery

```sh
CRAFT_CODEX_SMOKE=1 node --test tests/codex-hook-discovery.test.mjs
```

This optional test installs the generated packages into an isolated Codex home and checks the
host's hook inventory. It makes no model requests and does not trust or run hooks.
