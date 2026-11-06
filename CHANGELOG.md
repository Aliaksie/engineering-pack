# Changelog

## Unreleased

- Review guidance loads pull-request follow-up details only when needed through a linked reference.

- Testing guidance describes investigation of unexpected behavior as well as failing checks.
  Specification audits and story drafting include examples of conflicting rules, optional
  clarification and useful outcome slices with unresolved decisions.

Project standards:

- `.craft/project.json` declares the project's own standards in `standards`: a file, a folder
  or an established standard by name, scoped by module, language, layer, path or kind of
  work (`story`, `plan`, `verify`, …), at level `advise`, `check` or `review`. Routes and edit
  hints name the standards that apply, with a one-line summary instead of their content.
- Eight standards are checked without a model: `conventional-commits`, `keep-a-changelog`,
  `semantic-versioning`, `adr-according-to-nygard`, `madr`, `gherkin`, `ears-requirements` and
  `connextra-user-story` (aliases `semver`, `nygard-adr`, `ears`, `connextra`). After a file
  in a checked standard's scope is written, or a shell command makes a Git commit, unmet
  required elements are reported in the same tool result; the tool is never blocked.
  `routing.standardChecks: false` turns the checks off.
- Any id of the Apache-2.0 Semantic Anchors catalog, embedded from a pinned commit, can be
  named as an anchor at `advise` or `review`; routes show its title. The native bundle
  carries the catalog's licence and notice. Naming `clean-code` is rejected with the reason:
  models do not agree on what it requires.
- `anchors.catalogs` in `.craft/project.json` lists the project's own anchor catalog files.
  Their entries add names, or replace a Craft or catalog entry with the same id, and may reuse
  one of Craft's checks.
- `anchors.defaults: true` adds Craft's anchors for the kind of work to routes, advised and
  never checked: `invest` and `moscow` for stories, `adr-according-to-nygard` for plans,
  `testing-pyramid` and `bdd-given-when-then` for verification, `testing-pyramid` and
  `kiss-principle` for review, `conventional-commits` and `kiss-principle` for
  implementation. A declared standard for the kind of work replaces them; `anchors.disables`
  drops single anchors. Defaults are off unless a project turns them on.
- `hexagonal-architecture` (alias `ports-and-adapters`) and `clean-architecture` are checked
  against the declared modules. When a source file is written, each import the change adds
  that points into a declared module outside the file's module's `dependsOn` is reported.
  Imports already present at `HEAD`, modules without `dependsOn`, and imports Craft cannot
  place in the project are not reported. Rust, Go, Java, TypeScript/JavaScript and Python
  imports are placed. A project anchor with `"check": "module-dependencies"` applies the rule
  under its own name.
- `craft anchors` lists the anchors a project can name with the source each resolves to;
  `--search` searches the catalog by id, title, category or role.
- `craft standards` lists the declared standards, with each anchor's source, and runs the same checks on `--file` and
  `--commit-message`, exiting non-zero when a standard is not met.
- When a turn ends after changing files, hosts that continue a turn from a stop hook
  (Claude Code, Codex, Copilot CLI, Gemini CLI, Cursor) get one summary of what the changes
  introduced: module dependency findings, the declared `review` items of the changed scope
  and, with `routing.debtMarkers: true`, new TODO/FIXME comments, lint suppressions, skipped
  tests and removed assertions against `HEAD`. Each item is said once per session; a stop is
  never held twice in a row. `routing.stopReview: false` turns it off.
- Worker briefs from `plan-assign` and `plan-envelope --brief` carry the project standards of
  the task's files and owned paths in `standards`, as a route there shows them.
- `languages` accepts `rust` and `ruby`; `.rs` and `.rb` files route with those languages.

Project skills:

- `skills.declared` in `.craft/project.json` routes the project's own skills: a skill in
  `.claude/skills`, `.agents/skills` or another host skill folder `replaces` package skills or
  joins routes in its `scope`. Hints show it by its bare name. Where the current host does not
  read the skill's folder, or its `SKILL.md` is missing, the route keeps the package skill and
  says why. `skills.disables` removes package skills from routes. Two declarations replacing
  one package skill are rejected as a conflict.

## 0.0.4

Project Intelligence:

- Project Intelligence now reads non-code evidence. `craft locate --mentions <name-or-path>`
  finds the documents that link to a file or name it in a code span, also qualified
  (`craft_engine::Evidence` for `Evidence`). `--decisions <title-word>`
  lists decision records (Markdown under an `adr`/`decisions` directory) with their status as
  written, supersession links resolved; `*` lists all. `--key <dotted.key>` finds where a JSON,
  TOML or YAML key is set, with its value. Lockfiles and configuration files over 64 KiB are
  searched literally instead of indexed.
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
- `craft locate --term` prints matches as compact text; `--json` (or `--verbose`) prints the
  full JSON receipt as before.
- Project Intelligence warms up when a session starts: on macOS and Linux the session-start
  hook starts `craft project-evidence warm` in the background for a Git worktree with evidence
  enabled and returns at once. The first lookup then usually finds a warm index; on a fresh
  clone of this repository, a lookup a few seconds after session start takes about 0.13 s
  instead of about 1.1 s for each of the first two. A lookup waits up to 400 ms for the
  background refresh to release the store. Windows keeps indexing on the first lookup.
  `CRAFT_EVIDENCE_WARM=0` turns the background start off.

Answers in hooks:

- When the agent searches for one literal name itself (Claude `Grep`, or `rg`, `grep` or
  `git grep` in a shell), the after-tool hook adds the name's verified declaration and callers
  to that tool result on Claude, Codex, Cursor, Copilot and Gemini, so no extra lookup turn is needed.
  It answers only with complete coverage of a name declared once; otherwise the search result
  stays as it was. `efficiency.lookup.enabled: false` turns it off.
- A prompt that asks where a configuration key is set, which documents mention a name, or about
  decision records gets the verified answer in the prompt hook, whatever intent it routes to.
  A decision answer leaves out records under test or fixture directories and says how many.
- A prompt that asks where a name is used gets no declaration answer, even when it mentions the
  declaration ("which files use `AdrSlug`, apart from the file that declares it?"). Before, the
  hook answered with the declaring file, the one the question excluded.
- A lookup prompt that Project Intelligence cannot answer with verified rows gets no routing
  hint. Before, the hint named a `craft locate` command, and models that followed it spent one
  or two turns more than a plain search. Answered lookups still carry their rows and the command.

Routing:

- A change request ("fix…", "implement…") no longer gets a prompt hint naming skills unless a
  task is bound; the first edit's hint still names them. A one-line fix took 1.76 times the
  tokens of the same fix without Craft because the model read every named skill first.
- Routing hints leave out empty fields and show the task identity only once a task is bound
  (the session line already names the session), and a hint that reads the same as the last one
  is not sent again, for example on an edit of a second file of the same kind. Over a typical
  prompt-and-edit session, hook context drops by about a third on Claude, Codex and Gemini.
- Prompt routing stays silent on questions that need no project work: an HTTP or exit
  "status code" question is no longer read as a progress question, and an instruction that
  only says where the answer goes ("Write only the number to `ANSWER.md`") no longer turns a
  question into a code change.
- Prompt routing: progress questions about ongoing work ("Where are we with the invoice
  export?", "How is the migration going?") get a status hint instead of a code lookup;
  reports of a failing check ("The test is failing again", "CI is red") and "how is X
  validated?" questions get a diagnosis (investigate) hint instead of none.
- `routing.promptHints: false` turns prompt routing hints off; the request still sets the
  session's intent. `efficiency.lookup.enabled: false` now also turns prompt answers off, not
  only answers on searches.
- `craft settings` reports `features`: whether Project Intelligence, the background index
  warm-up and telemetry are on, and where each value came from.
- Telemetry records the `story`, `research` and `explore` intents of a routed request; they
  were dropped as unknown before.

Sessions and tasks:

- `craft task-decide` records a decision on the bound task: what, why, rejected alternatives
  and whether the user or the agent made it. `--supersedes` replaces an earlier one. Current
  decisions are shown when the task resumes.
- After compaction, and when a task is adopted in a new session, the model gets a short resume
  of the bound task (goal, done, state or open result, files, next step, whether source
  changed, decisions and latest steps, at most 4 KiB) instead of checkpoint data. Fingerprints
  stay available through `craft checkpoint-read`.
- Cursor, Copilot and Gemini now resume a bound task after compaction, at the next event that
  can show context: the next tool result on Cursor and Copilot, the next prompt on Gemini.
- With `checkpoint.enabled: false`, the session identity line is no longer shown at session
  start and after compaction, so a fully manual policy shows the model nothing.

Hosts:

- On Copilot, a tool call Craft does not deny now gets no hook output, so Copilot's own
  permission flow decides. Before, Craft answered `permissionDecision: "allow"` for every such
  call. Cursor still gets `permission: "allow"`: it blocks a call when that field is missing,
  and it does not let a hook's `allow` override its own approvals.
- Copilot now gets Craft context after a tool runs: answers on the agent's own searches,
  routing hints for edited files and the task resume after compaction.
- JetBrains reports note that Junie CLI has hooks that this export does not register yet.
- INSTALL notes host changes that affect Craft: Claude Code refuses an action when a plugin
  hook cannot answer; Codex 0.164 and later asks to trust Craft's hooks again after every
  update, and Codex 0.150 and later loads no project `AGENTS.md` in an untrusted project;
  Gemini CLI now serves enterprise users only, and Craft has no Antigravity CLI adapter.
- On macOS, a Git query no longer waits for an unrelated process Craft started at the same
  time, so repository evidence stops going missing when several queries run at once.

Guidance:

- Exported skills state the package's permission rule inline ("Craft guidance grants no
  permission: the project's instructions and the user's scope decide") instead of telling the
  agent to read the package `AGENTS.md` first. On Codex, which followed that link, a routed task
  reads up to two fewer files. `AGENTS.md` still ships with each package, and `craft
  context-report` no longer counts it as required guidance (Codex `implement`: 18,107 → 15,577 bytes).
- Codex skills are listed as `<package>:<skill>` (for example `craft-engineering:foundations`)
  instead of repeating the package name (`craft-engineering:craft-engineering-foundations`);
  the skill list Codex sends with every model call shrinks by about 1.1 KB. Routing hints on
  Codex name skills the same way. Hosts with one shared skill folder keep `<package>-<skill>`.
- Installed plugin docs link to shared documentation at the release tag (`vX.Y.Z`) instead of
  the candidate `distribution-next` branch, so stable installs no longer point at candidate files.

Runtime and telemetry:

- `craft` reports the pack version as `binaryVersion` (previously a fixed `0.0.1`).
- Host hooks record a successful `project.loaded` telemetry event once per session and again
  only when the effective settings change, instead of on every hook; failures are still
  recorded each time. `.craft/state/telemetry/settings.json` keeps the digest each recent
  session last recorded, under its telemetry pseudonym.

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
