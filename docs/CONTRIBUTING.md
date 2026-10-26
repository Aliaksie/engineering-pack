# Contributing

Development needs Node 24 for tooling and tests, Rust 1.89 or later with Cargo (CI pins 1.99.1), and a C
compiler for the bundled SQLite and Tree-sitter sources. Run `npm run check`. Tooling supports
Node 22 as a minimum; CI also checks Node 26 for forward compatibility. No npm install is
needed. `npm test` builds exports and the debug `craft` executable first.
Review skill changes for useful decision criteria, selective references and clear scope.
Keep examples portable and verify any executable behavior affected by the change.

Keep canonical content in `packages/` and native host behavior in `hosts/`. Generated output is
ignored and reproducible; never repair it by hand. Build refuses to overwrite unmanaged output.

Read [package format](PACKAGE-FORMAT.md), [adapters](ADAPTERS.md) and [security](SECURITY.md)
before adding a component. Use common principles, language discipline, application-layer
advice and provider references independently. A stack example does not impose that stack.

Keep the current version until a version change is requested; adapter manifests are generated.
Change the engineering craft bar in `packages/craft-engineering/standards/craft-bar.md`,
then run `npm run sync:craft-bar`. It applies to reference-index craft skills, not every
standalone method. Add focused behavioral cases for changes with material risk.

