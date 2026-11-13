# Changelog

## Unreleased

- A hook no longer fails with "hook input exceeds 64 KiB" when the host sends a large payload,
  such as the text of a large file just read. Hooks read up to 1 MiB; above that, a hook with
  nothing to enforce stays silent, and before a shell command the safety policy denies what it
  could not read, where until now such a command was not checked at all.
- A test command whose output is piped into a filter such as `tail` or `grep` is no longer
  reported as having executed no test when the summary left in view is an empty one: such a
  run is `unknown` (`cut-output`) unless the output shows a failure. `cargo test | tail -4`
  had been reported as checking nothing although its tests ran.
- The safety policy is on with no configuration: a destructive shell command is refused in a
  project that declares nothing. `"safety": {"enabled": false}` in the project, organization
  or user file turns it off, and `disables` leaves single rules out. A project that relied on
  the policy being off until enabled now sees those commands refused.
- The safety policy and the read-only review guard no longer refuse a command for what the
  text of a here-document says: the text is data unless a shell is named on the line that
  opens it. Substitutions in a document with an unquoted delimiter are still read.
- A project that declares no standards is held to the ones its own files state: what
  `craft standards --propose` lists is in effect without being copied, as a `detected` layer
  beneath the organization and user files. Commit, changelog and decision-record formats are
  checked, contributor documents advised, and lint and format commands run by `craft check`
  when it is called; no hook starts them. Standards the project declares replace them, and
  `routing.conventionProposals: false` turns this off. The session-start line now says what
  is in effect instead of what could be accepted. In `craft config --json`, `order` and
  `layers` carry the `detected` layer.
- The conventions scan recognizes more: TypeScript (`tsconfig.json`), mypy, Pyright, and
  Checkstyle, Spotless, ktlint and detekt through the project's Gradle or Maven build. A Go
  module and a .NET project are advised of `go vet` and `dotnet format`, which `craft check`
  runs only once the project declares them. It also proposes a format the project keeps without naming it: Conventional
  Commits when four in five of the last commit subjects follow it, Keep a Changelog and MADR
  or Nygard decision records when the existing files already pass the check.
- A folder holding several repositories works as one workspace: opened there, a session is
  held to each repository's own standards within that repository's folder, with one `.craft`
  at the workspace root. The stop review reads every member repository and names its files
  from the workspace root, and the commit check reads the repository a commit was just made in.
- A changelog, version or Gherkin standard with a folder in `scope.paths` checks its own
  files inside that folder (`CHANGELOG.md`, manifests, `*.feature`) instead of every file
  there; a path that names one file still selects it. In a workspace this kept a member
  repository's changelog format from being reported on its other documents.
- A session stays with the project it was opened in. On Claude Code a hook reads the project
  from the directory the host was opened in, so a shell that moved into another repository no
  longer starts a second session record there. A command run without `--root` from a
  subfolder works on the project around it instead of creating `.craft` in the subfolder.
- Fixed: the stop review of a project whose root is a folder below the top of its repository
  read changed files from the wrong place and reviewed nothing. It now reviews that project's
  files and leaves its neighbours' alone.

## 0.0.6

This release lets an organization or a user declare a baseline once that every project
inherits, runs the checks a project declares at the stage it chose, refuses destructive shell
commands where a project turns that on, proposes standards from the convention files a project
already has, and shows in one line what Craft did in a session. Each of these is covered by
fixtures, and one scenario runs them together through Claude Code's and Codex's own hook
payloads. That agents keep declared standards more often, or leave less debt, with Craft than
without it is the goal and has not been measured.

One baseline for every project:

- An organization file and a user file declare a baseline every project inherits: switches,
  standards with their checks, the safety policy, execution profiles. The project overrides
  only what differs; `standards` and `safety.deny` merge by `id`, and an entry with
  `"disabled": true` removes an inherited one. `craft config` lists the layers and each value
  one set over another, `--explain` every effective value with its layer. The organization
  file is read from `CRAFT_ORGANIZATION_CONFIG` or, on Linux and macOS, a fixed location; the user
  file is `~/.config/craft/preferences.json`, which now also takes standards, safety and
  execution. A layer or project file that cannot be used is an error naming the file; while
  it lasts, the review package's shell guard refuses shell commands in the projects it covers.
- A switch or section the running build does not know in the organization or user file is
  left out and named by `craft config` and `craft doctor` instead of stopping every command
  and hook, so one file serves installed builds of different versions. The project file
  stays strict, and so does a known field with a value it cannot take.

The project's own checks:

- `craft check` runs the checks a project declares (`modules[].checks` and the `run` command
  of its standards) for the changed files, without a shell and each within a time bound. A
  check can name the `stage` it belongs to (`step`, `commit`, `pr`, `ci`); a run executes the
  checks up to the stage asked for and lists later ones as left to their stage, never as
  passed. Results carry the source digest of the tree they ran on; a test command that
  executed no test does not pass. A test command it runs in a session is recorded against the
  tree it started on, and a check history that cannot be written is reported in `recordErrors`
  without discarding the results. `routing.projectChecks: false` turns the command off.

Safety policy:

- An opt-in safety policy refuses destructive shell commands before they run on Claude Code,
  Codex, Cursor, Copilot CLI and Gemini CLI: recursive deletes outside the project or of its
  root, forced pushes, Git commands that discard uncommitted work, and privileged or
  system-level commands. `safety.enabled` turns it on, `safety.disables` leaves rules out and
  `safety.deny` adds the project's own by leading words. It only refuses and never approves a
  command; `craft settings` lists the rules in force and the session summary counts refusals.

Standards from existing conventions:

- `craft standards --propose` lists standards a project's existing files already support, ready
  to copy into `.craft/project.json`: conventions named in agent instructions or
  `CONTRIBUTING.md` that Craft checks, lint, format and type-check commands from package
  scripts and tool configuration, the contributor document and `.editorconfig`. Each proposal
  shows the files that support it; nothing is active until it is copied, and the command
  writes nothing.
- A session that starts in a project with no declared standards says once how many its own
  files support and that `craft standards --propose` lists them; the same set is not named
  again. Standards inherited from an organization or user file do not count as the project's
  declarations, and what they already cover is not proposed. `routing.conventionProposals:
  false` turns this off. The record of what was named is kept in `.craft/state/notices/`, a
  new shared state folder that `craft state-clean` measures.

Session summary and context budget:

- When a turn ends, Claude Code, Codex and Gemini CLI show one line on what Craft did in the
  session: standards findings raised and fixed, test runs that ran no test, lookups answered
  from the project index and contexts restored after compaction. Nothing happened, or nothing changed
  since the line was last shown: no line. `craft session` prints the same line and counts on
  every host (`delivered` in `--json`); `routing.sessionSummary: false` turns the line off.
- A hook answer carries at most 9,000 bytes of context on every host, down from 12,000
  characters, which was over the cap past which Claude Code, Codex and Copilot replace or cut
  a hook's context. A context over the ceiling now loses whole pieces, least important first,
  and names them in one line, instead of being cut mid-text; the session identity and a
  resumed task are kept longest.
- Hook answers from Project Intelligence are bounded per session: an answer with the same
  text is given once per context window, and a window receives at most 16 KiB of answers. A
  compaction starts a new window. Needs `routing.sessionRecords`.
- Sessions keep a `delivered.json` record of counts (never delivered text); `craft
  state-clean` treats it as a Craft record. `host.hook` telemetry events add `contextBytes`,
  `droppedPieces` and `heldAnswers`.
- `npm run overhead` replays a long session with repeated questions and searches; the
  overhead budget test fails when its total or any single hook answer passes its ceiling.

Hooks and hosts:

- The stop review covers work that passed no edit hook and work already committed: a file a
  shell command or a script wrote gets its anchor check at the stop, and files in commits the
  session made since its previous stop are reviewed like uncommitted ones. Before, a turn
  that wrote through the shell and committed ended with no review at all. Commits that
  arrive by a pull, merge, checkout, rebase or reset are not taken for the session's work.
- Prompt and edit hints name only skills of Craft packages the host installed beside the
  package whose hook runs: a plugin cache's sibling packages (Claude Code, Codex) minus those
  the host's settings turn off, or the package folders of a project overlay. A route whose
  every skill belongs to a package that is not installed says nothing unless it carries a
  standard, a bound task or a missing input. Such routes report `skillAvailability:
  "installed-packages"` and list what was left out in `skillSources` with status
  `not-installed`; `craft route` on the command line is unchanged (`not-verified`).
- `craft doctor` compares each Codex hook approval with the installed plugin content and says
  when an update left the approvals behind: Codex then skips every Craft hook without a
  message until they are approved again in `/hooks`. A hook event of another version no
  longer counts as the installed one running, and doctor reports the configuration layers:
  a file that cannot be used, and a safety policy that is on while no hook of an installed
  host is known to have run in the project.
- `craft doctor` reports Codex plugins that `config.toml` enables but the plugin cache does
  not hold as a problem with the `codex plugin add` command to run, instead of "not
  installed". For a host with no install found it still names the last hook event the project
  recorded for that host.
- Cursor: a Craft hook with nothing to say now answers `preToolUse` with `{"permission":"allow"}`
  and `beforeSubmitPrompt` with `{"continue":true}` instead of empty output. Cursor blocks a
  permission hook whose answer does not match its schema; `allow` stays advisory there. The
  launcher's answer without a bundled runtime does the same.

