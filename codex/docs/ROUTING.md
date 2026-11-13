# Routing

The router is a deterministic lookup, not another model conversation. From an explicit
intent, changed paths, optional risk tags and project configuration it returns which
guidance to load, who owns the code, which checks are declared and how to execute. It runs
nothing.

```sh
craft route --root examples/project --intent review --file services/orders/Order.java
```

Intents: `implement`, `plan`, `review`, `story`, `investigate`, `locate`, `research`,
`status`, `verify`, `explore`, `run-check`. An unknown intent is an error.

`--artifact` names what the request produces when that decides the workflow: `spec-audit`
(assess a specification; `story`) or `adr` (record a decision; `plan`). Without it, `story`
leads with the story workflow and `plan` with change planning. An artifact that does not
apply to the intent is an error. Prompt hints set it from the request.

The result contains:

- a [loading plan](PLAYBOOKS.md): workflow, practices and independent language, runtime,
  framework, layer and provider selections, each with its evidence, plus missing context;
- logical skills (`package:skill`) and conditionally selected references;
- owning modules, their dependents and declared checks (as argv, never assumed to pass);
- execution mode, role and an optional configured model;
- tool plans and missing bindings or skills.

Skill availability is reported as unverified; hosts resolve logical names against what is
installed. `craft lookup --root <project> [--file <path>... | --module <name>]` returns the
module inventory or the most specific owner of each path.

## Project configuration

`.craft/project.json` (every field: [project configuration](PROJECT-CONFIG.md),
example: `examples/project/`) declares modules with paths,
owners, `dependsOn`, checks and stack evidence (`languages`, `runtimes`, `frameworks`,
`layers`, `providers`). Module fields override project defaults; an empty array clears
them. Declare stack evidence per module in mixed repositories. Nothing is inferred beyond
what is declared: TypeScript does not imply Node, Java does not imply Spring.

Risk tags (`--risk timeout`, `concurrency`, `flaky-test`, …) select deeper guidance; they do
not claim a defect exists.

## Project standards

`standards` in `.craft/project.json` declares what the project's own standards are and where
they apply. Each entry has an `id` and one source: a `file`, a `folder` of documents, or an
`anchor`, the name of an established standard. Routes show the matching standards by id,
level, source and a one-line `summary`; the content stays in the project.

```json
"standards": [
  { "id": "commits", "anchor": "conventional-commits" },
  { "id": "changelog", "anchor": "keep-a-changelog" },
  { "id": "adr", "anchor": "adr-according-to-nygard", "scope": { "paths": ["docs/adr"] } },
  { "id": "stories", "anchor": "connextra-user-story", "scope": { "paths": ["docs/stories"] } },
  { "id": "domain", "anchor": "domain-driven-design" },
  { "id": "api", "file": "docs/api-style.md", "summary": "REST naming and error envelope",
    "scope": { "layers": ["backend"] } },
  { "id": "migrations", "checklist": ["Backward compatible with the previous release"],
    "scope": { "paths": ["db/migrations"] } }
]
```

A project that declares no standards is held to the ones its existing files state, and
`craft standards --propose` lists them, so the first session needs no configuration and the
first declarations no writing by hand. The scan reads fixed files at the project root, each
up to 256 KiB, and follows no links:

| The project has | Proposed |
|---|---|
| Conventional Commits, Keep a Changelog, Semantic Versioning, MADR, Nygard ADRs or Gherkin named in `AGENTS.md`, `CLAUDE.md`, `GEMINI.md`, Copilot or Junie instructions, or `CONTRIBUTING.md`; a changelog preamble naming its format; a commitlint configuration extending `config-conventional` | The anchor, checked. Decision-record anchors are scoped to the first of `docs/adr`, `docs/decisions`, `docs/architecture/decisions`, `doc/adr`, `adr` that exists, and are not proposed without one |
| A `lint`, `format:check` or `typecheck` script in `package.json` (and the usual spellings) | A `run` standard with the script, through the package manager its lock file names |
| Configuration of rustfmt, Clippy, ESLint, Biome, Prettier, Ruff, Black, Flake8, mypy, Pyright, golangci-lint or RuboCop; a `tsconfig.json` | A `run` standard with the tool's check command, scoped to its language; a package script for the same job replaces it |
| Checkstyle, Spotless, ktlint or detekt configured in the project or named in its Gradle or Maven build | A `run` standard with the build's own task: `./gradlew` when the wrapper is there, else `gradle`, or `mvn`. Not proposed without the build file |
| `go.mod`; a solution or project file (`.sln`, `.slnx`, `.csproj`, `.fsproj`) at the root | `go vet ./...`; `dotnet format --verify-no-changes`. Advised, not run: the file shows the toolchain, not a request for the check. Declare the standard to have `craft check` run it |
| At least 20 recent commits of which four in five have a Conventional Commits subject; a changelog whose release headings already pass Keep a Changelog; two or more numbered decision records that all pass MADR or Nygard | The anchor, checked, with what was observed as its evidence. Never proposed when what exists does not pass the check |
| `CONTRIBUTING.md` (root, `.github/` or `docs/`), `.editorconfig` | A `file` standard, advised; the EditorConfig summary states what `[*]` asks for |