Script lint rejects shell executables and checks the syntax of tooling and test scripts. Use `npm run test:unit`
only after building; `npm test` and `npm run check` build automatically.
Use `npm run validate:claude` after building when Claude Code is available. Native validation
complements isolated build tests; neither proves actual model behavior. Use [eval cases](EVALUATION.md#package-scenarios)
for workflow and instruction changes, and record any live checks honestly.

Decisions belong in `engine/` (pure, no I/O or ambient clock), use cases and the `craft`
executable in `application/`, filesystem, Git, process and network access in `platform/`,
host tables in `hosts/`, guidance in `packages/`, and build, export, validation and release
code in `tooling/`. Runtime behavior is Rust; JavaScript is limited to tooling, tests and the
small host-table and hook-declaration files ([ADR 0001](adr/0001-rust-core-runtime.md),
[ADR 0002](adr/0002-module-map-and-boundaries.md)). `npm test` builds the debug `craft`
executable, or uses `CRAFT_NATIVE_BINARY` when set. Exercise real Git snapshots and
hostile/partial hook payloads with isolated fixtures. Service planning tests must never
contact a real tracker or codehost.

## GitHub Actions

Install [actionlint](https://github.com/rhysd/actionlint/blob/main/docs/install.md)
1.7.12 locally, then run `npm run lint:workflows` and `npm run test:workflows`.
Install ShellCheck to lint embedded shell scripts and run the integration smoke test. These are separate from
`npm run check`, so normal Node development needs no additional tools.
CI runs both commands in a dedicated job with checksum-verified actionlint and
ShellCheck. The smoke tests verify valid workflows pass and malformed YAML,
invalid expression contexts, missing job dependencies and unsafe shell expansion fail. Packaging and
publication depend on this job passing. When updating actionlint, update its
version and archive checksum together in `ci.yml`.

CI runs on push, pull request and manual dispatch. It validates manifests, catalog and source targets,
checks generated guidance, lints portable scripts, builds eight host exports and the GitHub distribution, validates exports
and runs tests on Linux/macOS/Windows with primary Node 24, plus Linux with the Node 22 minimum and Node 26 forward check.
No dependency installation, cloud credentials or model runner is required.

After every matrix check succeeds, the packaging job rebuilds and uploads `dist/` as
`craft-capabilities-<commit-sha>` for 14 days. Hidden generated manifests are included because
the host layouts require them. The upload contains generated bundles only, not local plans
or personal state. On eligible main-branch runs, a separate job publishes the generated GitHub installation branch; see [publishing](RELEASING.md). It does not publish npm packages or create versioned releases.

The workflow defaults to read-only repository permissions and SHA-pinned official actions. Action
runtime versions are separate from the Node versions tested by this project. Local checks and hosted CI results are reported separately.

Pinned action releases: [checkout v7.0.1](https://github.com/actions/checkout/releases/tag/v7.0.1),
[setup-node v7.1.0](https://github.com/actions/setup-node/releases/tag/v7.1.0),
and [upload-artifact v7.0.2](https://github.com/actions/upload-artifact/releases/tag/v7.0.2).

Each successful matrix job also uploads a hook diagnostic report; see
[hook diagnostics](#hook-diagnostics).

### Performance diagnostics

The separate **Performance** workflow runs on pushes to `main` and manual dispatch,
with no pull-request trigger. For [manual dispatch](https://docs.github.com/en/actions/how-tos/manage-workflow-runs/manually-run-a-workflow), select **Performance → Run workflow** and
choose a branch when a focused run is needed. The workflow becomes available after
its definition reaches the default branch. It measures the selected commit on native
Ubuntu, macOS and Windows with the same pinned Node 24.21.0 patch; update that pin
across the matrix together when refreshing the reference environment.

Each platform retains raw JSON, environment and source/harness hashes, an assessment
and a Markdown summary for 14 days. The job summary shows synthetic turn median/p95
and telemetry diagnostics. Process/assertion failures, changed wire or selected task
state, incomplete measurements and modified exports fail the benchmark job. Timing
has no hard threshold: hosted hardware/cache variability and reported telemetry
loss remain visible diagnostics. This workflow is independent of packaging and
publication and is not a release prerequisite.

For a local run against a clean checkout, use a new output directory outside that checkout:

```sh
node .github/benchmarks/run.mjs <clean-checkout> <new-results-directory>
```

The default is 30 repetitions plus a separate first sample. Set
`PERFORMANCE_ROUNDS=1` only for a quick integration check, not a performance baseline.
The suite measures process/Git operations, equal-byte file-count workloads, state
primitives and synthetic parallel hook turns. It excludes actual tools/models and
native host scheduling. Compare matching source/harness/runtime identities and retain
runner differences; do not interpret a green job as lossless telemetry or a latency
budget pass. The first telemetry reference can itself be lossy, inflating mismatch
counts; reported dropped events and affected turns are shown separately.

The grounded lifecycle workload composes context, starts a structured task, checks the
real developer fixture, records acceptance, and verifies rejection after a source edit.
Each round launches a fresh Node process and measures its first (`cold`) and repeated
(`warm`) lifecycle separately. Imports/setup are excluded from those sample spans; the
separate process wall includes launch, imports, setup and both phases. OS caches remain
uncontrolled. CPU and cumulative maximum RSS describe the benchmark child only, excluding
spawned Git/test processes; whole-process-tree CPU and peak memory remain unknown.

Run just this workload from the source checkout with a new output path:

```sh
node .github/benchmarks/lifecycle.mjs <new-output.json> 10
```

Retain at least ten samples per phase for each runner class before freezing an absolute
budget and relative regression threshold. Local calibration cannot stand in for hosted
Linux, macOS or Windows results. Until those baselines and their rationale are recorded,
timing remains diagnostic and only correctness/completeness can fail the job.

With a local Codex executable available, run the optional discovery regression after building:

```sh
CRAFT_CODEX_SMOKE=1 node --test tests/codex-hook-discovery.test.mjs
```

It installs all four generated packages into a temporary, isolated Codex home and checks
the real host's hook inventory. It makes no model requests and neither trusts nor executes
hooks. Codex 0.163.0 discovered 29 hooks with the native manifest export; the portable
Agent Plugins 1.0 manifest export returned zero. Default checks skip this optional host
test; registration/runtime tests run for every hook-capable adapter. Actual automatic
dispatch in other hosts still needs a trusted live session and fresh telemetry.

### Hook diagnostics

`npm run hooks:report -- --output <new-file>` checks exported source hashes, samples the
generated read-only hook launcher and records a bounded hand-labeled policy corpus. It does
not execute the corpus commands or install hooks. Exit 0 means collection succeeded, even if
policy cases expose misses; outcomes other than `allow` and `deny` are counted as
`unexpectedDecisions`, separately from false and missed blocks. Process wall time includes
startup and serialization, excluding host dispatch, model calls and network. Neither sample
latency nor corpus counts establish production rates. See the
[guard limits](SECURITY.md#read-only-review-guard).
