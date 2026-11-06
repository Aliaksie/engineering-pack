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

`.craft/project.json` (schema: [project.schema.json](../schemas/project.schema.json),
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
  `adr-according-to-nygard`, `madr`, `gherkin`, `ears-requirements` and `connextra-user-story`;
  `semver`, `nygard-adr`, `ears` and `connextra` are aliases. Any other id of the Apache-2.0
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
  manifests; the others read the files under `scope.paths`. Checks cover structure, not
  whether the content is right, and an ADR's sections are checked in any order.
- `craft anchors` lists the project's and Craft's anchors with the source each name resolves
  to; `craft anchors --search <text>` searches the catalog too, by id, title, category or role.
- `craft standards` lists the declarations with the source of each anchor; `craft standards --file <path>` and
  `--commit-message <file>` run the same checks and exit non-zero when one is not met, for a
  Git `commit-msg` hook or CI. `routing.standardChecks: false` turns the hook checks off.

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