Each proposal carries the declaration to copy and the files, with lines, that support it;
what the configuration already declares is listed apart. Craft never writes the
configuration.

While the project's own file declares no `standards`, every proposal is in effect as the
`detected` layer, beneath the organization and user files: an anchor is checked as a declared
one is, a `file` standard is advised, and a `run` standard is a check `craft check` runs when
it is called. No hook starts a command read from the project's files. A standard of the same
id in the organization or user file wins. Once the project declares a standard of its own,
only declared standards apply: copy the proposals to keep. Any entry counts, a lone
`{"id": …, "disabled": true}` included, so one detected standard cannot be removed alone:
declare the ones to keep. `craft config` shows the layer,
and what the files state never makes a valid configuration invalid.

A format the project keeps without naming it is read from the last 50 commit subjects
(merges left out), `CHANGELOG.md` and the five newest numbered records of the decision
folder. A hook asks Git once per commit the checkout is at and keeps the answer in
`.craft/state/notices/practice.json`; a command you run keeps nothing.

A root that is no repository and has no `.craft/project.json` is read as a workspace: the
scan also reads each direct folder that is a Git repository, at most 16, and holds every
standard it finds there to that folder. The id is led by the folder's name (`api-rustfmt`),
`scope.paths` and files by the folder, and a command runs in it, so a file is routed and
checked with its own repository's standards and not its neighbour's. A commit format is
brought to the workspace only when every member repository has it. A member's standard that
cannot be held at the workspace (two folders whose names give the same id, no room left) is
listed under `skipped` with the reason. State stays in the one
`.craft` at the workspace root. The stop review reads each member repository (its changed
files, and the commits the session made there) and names files from the workspace root; it
does so at any root that is no repository, with or without a `.craft/project.json`. The commit
check reads the newest commit made in the last two minutes among the member repositories. Not
covered there: a member's own `.craft/project.json`, and a repository deeper than a direct
folder.

Otherwise the scan matches names and
configuration files only: it does not parse commands written in prose, does not read nested
packages of one repository, and a file that names a convention in order to reject it is still a match, so read
the evidence. Agent instruction files are read for names but never proposed
as standards, since the host loads them itself.

A session that starts in a project declaring no standards says once how many standards
read from its files are in effect, to the user where the host shows a hook's message
(Claude Code, Codex, Gemini CLI) and to the agent with the command that lists them. The same
set is not named again in any session; a changed set is. `routing.conventionProposals: false`
turns the scan off: nothing is read from the files and nothing is said.

- `scope` narrows where a standard applies: `modules`, `languages`, `layers`, `paths` and
  `intents` (the kind of work: `story`, `plan`, `verify`, `explore`, `review`, `implement`,
  …). Every key given must match; any value within a key matches. Without `scope` a
  standard applies to the whole project. Without files, a standard scoped by place appears
  only for the kind of work it names, so story and decision standards reach product and
  architecture work before any file exists.
- `level` is `advise` (named when work reaches the scope), `check` (an anchor's check, or a
  `run` command the agent is told to run) or `review` (a `checklist` shown with the route).
  It defaults from the declaration: `run` checks, checklists review, files advise; an anchor
  is checked when it has a check, reviewed when it has a checklist, advised otherwise.
