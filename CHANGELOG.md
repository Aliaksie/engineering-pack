# Changelog

## Unreleased

Native runtime (published to `distribution-next`):

- One bundled `craft` executable per supported platform (macOS Apple Silicon, Linux x86-64,
  Windows x86-64) runs every hook, command and the status line. Installed packages no longer
  need Node; generated `bin/craft` and `bin\craft.cmd` launchers select the executable.
- Skills and documentation invoke `<package-root>/bin/craft <command>`.
- New commands: `acceptance-view` (QA, product and architecture views of a contract and its
  evidence), `review-followup` (next status of a reviewed GitHub pull request),
  `statusline <host>`, and `project-evidence` as a subcommand.
- Remote OTLP export is posted natively over HTTPS, with certificates verified by the
  operating system.
- Removed: the JavaScript host-control transports (worker, review and compaction phases
  remain as native JSON requests), the optional personal review inbox, and the separate
  `craft-project-evidence` executable in native bundles (use `craft project-evidence`).
- Documentation consolidated: installation, a command reference and one evaluation guide.

## 0.0.2

First public release, combining the initial 0.0.1 capability set with the release and
observability work delivered for 0.0.2.

Release and observability:

- One canonical pack version, synchronized capability/runtime metadata and versioned bundles.
- Manual GitHub release validation, tags, adapter archives, provenance and SHA-256 checksums.
  Version and changelog changes are reviewed through PRs; release publication does not update main.
- Private structured local telemetry with bounded retention, scoped correlation,
  meaningful operation/child traces and stable failure diagnostics.
- Explicit opt-in, bounded OTLP export with local export/loss diagnostics and no added model calls.
- Standalone lifecycle observations, Codex hook discovery repairs and nonblocking observational hooks.
- Bounded telemetry lock retries, portable contention tests and nested publication-command checks.
- First-use setup guidance and infrastructure/CI engineering guidance.
- Nested consumer telemetry stays outside Git history and does not dirty release checkouts.

Initial capabilities:

- Portable engineering, craft-delivery, craft-review and story-authoring capabilities.
- Selective workflow, practice, language, runtime, framework, layer and provider guidance.
- Deterministic routing, explicit CLI/MCP operation plans, optional checkpoints and
  revision-bound review artifacts.
- Bounded batched lookup, session-scoped service preparation, configurable economical-worker
  selection and compact opt-in edit routing hints.
- Unified session status separating selected guidance, observed context occupancy and pack activity.
- Safe-boundary continuation advice, bounded handoffs, source-bound discovery reuse and
  evidence-based retry budgets/model-profile recommendations without automatic execution.
- Portable JSON engine with explicit application, platform, host, external-integration, CLI
  and tooling boundaries; dependency checks and isolated-engine contract replay.
- Configurable service setup, execution-optimization techniques and independently usable
  commit, push, install, deploy, publish and license workflows.
- Generated Claude, Codex, Cursor, Copilot, Gemini, Gemini extension, JetBrains and
  AGENTS/Agent Skills bundles.
- Optional Node-based personal GitHub review inbox outside default exports.
- Source, export and deterministic behavioral validation, with a GitHub Actions
  check matrix and bundle artifacts. Broader model and native-host verification remains open.
