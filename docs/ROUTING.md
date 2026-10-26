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

Prompt and edit hooks add compact routing hints (`routing.editHints`). Identical hints are
suppressed for up to 15 minutes (`routing.dedupeHints`); session start and compaction reset
this. Suppression never blocks work.

```sh
craft context-report --root examples/project --intent review --file services/orders/Order.java --budget-bytes 20000
```

`context-report` measures the selected guidance files: paths, hashes, bytes and estimated
tokens (`ceil(bytes / 4)`, a heuristic). The budget is report-only and never removes
guidance. It does not measure host context; see [sessions](SESSION.md#what-is-measured).