- Anchors Craft checks are `conventional-commits`, `keep-a-changelog`, `semantic-versioning`,
  `adr-according-to-nygard`, `madr`, `gherkin`, `ears-requirements`, `connextra-user-story`,
  `hexagonal-architecture` and `clean-architecture`; `semver`, `nygard-adr`, `ears`,
  `connextra` and `ports-and-adapters` are aliases. Any other id of the Apache-2.0
  Semantic Anchors catalog, embedded from a pinned commit with its notice, can be named at
  `advise` or `review`: routes show its title, never its text. An unknown name is a
  configuration error, and so is a name Craft declines because models do not agree on what it
  requires (`clean-code`); the error says why. Story and requirement anchors apply to `story`
  work, ADR anchors to `plan` work, unless `intents` says otherwise. The catalog anchors Craft
  adds by default (see Project anchors) show Craft's one-line summary and apply to the kinds
  of work they are added for.
- After a hook sees a file written in an anchor standard's scope, or a `git commit` made by a
  shell command, Craft checks the anchor's required elements without a model and reports
  unmet ones in the same tool result. It never blocks the tool. `keep-a-changelog` reads
  `CHANGELOG.md`, `gherkin` reads `*.feature`, `semver` reads changelogs and package
  manifests; the others read the files under `scope.paths`. With `scope.paths`, those three
  read their own files inside a named folder, and any file a path names exactly. Checks cover
  structure, not whether the content is right, and an ADR's sections are checked in any
  order.
- `hexagonal-architecture` and `clean-architecture` check module dependencies instead of
  text. Declare each ring or layer as a module with the `dependsOn` it may import. When a source
  file in such a module is written, Craft reports each import that the change adds (compared with
  the file at `HEAD`) and that points into a declared module the file's module does not list.
  Imports that were already in `HEAD`, modules without a `dependsOn` field and imports Craft
  cannot place in the project are never reported. Imports are placed for Rust (crate names
  from the modules' `Cargo.toml`), Go (`go.mod` module path), Java (package path), TypeScript
  and JavaScript (relative paths and the modules' `package.json` names) and Python (relative
  imports, and absolute ones that name a directory in a module). Other languages are not
  checked. A project anchor with `"check": "module-dependencies"` applies the same rule
  without an architecture name.
- `craft anchors` lists the project's and Craft's anchors with the source each name resolves
  to; `craft anchors --search <text>` searches the catalog too, by id, title, category or role.
- `craft standards` lists the declarations with the source of each anchor; `craft standards --file <path>` and
  `--commit-message <file>` run the same checks and exit non-zero when one is not met, for a
  Git `commit-msg` hook or CI. `routing.standardChecks: false` turns the hook checks off.
- A session restored after compaction carries the `check` and `review` standards of its scope
  in `standards`: the session's established work and, for a bound task, the task's files and
  owned paths, selected as for a delegated worker. A bound task's resume carries them too
  when it is adopted in a new session.

## Stop review

When a turn ends after the session changed files, Craft reviews those files once and, where
the host can continue a turn from a stop hook (Claude Code, Codex, Copilot CLI, Gemini CLI,
Cursor), holds the stop with one summary the agent addresses before reporting done:

- module dependency findings (above) for imports the changes add;
- anchor check findings (above) for a changed file the session was not told about when the
  file was written: a file a shell command or a script wrote passes no edit hook, so its
  check runs here;
- debt markers the changes add, when `routing.debtMarkers` is true: a
  TODO/FIXME/XXX/HACK comment, a lint or type-check suppression (`eslint-disable`,
  `@ts-ignore`, `# noqa`, `#[allow(`, `@SuppressWarnings`, …), a skipped or disabled test
  (`it.skip(`, `#[ignore]`, `@Disabled`, `@pytest.mark.skip`, …) and fewer assertion lines
  than the change removed. Text inside a string literal or a Markdown code span on the same
  line is not a marker. Commented-out code is not detected;
- the declared `review` standards whose scope the changed files fall under, with their
  checklist.

Only files whose content changed since the session started, or since the previous stop, are
reviewed, whichever tool wrote them: uncommitted changes, and files in commits made since
then, so work committed before the turn ends is still reviewed. Markers are those added since
the commit the previous stop saw. Uncommitted work from before the session is not reviewed,
and neither are commits that arrived another way: after a pull, a merge, a checkout, a rebase
or a reset, the stop takes nothing from the commits in between. One stop reviews at most 64
files. A finding or review item is
said once per session, and a stop is never held while the host is already continuing a held
one. Nothing is enforced: the agent fixes or tells the user why not. The review runs only
when the project reports debt markers, declares a `review` standard, checks module
dependencies or checks files against an anchor; `routing.stopReview: false` turns it off.

## Test runs

When the agent runs a test command through the host's shell tool (`cargo test`, `node --test`,
`pytest`, `python -m unittest`, `vitest`, `jest`, Maven, and wrappers such as `npm test` or
`npx`, also as Windows `.cmd`, `.bat` or `.exe` and `py -m`), Craft reads the runner's summary
line from the tool result the host reports and keeps the verdict for the session with the
source digest of the tree it ran on:

- `failed` when the exit code says so, when the summary counts failures, or when no test
  executed (`zero-tests`: a filter matched nothing, tests were skipped by a flag;
  `all-skipped`), whatever the exit code;
- `unknown` when the output carries no summary Craft recognizes (another runner, output cut
  before the summary, `cargo test --no-run`), or the host reports a failed call with neither
  summary nor exit code (`tool-failed`); never `passed`. Output the command piped into a
  filter (`| tail -5`, `| grep`, anything but `tee` or `cat`) may show only the last of several
  summaries: from it a shown failure still fails the run, anything else is `unknown`
  (`cut-output`);
- `passed` only with at least one executed test and no failures.

A run that checked nothing (no executed test, or `unknown`) neither confirms nor contradicts
an earlier run: the test status of a tree is its latest decisive run (passed, or failed with
failures or a failing exit), and only the latest run when none decided. A run that exited
cleanly without executing a test is reported in the same tool result where the host shows
tool context. A resumed task names the test status of the current tree, or `not-run` when the
tree changed since. Claude Code reports a failing command as `PostToolUseFailure` and keeps a
long output in a file whose end Craft reads for the summary; Codex passes the output without
an exit code, so its summary alone decides; Gemini CLI's shell result text is read the same
way. A result with neither output nor a failing exit code, an interrupted call and a call
moved to the background record nothing. `routing.testRuns: false` records no run, reports no
run that executed nothing and leaves test status out of a resume. With no run observed, an
acceptance record is not filled from one and a declared pass is not checked against one: a
`task-step` that says verification passed is recorded as the agent states it. With runs
observed, that step is refused while the test status of the current tree is `failed`, which
includes a tree whose latest run executed no test and no run decided; when that status is
`unknown`, the pass is recorded as stated.

## Project checks

`craft check` runs the commands a project declares: the `checks` of the modules that own the
changed files and the `run` command of each `check` standard whose scope covers them. The
changed files are the uncommitted ones, or those named with `--file`; `--all` takes every
declared check, as a CI job on a clean tree needs. A module check runs in the module's path,
a standard's command in its `cwd`.

Each command may name a `stage`: `step`, `commit` (the default), `pr` or `ci`. A run at
`--stage commit` executes the `step` and `commit` checks and lists the others as
`left-to-pr` or `left-to-ci`. A check left to a later stage, one that could not start
(`not-started`), one ended at its time bound (`timed-out`, 600 seconds unless
`timeoutSeconds` says otherwise) and a test command whose runner summary executed no test are
never reported as passed. A run in which no check was executed is `nothing-run`, not
`passed`.

```json
{
  "modules": [
    { "name": "api", "path": "services/api", "checks": [
      { "command": "cargo", "args": ["fmt", "--check"], "stage": "step" },
      { "command": "cargo", "args": ["test"] },
      { "command": "cargo", "args": ["test", "--features", "slow"], "stage": "ci", "timeoutSeconds": 1800 }
    ] }
  ]
}
```

The result carries the source digest of the tree the commands ran on, and `treeChanged` when
a command (a formatter, a code generator) changed it. Output is shown only for a check that
did not pass, from its end. With `--session` the run is kept in the session's records, and a
test command among the checks counts as an observed test run. Commands start without a shell,
exactly as declared, and only from this command: no hook runs them.
`routing.projectChecks: false` makes `craft check` run nothing and say so.

## Project anchors

`anchors` in `.craft/project.json` adds the project's own anchors, or changes Craft's:

```json
"anchors": { "catalogs": [".craft/anchors.json"], "disables": ["testing-pyramid"] }
```

```json
{ "anchors": [
  { "id": "acme-adr", "title": "ACME decision record", "check": "nygard-adr",
    "checklist": ["Owner named"], "intents": ["plan"] },
  { "id": "keep-a-changelog", "summary": "Our changelog: releases by date", "check": "keep-a-changelog" }
] }
```