Project Intelligence and speed:

- Hooks and lookups that ask Git start fewer processes and no longer wait for one after
  another: a source inventory starts two Git listings together instead of three queries in
  a row, and a worktree snapshot runs its three queries at once. Where starting a process is
  slow, on Windows most of all, the turn-end, session-start, search and lookup hooks answer
  sooner.
- A refresh whose file or time budget is spent no longer reads and hashes every source it
  has not indexed yet; it defers them unread. A first lookup in a large project stays within
  its budget instead of reading the whole project. `bytesScanned` counts only sources read.
- Index warm-up keeps one store connection for all its chunks, so each chunk releases the
  store sooner for a waiting lookup.

Platforms:

- Bundles carry a native executable for Linux on arm64 (glibc), next to Linux x86-64, macOS
  on Apple Silicon and Windows x86-64. Hooks, routing and commands now run on arm64 Linux
  machines, containers and cloud instances, where every package was inactive before.

Upgrading:

- Codex binds a hook approval to the plugin's content, so after this update, as after every
  one, it skips every Craft hook without a message until they are approved again in `/hooks`.
  `craft doctor` now says when that is the case.
- `~/.config/craft/preferences.json` is read by every installed Craft build. Builds before
  0.0.6 stop on a field they do not know: update every installed package before adding
  standards, safety, execution or new switches to that file.
- A hook answer now carries at most 9,000 bytes of context; a longer one names the pieces it
  left out.

Known limits:

- The safety policy judges a command by its words: `xargs rm -rf`, `find -delete`, deleting a
  remote branch by push, interpreters (`python -c`, `node -e`) and scripts on disk are not
  recognized, and symbolic links are not resolved. It is evaluated by `craft-delivery`'s
  hook, so a host without that package has no policy.
- `craft check` has no scope against a base branch: in CI, name the files with `--file` or
  run everything with `--all`.
- A CI runner or teammate without the organization or user file sees the project's own
  configuration alone.
- The stop review places changed files relative to the repository's top level; a project
  whose root is a folder below it is not reviewed correctly.
- A file written by a shell command is checked when the turn ends, not when it is written,
  and gets no routing hint.
- A test run is read from the runner's summary in the tool result. Output cut before the
  summary, for example by `| tail` or `| grep`, is recorded as `unknown`, or as a run of no
  test when the last summary left is an empty test binary's.
- Restoring standards and test status after compaction is exercised live on Claude Code; on
  Codex it is covered by the hook-payload scenario only. Cursor, Copilot CLI and Gemini CLI
  are built from their published hook references and tested by payload fixtures.
- The Linux arm64 executable is built and tested in CI; no host was run on it.

## 0.0.5

This release lets a project declare its own standards and skills, checks what can be checked
without a model while the agent works, and records the test runs the agent actually made.
Each check, routing decision and hook response is covered by fixtures, and one scenario runs
every standards and evidence feature through Claude Code's and Codex's own hook payloads.
That agents keep declared standards more often, or leave less debt, with Craft than without
it is the goal and has not been measured.

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
- Subagents that Claude Code, Codex or Copilot CLI start themselves receive the project
  standards of the delegating session's work and bound task as context. Nothing is added
  when the session has no established work or the project declares no standards.
- `craft doctor` reports, from files only, whether Claude Code and Codex would run each
  installed Craft package for the project (enabled, bundled runtime digest, Codex trust and
  hook approvals at the newest cached version, duplicate registrations, last hook event and
  its version) and checks the
  runtime behind Cursor, Copilot CLI and Gemini CLI overlays. It exits 1 only when an
  enabled package cannot run.
- Test commands the agent runs are observed from the host's tool result: the runner's summary
  decides (`cargo`, `node --test`, pytest, unittest, vitest, jest, Maven), a run in which no
  test executed is failed, and output Craft does not recognize is `unknown`, never passed. A
  run that checked nothing does not override an earlier pass or failure on the same tree. A
  clean exit that ran no test is reported in the tool result; a resumed session names whether
  its last run still matches the tree. Claude Code failure events (`PostToolUseFailure`), Cursor
  and Copilot CLI `postToolUseFailure` are registered for this; interrupted and background
  calls record nothing.
- Acceptance ledgers accept `formatVersion: 2`: a record may carry its run's test counts, or
  `"tests": "observed"` to take the run Craft observed on the current tree; only Craft fills
  `source: observed`. A run that executed no test fails its criteria, and undecided current
  evidence reads `unknown`, apart from `gap`; resume lists `unknown` criteria. Format 1
  ledgers are unchanged.
- A passed `task-step` at the verify stage, named or carried over from the task, is refused
  while the test run that decides the current tree failed.
- After compaction or on resume, the restored context also names the `check` and `review`
  standards of the session's scope (its established work and, for a bound task, the task's
  files), so hard rules return with the work, with or without a Craft task. `advise`
  standards come back with routing as before.
