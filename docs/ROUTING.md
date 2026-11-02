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

One bounded scan for all literal terms. It respects Git ignores and skips symlinks, binary
and oversized files. It reports limits and truncation, and returns path, line and snippet
evidence. It neither classifies definitions nor proves absence. Session reuse is described
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

Prompt and edit hooks add compact routing hints (`routing.editHints`). A prompt hint names
the intent its request leads with ("Can you fix…", "Where is…", "Write tests for…"); a
later change request turns a lookup into `implement`, and statements, chat or questions
about the conversation get no hint. Where a result goes ("write them to `ANSWER.md`") never
decides the intent, what someone else wants ("a teammate wants to change…") is context
while the question they asked is the request, and text pasted after a question that names
no request does not decide one either. The words are English and matched deterministically;
an unrecognized request simply gets none. A lookup hint also carries `lookup`: the
`craft locate` command of the runtime that answered the hook, rooted at the project, and the
questions it can ask, so the agent can run it without resolving the package path itself.
Such a hint names no skill: the command and its output explain themselves, and a lookup
should not cost reading the skill, package instructions and setup first.

When the prompt asks who calls one code name or where it is declared ("which methods call
`Policy.apply`?", "where is `apply_discount` defined?"), the prompt hook runs that lookup
too, refreshing Project Intelligence for at most one second, and adds `answer`: up to ten
cited rows (`path:line in method: excerpt`) with the question and scope. It answers only when
it is certain: coverage is complete, no gap file matched literally, the rows fit the hint,
and for callers the name is declared exactly once in the project, in the source a qualifier
names (`Policy` in `Policy.apply`, `module` in `module::fn`). Two questions, two names, a plain
word or anything less certain add no answer; the runnable `lookup` remains either way.

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