- `catalogs` lists up to eight project-relative files, read in order. Each entry has an `id`
  and a `summary` or `title`; optional `checklist`, `check`, `intents` and `targets`. A
  missing or invalid file is a configuration error; hooks then run no checks.
- An entry with the id of a Craft or catalog anchor, or of an alias, replaces that entry
  whole: a project may give `clean-code` its own meaning, and an entry without `check` is no
  longer checked. A later file replaces an earlier entry with the same id.
- `check` names one of Craft's checks or an anchor that has one, so a project's own name can
  reuse it. Without `targets` a check reads the files Craft's anchor for it reads.
- `defaults: true` turns on the anchors Craft adds by kind of work; they are off unless a
  project turns them on. A default is advised, with Craft's one-line summary, and never
  checked. Any declared standard for the kind of work (through `scope.intents` or its
  anchor) replaces all defaults for it, so a project that declares `madr` for `plan` is not
  told about Nygard. An anchor the project already names, or lists in `disables`, is not
  added. A name in `disables` must resolve.

| Kind of work | Craft adds |
|---|---|
| `implement` | `conventional-commits`, `kiss-principle` |
| `plan` | `adr-according-to-nygard` |
| `story` | `invest`, `moscow` |
| `verify` | `bdd-given-when-then`, `testing-pyramid` |
| `review` | `testing-pyramid`, `kiss-principle` |

## Project skills

`skills` in `.craft/project.json` tells routes which of the project's own skills to
recommend. Hosts already list every skill in their project folders; Craft does not load,
inject or hide them, it only decides what a route names.

```json
"skills": {
  "declared": [
    { "name": "team-review", "path": ".claude/skills/team-review",
      "replaces": ["craft-review:review-change"] },
    { "name": "release-notes", "path": ".agents/skills/release-notes",
      "scope": { "intents": ["implement"], "paths": ["CHANGELOG.md"] } }
  ],
  "disables": ["craft-engineering:react-engineering"]
}
```

- `path` is a skill folder at the project root in one of the folders hosts read:
  `.claude/skills` (Claude Code, Copilot CLI, Cursor), `.agents/skills` (Codex, Copilot CLI,
  Cursor, Gemini CLI, Junie), `.github/skills` (Copilot), `.cursor/skills`, `.codex/skills`
  (Cursor), `.gemini/skills` (Gemini CLI), `.junie/skills` (Junie). Craft reads only the
  front matter of the declared `SKILL.md`; its `name` must equal `name`.
- `replaces` puts the skill where a route would name those package skills; `scope` (as for
  standards) narrows that, or on its own adds the skill to matching routes. A declaration
  needs one of them. Routes name the skill `project:<name>`; hosts are shown the bare name.
- A host that does not read the declared folder uses a copy of the skill in a folder it does
  read, with the same folder name and front matter `name` (`.agents/skills/team-review` for
  Codex, `.claude/skills/team-review` for Claude Code); `skillSources` names the copy used.
- When the current host reads neither, or the `SKILL.md` is missing or names another skill,
  the route keeps the package skill and says why in `missing`, for example
  `project:team-review unavailable: claude does not read .agents/skills; mirror the skill to
  .claude/skills/team-review; routed craft-review:review-change instead`.
- `disables` removes package skills from routes. A host may still list them: Craft never
  edits host configuration.
- Conflicts are configuration errors, never merged: two declarations replacing one package
  skill, or a package skill both replaced and disabled.
- A route that a declaration changed carries `skillSources`: each project skill with its
  path and what it replaces, each unavailable one with the reason, and each disabled
  package skill. Telemetry records package skill names only.

## Intents with special scope

- `verify` selects testing-only work. `--verification-mode manual|automation|test-code`
  distinguishes observations, existing checks and requested test changes; none selects
  product-code changes.
- `explore` selects a bounded manual testing charter, without assuming a repository,
  runner or browser.
- `run-check` plans the declared module commands for the affected paths; they stay
  `not-run` until someone executes them.
- `locate` selects only `craft-engineering:locate-code`.

## Lookup

```sh
craft locate --root <project> --path src --term Handler --term configKey --max-matches 20
```