- `languages` accepts `rust` and `ruby`; `.rs` and `.rb` files route with those languages.

Project skills:

- `skills.declared` in `.craft/project.json` routes the project's own skills: a skill in
  `.claude/skills`, `.agents/skills` or another host skill folder `replaces` package skills or
  joins routes in its `scope`. Hints show it by its bare name. Where the current host does not
  read the skill's folder, or its `SKILL.md` is missing, the route keeps the package skill and
  says why. `skills.disables` removes package skills from routes. Two declarations replacing
  one package skill are rejected as a conflict.
- A host that does not read the declared skill folder uses a copy with the same folder name
  and skill name in a folder it reads, so one declaration serves Claude Code
  (`.claude/skills/<name>`) and Codex (`.agents/skills/<name>`). `skillSources` names the copy
  the host reads, and the "mirror the skill to" advice names the exact folder.

Hooks:

- Codex edits are seen: an `apply_patch` call names its files only inside the patch, so Codex
  edits got no edit hint, no standard or module check, and did not count toward the session's
  scope. Every file a patch adds, updates, deletes or moves to (up to 32 per patch, as many as
  the session keeps) is now routed and checked, even when the first is outside the project;
  with several files, each finding names its file.
- A `grep` or `rg` that reads another command's output through a pipe without a file operand
  (`copilot --help | grep model`) no longer gets a code-search answer.
- A prompt that mentions a test run, its results or logs ("what with test run?") no longer
  routes to testing-only verification work.
- Every hook output now has a switch in `.craft/project.json`: `routing.testRuns: false` stops
  recording test commands (and the zero-tests notice and resume test status; a declared
  verification pass is then not checked against a run),
  `routing.workerStandards: false` starts host subagents without the delegating standards,
  and `routing.sessionRecords: false` keeps no intent, planning or resume records under
  `.craft/state`. The fully manual policy now shows nothing and keeps no session records; a
  compaction only updates a host context the status line or `craft host-observe` already keeps.
  `craft settings` lists every routing switch with its origin, including `standardChecks`,
  `stopReview` and `debtMarkers`.

Project Intelligence:

- Project Intelligence rows say what they rest on: `basis` `parsed` (a declaration, import,
  section, link or key read from the parse) or `name` (a call, reference or code-span mention
  that only spells the name and may mean another declaration of it). `craft locate` marks name
  rows `[by name]`; a hook answer states the basis of its rows and, for callers, that they are
  not resolved targets.
- The Project Intelligence store writes through a write-ahead log: `project.sqlite-wal` and
  `project.sqlite-shm` sit next to `project.sqlite`, and refreshes no longer sync the disk on
  every commit. A power failure can lose the last commits, which the next refresh extracts
  again. A filesystem that refuses the log keeps the previous journal and full syncs.
- A Project Intelligence refresh that finds no source changed, added or removed writes
  nothing: `project.sqlite` is left untouched and the store `generation` stays the same. A
  refresh stopped before its first change leaves the last complete inventory standing.
- A refresh reuses a source without reading it when the file's metadata matches what it was
  last read with and the file has not changed in the last two seconds; `reusedUnread` counts
  them. The index store is schema 5: an earlier store is discarded and rebuilt by the next
  refresh.

Configuration:

- `.craft/project.json` may carry a `$schema` link for editor completion; `craft` ignores it.
  Every field of the project schema has a description and its default, and
  `docs/PROJECT-CONFIG.md` lists them all.

Guidance:

- Task continuation guidance requires an ownership handoff or explicit reassignment before
  taking over another session's work; stale receipts do not imply its tools have stopped.
- Debugging guidance keeps diagnosis separate from authorization to repair. Existing guidance
  adds risk-based test-case selection, frontend performance investigation and transition limits,
  reliability-measure boundaries, and evidence-based story priorities and success measures.
- Review guidance loads pull-request follow-up details only when needed through a linked reference.
- Testing guidance describes investigation of unexpected behavior as well as failing checks.
  Specification audits and story drafting include examples of conflicting rules, optional
  clarification and useful outcome slices with unresolved decisions.

Upgrading:

- Codex 0.164 and later asks to approve Craft's hooks again after the update (`/hooks`), as
  for every release.
- The Project Intelligence index is rebuilt once on first use (store schema 5).

Known limits:

- A test run is read from the runner's summary in the tool result; output cut before the
  summary, for example by `| tail` or `| grep`, is recorded as `unknown`.
- Restoring standards and test status after compaction is exercised live on Claude Code; on
  Codex it is covered by the hook-payload scenario only.
- Copilot CLI skips repository hooks in an untrusted folder, so the overlay's checks run only
  in a trusted project.

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
