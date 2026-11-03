# Changelog

## Unreleased

- Project Intelligence reads TypeScript, Python and Go: `craft locate --declaration`,
  `--callers` (with the enclosing function or method) and `--imports` answer from the index
  for `.ts`, `.tsx`, `.py` and `.go` sources, as for Java and Rust. JavaScript now answers
  `--callers` too. C, C++, C#, PHP and Ruby get declarations and callers by name.
- `craft locate --imports` finds a named import (`import { X } from`, `from m import X`) and
  matches a module by its trailing path or module segments, so `utils/buffer`, `Gson` or
  `_urlparse` find `../utils/buffer.js`, `com.google.gson.Gson` and `._urlparse`.
- A source with a syntax error is indexed outside the error instead of skipped, and reported
  as `partial` (`partialSources` in status); it is still searched literally beside the index.
  A source that cannot be read still fails.
- JavaScript declaration kinds name what was declared: `js:declares-function`, `-class`,
  `-method` or `-variable` instead of `js:declares`. Stored indexes are rebuilt once on first
  use.
- `craft locate --term` finds every file `git grep` finds within its bounds: in a Git worktree
  Git preselects the files that contain a term, so large assets, lockfiles and unrelated text
  no longer use up the byte bound, and a binary file never counts against it. Matching files
  whose lines exceed the match bound are listed in `moreFiles`. Structural lookups searching
  unindexed files benefit the same way.
- Routing hints leave out empty fields and show the task identity only once a task is bound
  (the session line already names the session), and a hint that reads the same as the last one
  is not sent again, for example on an edit of a second file of the same kind. Over a typical
  prompt-and-edit session, hook context drops by about a third on Claude, Codex and Gemini.
- Codex skills are listed as `<package>:<skill>` (for example `craft-engineering:foundations`)
  instead of repeating the package name (`craft-engineering:craft-engineering-foundations`);
  the skill list Codex sends with every model call shrinks by about 1.1 KB. Routing hints on
  Codex name skills the same way. Hosts with one shared skill folder keep `<package>-<skill>`.
- `craft locate --term` prints matches as compact text; `--json` (or `--verbose`) prints the
  full JSON receipt as before.
- On macOS, a Git query no longer waits for an unrelated process Craft started at the same
  time, so repository evidence stops going missing when several queries run at once.
- Installed plugin docs link to shared documentation at the release tag (`vX.Y.Z`) instead of
  the candidate `distribution-next` branch, so stable installs no longer point at candidate files.
- `craft` reports the pack version as `binaryVersion` (previously a fixed `0.0.1`).
- Host hooks record a successful `project.loaded` telemetry event once per session and again
  only when the effective settings change, instead of on every hook; failures are still
  recorded each time. `.craft/state/telemetry/settings.json` keeps the digest each recent
  session last recorded, under its telemetry pseudonym.
- Project Intelligence warms up when a session starts: on macOS and Linux the session-start
  hook starts `craft project-evidence warm` in the background for a Git worktree with evidence
  enabled and returns at once. The first lookup then usually finds a warm index; on a fresh
  clone of this repository, a lookup a few seconds after session start takes about 0.13 s
  instead of about 1.1 s for each of the first two. A lookup waits up to 400 ms for the
  background refresh to release the store. Windows keeps indexing on the first lookup.
  `CRAFT_EVIDENCE_WARM=0` turns the background start off.
- Prompt routing stays silent on questions that need no project work: an HTTP or exit
  "status code" question is no longer read as a progress question, and an instruction that
  only says where the answer goes ("Write only the number to `ANSWER.md`") no longer turns a
  question into a code change.