One bounded scan for all literal terms. In a Git worktree, Git's own text search first names
the files that contain a term, so file, byte and match bounds are spent only on those and the
file set equals what `git grep` finds. It respects Git ignores and skips symlinks, binary and
oversized files. It prints path, line and snippet evidence as text, lists the matching files
whose lines no longer fit the match bound, and reports truncation and skips; `--json` prints the
full receipt. It neither classifies definitions nor proves absence. Session reuse is described
in [sessions](SESSION.md#lookup-reuse).

## Delegation and model advice

`--delegate` selects a configured worker role only when both the role and the host enable
it. Roles map to logical profiles, and profiles map to exact model names per host in
`execution`; nothing vendor-specific is built in. Routing never starts a worker. Brief a
worker with scope, evidence bar, affected files and return shape, never the whole
conversation. Isolate concurrent writers in separate worktrees; the coordinator owns review
and integrated checks. `efficiency.delegation.maxWorkers: 0` disables worker plans.

When the host starts a subagent itself (Claude Code and Codex `SubagentStart`, Copilot CLI
`subagentStart`), the subagent receives the project standards of the delegating session's
work as context: the kind of work the session established with the files it was routed to,
and the files and owned paths of the task bound to the session. Hosts do not pass the delegated
prompt, so nothing is guessed from it; a session with no established work and no bound task
adds nothing, and so does a project without standards. The delegating instructions still
define the subagent's task. Cursor's subagent hook cannot add context, and Gemini CLI and
JetBrains have none. `routing.workerStandards: false` starts subagents without them.

```sh
craft model-advice --root <project> --data request.json
```

`model-advice` recommends a configured profile for `host`, `role`, `task`, `revision`, and
optionally `capabilityClass`, `currentProfile` and `attempts`. Profiles declare capabilities,
`maxAttempts` (1–5, default 3), `class` (`economical|standard|strong`) and `escalateTo`.
Only quality failures on the same task and revision count toward escalation. A
`permission-denied` or `tool-unavailable` attempt stops advice until it is resolved. Without
a chain, advice stops rather than inventing a stronger model.

## Hints and guidance size

Prompt and edit hooks add compact routing hints (`routing.promptHints`, `routing.editHints`;
with `promptHints: false` a request still establishes the session's intent, unshown). A prompt hint names
the intent its request leads with ("Can you fix…", "Where is…", "Write tests for…"); a
change request turns a lookup or a test request into `implement` ("write a failing test,
then fix it" is a change, not testing-only work), and statements, chat or questions
about the conversation get no hint. Where a result goes ("write them to `ANSWER.md`") never
decides the intent, what someone else wants ("a teammate wants to change…") is context
while the question they asked is the request, and text pasted after a question that names
no request does not decide one either. The words are English and matched deterministically;
an unrecognized request simply gets none. A lookup prompt gets a hint only when the hook
answers it (below): a hint that merely pointed at a command cost the model one or two turns
more than its own search. An answered hint also carries `lookup`: the `craft locate` command
of the runtime that answered the hook, rooted at the project, and the questions it can ask,
for a search beyond the answered question. It names no skill: a lookup should not cost
reading the skill, package instructions and setup first.

A change request (`implement`) gets no prompt hint either, unless a task is bound, the route
reports a missing input or the hook answers a lookup in it. Its skill names sent models to read
every skill before a one-line fix, at 1.76 times the tokens of the same fix without Craft. The
request still establishes the session's intent, so the first edit's hint names the skills and
references for the edited scope where the change happens.

A hook hint names only skills and references of packages the host installed. The hook looks
beside the package it runs from: the other packages of the same marketplace in a plugin cache
(Claude Code, Codex), except those the host's settings turn off, or the package folders of a
project overlay. Skills of other packages leave the route and appear in `skillSources` with
status `not-installed`; the route reports `skillAvailability: "installed-packages"`. The
project's own skills stay. A route left with no skill says nothing unless it carries a
standard, a bound task or a missing input. `craft route` on the command line does not look
and reports `not-verified`.

When the prompt asks who calls one code name or where it is declared ("which methods call
`Policy.apply`?", "where is `apply_discount` defined?"), the prompt hook runs that lookup
too, refreshing Project Intelligence for at most one second per prompt (the declaration check
for callers runs only on a result that could answer and shares that second), and adds `answer`: up to ten
cited rows (`path:line in method: excerpt`) with the question and scope. It answers only when
it is certain: coverage is complete, no gap file matched literally, the rows fit the hint,
and for callers the name is declared exactly once in the project, in the source a qualifier
names (`Policy` in `Policy.apply`, `module` in `module::fn`). Two questions, two names, a plain
word or anything less certain add no answer, and then no hint.

A search the model runs itself gets the same certainty without a turn. After a Grep-style
search tool with a literal pattern, or a shell command whose one `rg`, `grep` or `git grep`
looks for one literal identifier (word-bounded, optionally qualified or followed by `(`), the
after-tool hook adds that name's declaration and callers to the tool result, under the same
rules: declared exactly once, complete coverage, at most ten rows. It refreshes Project
Intelligence for at most 200 ms, so it answers from the index the session warmed; a regex,
several patterns, an inverted match or anything less certain adds nothing.

A prompt also gets an answer, whatever intent it routes to (or with none), when it asks where a
dotted configuration key is set (`` Where is `compilerOptions.strict` set? ``: `--key`), which
documents mention one code name or file (`` Which docs mention `AdrSlug`? ``: `--mentions`),
or about decision records ("Which ADR decides…", "architecture decision record"): the index of
every record with its status, when there are at most 24. Records under a test or fixture
directory (`test`, `tests`, `__tests__`, `spec`, `fixtures`, `testdata`, `e2e`, or a name
ending in `-tests` or `-fixtures`) are test data: the answer leaves them out and says how
many. Same rule as above: complete coverage or nothing. `efficiency.lookup.enabled: false` turns all hook answers off.

### What a session's hooks may inject

Hook answers are bounded per answer, per hook and per session, so a long session does not pay
for the same text again and again.

- **Per hook.** One hook answer carries at most 9,000 bytes of context on every host
  (`contextBytes` in `hosts/hooks.json`). Claude Code replaces a hook string over 10,000
  characters with a file path and a preview, Codex cuts a hook message over about 10,000 bytes
  and Copilot caps tool-result context at 10 KB; a context over the cap reaches the model in
  part or not at all. A context that would pass the ceiling loses whole pieces, least
  important first (planning evidence and unfinished tasks of earlier sessions, the service
  map, open work plans, then hints), and ends with one line naming what was left out. The
  session identity, a resumed task and a test run that ran nothing are kept longest. Only a
  single piece larger than the ceiling is cut, with a marker.
- **Per session.** Hook answers from Project Intelligence stay within 16 KiB per context
  window, and an answer with the same text is given once per window: a search or question
  repeated forty times is answered the first time. A compaction starts a new window, because
  what was answered before it is no longer in the model's context. Past the budget the lookup
  does not run; the model's own search result stands, as it does without Craft.

The counts are kept in the session's `delivered.json` (under `routing.sessionRecords`; without
it there is no session budget and each answer is bounded only on its own) and shown by
`craft session`. Standards findings and test notices are not budgeted: they report what was
just written or run.

A recognized request establishes the session's intent, also on hosts that cannot show the
prompt hint. Edits are side effects of that intent: writing `PLAN.md` in a planning session or
`ANSWER.md` after a lookup routes nothing. An edit routes `implement` only when no request
established another intent, once per file until the next request, and only at one tool hook:
before the tool runs where the host shows context there, otherwise after it.

For requests that create, change or assess something, what they are about decides the
intent. Feedback and reading requests ("any feedback…", "look at the diff…") are reviews when
they name a change and otherwise follow the same table:

| About | Create ("write", "draft", "prepare") | Assess ("review", "compare", "what…") |
|---|---|---|
| Code or a change | `implement` | `review` (`plan` for compare/evaluate) |
| Spec, PRD, story, acceptance criteria | `story` | `story` |
| Tests, scenarios, coverage | `verify` | `verify` for questions, `review` for review |
| ADR, decision, options, trade-offs | `plan` | `plan` |
| Exploratory charter | `explore` | — |
| Report, message, notes, summary | no hint | no hint |

Identical hints are suppressed for up to 15 minutes (`routing.dedupeHints`); session start
and compaction reset this. Suppression never blocks work.

```sh
craft context-report --root examples/project --intent review --file services/orders/Order.java --budget-bytes 20000
```

`context-report` measures the selected guidance files: paths, hashes, bytes and estimated
tokens (`ceil(bytes / 4)`, a heuristic). The budget is report-only and never removes
guidance. It does not measure host context; see [sessions](SESSION.md#what-is-measured).