- Prompt routing: progress questions about ongoing work ("Where are we with the invoice
  export?", "How is the migration going?") get a status hint instead of a code lookup;
  reports of a failing check ("The test is failing again", "CI is red") and "how is X
  validated?" questions get a diagnosis (investigate) hint instead of none.

## 0.0.3

Native runtime:

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
- Package manifests name the native hook handler directly (`"handler": "observe"`,
  `"deliver"` or `"review-read-only"`); the one-line `hooks/*.mjs` handler files are gone
  and no longer ship in exports. A handler file path now fails validation.
- Private state layout 4: per-session records move to `sessions/<session-id>/` with fixed
  names (`checkpoint.json`, `task.json`, `activity-*`, `control-*`) and a `manifest.json`;
  shared records are grouped into `telemetry/`, `intelligence/` and `workspaces/`. State of
  earlier versions is neither read nor cleaned: stop older plugins and delete
  `.craft/state/<worktree-id>/`, `.git/craft-state/` and `.capabilities/state/` by hand.
  `state-clean` no longer reports `layout` or `previousShared`. `craft settings` reports
  state `formatVersion` 4.
- Registered hook commands start the bundled executable from the host's own shell; the
  `bin/craft` launcher remains the fallback (for example dash as `/bin/sh`, or a missing
  executable). A routed prompt hook takes about 9–11 ms instead of 25 ms on macOS.
- Fewer and cheaper Git process starts per hook (on macOS the `/usr/bin/git` forwarder is
  skipped when the developer directory is unambiguous) and a size-optimized release build.
- When a package's bundled runtime cannot run, it says so once at session start instead of
  adding "Routing/checkpoint unavailable" to every prompt and tool call.
- Telemetry lock waits are longer, so parallel hooks of the installed packages no longer drop
  events on slow machines.

Routing:

- Prompts are routed by the request they lead with, through a deterministic classifier that
  skips courtesy openers, reads clause by clause and decides create/assess verbs by what they
  are about. Developer, QA, reviewer, product and architect requests all get hints; statements
  and questions about the conversation get none. See `docs/ROUTING.md`.
- Implementation requests select the `craft-delivery:workflow-deliver-change` workflow, which
  carries a supplied story through `craft-delivery:implement-story`. A story request leads with
  `craft-stories:workflow-prepare-stories`; assessing a spec leads with
  `craft-stories:audit-specs` and writing an ADR with `craft-delivery:write-adr`.
  `craft route --artifact <name>` selects an artifact's workflow.
- A prompt's request, not the files written while doing it, decides the intent: output file
  names, others' wants and pasted text decide nothing. The session's intent is recorded in
  `intent.json`; an edit routes `implement` only when no request established another intent,
  once per file, and on one tool hook.
- A change request that also asks for a test routes `implement`, not a testing-only `verify`.
- Hosts that cannot show context for an event (Cursor and Copilot prompts, several before-tool
  hooks) build no route and record no failure.

Project Intelligence:

- Structural lookups: `craft locate --declaration | --callers | --imports | --section |
  --outline` answer from a local, verified index, refreshed within a time budget, with the
  enclosing declaration per row and a literal search over sources the index does not cover.
  Rust, Java, JavaScript (declarations and imports) and Markdown sections are indexed; plain (non-Git) workspace folders and nested
  worktrees are walked; `evidence.exclude` in `.craft/project.json` scopes the inventory.
- On by default; opt out per project with `"evidence": {"enabled": false}` in
  `.craft/project.json`. The `project-evidence` commands still require `--enable`.
- Qualified names (`Type.method`, `Type::method`, `module::fn`, `Type#method`) match their last
  segment instead of reporting a false "absent"; Java method references and Rust functions
  passed as values count as callers. Existing indexes re-extract on the next lookup.
- A prompt that asks who calls one name or where it is declared gets up to ten cited rows in
  its routing hint when the answer is certain, and abstains otherwise. The hint carries a
  ready-to-run `lookup` command, and no longer sends the model through skill and setup files
  first. Prompt-time lookups share one second of index refresh per prompt.
- A bounded literal search that stopped at its limits or skipped files reports
  `complete: false` and is no longer recorded as a failure (`SEARCH_INCOMPLETE` is no longer
  emitted).
- Refreshes report `phasesMs` (inventory, metadata, read, extract, store). The index store is
  schema 4.

Sessions and context:

- `host-setup --host <host> --package <package>` previews display controls, context policy and
  integration gaps; on macOS and Linux `--apply` connects Claude's status line.
  `host-observe` accepts forwarded Claude and Codex usage and compaction observations.
  `permissions --host <host>` previews opt-in inspection-command rules without writing grants.
- A file written while the session plans, such as a requested `PLAN.md`, is recorded as
  planning evidence (path, content hash, revision), also when a shell command wrote it.
  Checkpoint recovery and `craft session` list it; a new session is offered unchanged earlier
  evidence. The text is never parsed and creates no work plan.
- Session states (stale checkpoint, unavailable host observation or model advice, exhausted
  attempt budget) are recorded as states, not failures. Status line refreshes are recorded
  only when the reported state changes, or every ten minutes.
- One `host.hook` record per hook event instead of one per installed package.

Guidance:

- Each package's `AGENTS.md` carries a self-contained first-use notice; `SETUP.md` is read only
  for a setup, hook or usage question.
- The deliver-change workflow is lean for scoped changes; task binding, workers, workspace
  scope, acceptance receipts and action offers move to `craft-delivery/standards/task-runtime.md`
  for multi-step work. Workflow links are rebased in every export.
- `apply-change` and `deliver-change` size verification: focused checks while working, the
  declared full checks once at hand-off, and checks a pre-commit hook or CI owns are reported
  as left to it, not as passed.

Release, evaluation and verification (repository only, not shipped):

- A Harbor engineering eval with fix, callers-lookup, literal-search and planning tasks, model-free
  qualification on pull requests and a manual vanilla vs Craft comparison.
- `npm run eval:audit -- <transcript.jsonl>` prints an engagement card for one real Claude Code
  or Codex session; decision replay checks realistic prompts and session situations.
- Host conformance tests check every hook against its host table; the Installed hosts workflow
  runs each host's registered hook command under that host's shells on Linux, macOS and
  Windows before a release.
- Hyperfine performance suite on all three platforms with history in Bencher.
- A versioned release now promotes its attested `distribution` archive to the stable branch
  as a fast-forward commit; **Promote** can also be run by hand to retry or roll back. 0.0.3
  is the first native stable release.
  Native executables are not code-signed or notarized.

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
