# Craft Capabilities

Your team's engineering rules, kept by any agent: declared once, checked while it writes,
enforced where you choose, the same in every project and on every host.

![What Craft tells the agent and the person during one task: callers from the project index for
a search, a module dependency finding, an empty test run, a commit message finding, a refused
force push, a declared check run and the session summary](docs/assets/demo.svg)

The `craft` lines are what the released executable answered to Claude Code's hook payloads for
these steps, and to `craft check`; the agent's steps are scripted, not a model run.

Engineering practices and workflows for AI coding agents, together with your project's own
rules and skills, installed into Claude Code, Codex, Gemini, Cursor, Copilot, JetBrains AI
Assistant or any Agent Skills host from one source.

Agents and hosts change; your engineering standards should not. Craft ships useful defaults,
lets the project declare what "good" means for it, brings the right part to the agent while
it works, and checks what can be checked without a model.

## Why it exists

An agent starts every session from general knowledge. Your commit format, architecture
rules, ADR template and review checklist live in documents it may not read, or reads once
and loses after compaction. "Tests pass" is often a claim, not an observed run. Your CI
linters catch some of this, but only after the agent has finished and handed the work back.
Writing plugins, hooks and skills for each host to fix that is infrastructure most teams
should not have to maintain.

Craft ships working conventions by default and lets your project customize where it differs:

- **Defaults.** Engineering guidance and workflows for planning, implementation, testing,
  review, stories and shipping. Each task gets only the guidance its stack and kind of work
  need, not everything at once.
- **Your rules and skills.** `.craft/project.json` declares your modules, your standards (a
  file, a folder or an established name such as `conventional-commits`), where they apply and
  how strictly, and your own skills. On conflict, yours win, and `craft standards` shows which
  source supplied each piece.
- **One baseline for every project.** An organization file and a user file declare standards,
  checks and a safety policy once; each project overrides only what differs, and
  `craft config --explain` shows which layer set each value.
- **Feedback while the agent works.** Checks that need no model run on what the agent has
  just written, so it corrects course in the same turn, not after review.
- **Evidence instead of claims.** Test results are read from the runner's own output. What
  was not run is reported as not run, and what Craft cannot read as unknown, never as passed.

## One task, with and without Craft

A project declares its modules and three standards:

```json
{
  "formatVersion": 1,
  "modules": [
    { "name": "domain", "path": "src/domain", "dependsOn": [] },
    { "name": "adapters", "path": "src/adapters", "dependsOn": ["domain"] }
  ],
  "standards": [
    { "id": "commits", "anchor": "conventional-commits" },
    { "id": "rings", "anchor": "hexagonal-architecture" },
    { "id": "migrations", "checklist": ["Backward compatible with the previous release"],
      "scope": { "paths": ["db/migrations"] } }
  ]
}
```

Then asks the agent to add discount codes to orders.

**Without Craft**, the agent works from what it already knows. It may import the database
adapter straight into the domain, write `fixed discounts` as the commit message, run a test
filter that matches nothing and report that the tests pass.

**With Craft**, on the same host:

- The route for the change names the standards in its scope, one line each; their content
  stays in your project.
- When the agent writes `src/domain/order.ts` with `import { db } from "../adapters/db"`, the
  result of that edit tells it (Claude Code, verbatim):

  ```text
  Project standards not met by what was just written (deterministic anchor check; report only,
  fix or tell the user why not): [{"standard":"rings","anchor":"hexagonal-architecture",
  "findings":[{"rule":"undeclared-dependency","line":1,"message":"imports ../adapters/db from
  module adapters; module domain does not declare dependsOn adapters"}]}]
  ```

- The commit message is checked against Conventional Commits in the commit's result.
- A test command that executed no test is recorded as failed, not passed.
- When the turn ends, the agent gets one summary of the changed files: the migration
  checklist if a migration changed, and any dependency findings still open. It addresses them
  before reporting done.
- After compaction, the session gets those standards back with its work.

Nothing is blocked; the agent fixes the finding or tells you why not. The same checks run
outside the agent: `craft standards --root . --file src/domain/order.ts` prints the finding
and exits 2, for CI or a Git hook.

## What else it does

- **Workflows for whole tasks.** Deliver a change, review a local diff or a GitHub/GitLab
  review, write code-grounded stories, ship a release: each a named workflow that loads the
  guidance it needs and reports what it verified.
- **Your checks, at the stage you chose.** `craft check` runs the lint, type and test commands
  a project declares for the changed files, each at its stage (`step`, `commit`, `pr`, `ci`).
  A check left to a later stage is listed as not run, never as passed
  ([project checks](docs/ROUTING.md#project-checks)).
- **A safety policy you turn on.** With `safety.enabled`, recursive deletes outside the
  project, forced pushes, Git commands that discard uncommitted work and system-level commands
  are refused before they run, together with the project's own rules. It only refuses and
  never approves ([security](docs/SECURITY.md)).
- **Standards from what you already have.** `craft standards --propose` reads your agent
  instructions, contributor document and tool configuration and lists the standards they
  support; nothing is active until you copy it.
- **One line on what happened.** When a turn ends, Claude Code, Codex and Gemini CLI show
  what Craft did in the session: findings raised and fixed, test runs that ran no test,
  lookups answered and contexts restored. Nothing happened, no line.
- **Continuity.** After compaction, a task gets back its goal, decisions, open result and test
  status. Subagents started by Claude Code, Codex or Copilot CLI receive the same standards.
- **Fewer turns.** When the agent asks who calls a name, where it is declared or which ADR
  decides something, a hook can answer from a local index
  ([Project Intelligence](docs/PROJECT-EVIDENCE.md)), but only when the answer is complete.
- **Trustworthy completion.** In a Craft task, a step that claims tests passed is refused while
  the latest run on the current files failed. An opt-in
  [acceptance contract](docs/ACCEPTANCE.md) refuses completion while a required criterion
  lacks current passing evidence. See [test runs](docs/ROUTING.md#test-runs).
- **Not only code.** Story, specification, decision-record and exploratory-testing work get
  the same standards and evidence, with [reader views](docs/ACCEPTANCE.md#reader-views) for
  QA, product and architecture.

Without configuration, Craft supplies the defaults, observes test runs, answers lookups and
resumes tasks; it checks nothing against an architecture you have not declared.
[Project configuration](docs/PROJECT-CONFIG.md) lists every field with its default, and every
hook output has a switch ([routing](docs/ROUTING.md#project-standards)).

## Install

Claude Code:

```sh
claude plugin marketplace add Aliaksie/engineering-pack#distribution
claude plugin install craft-engineering@craft-capabilities
claude plugin install craft-delivery@craft-capabilities
```

Codex CLI:

```sh
codex plugin marketplace add Aliaksie/engineering-pack --ref distribution
codex plugin add craft-engineering@engineering-pack
```

Gemini CLI:

```sh
gemini extensions install https://github.com/Aliaksie/engineering-pack --ref distribution
```

Each package bundles a native `craft` executable; no Node is needed. `distribution` is the
stable release; `distribution-next` carries the candidate built from every change to `main`.
Swap the branch name in the commands above to try it. Other hosts, updates and rollback are
covered in [installation](docs/INSTALL.md).

## Try it

1. Check the installation. `craft doctor --root <project>` (the executable is `bin/craft` in
   any installed package) reports from local files whether each host would run Craft: the
   package is enabled, its runtime is present, Codex trust and hook approvals are recorded,
   and when a hook last ran. Or start a session and ask the agent:

   > Check Craft Capabilities setup, hook activity and session usage.

2. Give the agent a real task through a workflow, for example in Claude Code:

   ```text
   /craft-delivery:workflow-deliver-change Fix the rounding error in the order total
   ```

   Codex and other hosts use the package-prefixed name, such as
   `$craft-delivery-workflow-deliver-change`.

3. Declare one standard, then the modules you want checked, as in the example above. Link the
   schema with `"$schema"` for completion in your editor:

   ```json
   {
     "formatVersion": 1,
     "standards": [{ "id": "commits", "anchor": "conventional-commits" }]
   }
   ```

   `craft standards --propose` lists declarations your existing files already support
   (conventions named in `AGENTS.md` or `CONTRIBUTING.md`, lint and format configuration,
   `.editorconfig`), each with its evidence; copy the ones you accept. It writes nothing.
   `craft standards` lists what the project declares; `craft standards --commit-message <file>`
   runs the hooks' check and exits non-zero when it is not met, for a `commit-msg` hook or CI.

## Packages

| Package | Use it for |
|---|---|
| [craft-engineering](packages/craft-engineering/README.md) | Design, six languages, backend, UI, security, testing and agent engineering guidance |
| [craft-delivery](packages/craft-delivery/README.md) | Planning, implementation, routing, service setup and shipping workflows |
| [craft-review](packages/craft-review/README.md) | Revision-bound review of local diffs, GitHub PRs and GitLab MRs, including bots |
| [craft-stories](packages/craft-stories/README.md) | Code-grounded specifications and user stories |

Install only what you need; packages work alone and use each other when present.

## Hosts and platforms

| Host | Skills | Hooks |
|---|---|---|
| Claude Code, Codex | Plugins | Yes |
| Cursor, Copilot, Gemini CLI | Project overlay (Gemini also as an extension) | Yes, in the overlay |
| JetBrains AI Assistant | Project rule | No |
| Any Agent Skills host | `.agents/skills` overlay | No |

`craft` decides without a model, so the same facts give the same decision on every host. What
reaches the agent depends on the host: which hook events exist, whether a tool result or a
stop can carry context, and when a resume after compaction can be delivered.

Claude Code and Codex are tested in depth: an integration scenario drives `craft` with each
host's own hook payloads through every standards and evidence feature. The other hosts are
built from their published hook references and covered by payload fixtures, with less depth.
No host's live loading or model behavior is certified. The native executable is bundled for
macOS on Apple Silicon, Linux x86-64 and arm64, and Windows x86-64. See
[adapters](docs/ADAPTERS.md) for what each host supports.

## What is shown and what is not yet

- **Shown by tests on every change:** each check, routing decision and hook response, and an
  end-to-end scenario covering standards, anchors, routing, test evidence, resume after
  compaction, configuration layers, declared checks, the safety policy and `doctor`.
- **Measured:** the runtime cost of `craft` on Linux, macOS and Windows (see Performance), and
  turns and tokens in short model probes on Codex, where cost is watched as a guardrail.
- **Not yet shown:** that agents keep declared standards more often, or leave less debt, with
  Craft than without it. That is the goal; results will be published when measured, losses
  included ([evaluation](docs/EVALUATION.md)).

## What it does not do

`craft` never calls a model, switches models, starts workers, compacts your session or posts
to a code host. Standards checks report; they do not block tools. Two hooks deny, and only
shell commands: the safety policy, when a project turns it on, and craft-review's shell
guard, which with `review.readOnly: true` denies common publishing commands and, while the
project configuration is invalid, every shell command
([read-only sessions](docs/REVIEW.md#read-only-sessions)). Your host's permissions and your
authorization still decide what happens. Nothing is sent remotely unless you configure an
OpenTelemetry endpoint; bounded local usage metadata is on by default and can be disabled
([observability and privacy](docs/OBSERVABILITY.md)).

## Direction

- **Verified standards:** more of the project's own standards checked without a model, and
  the same declared check giving the same result in a hook, before a commit and in CI.
- **A baseline a team can share:** the organization baseline reaching CI runners and
  teammates, not only the machines that hold the file.
- **Service recipes:** ready GitHub, GitLab and Jira setups instead of a mapping per project.
- **Trust and less debt, cost as a guardrail:** Craft should earn its place through standards
  kept and debt avoided, without costing more where it adds nothing.
- **Project Intelligence and continuity:** keep what works and extend it only where it
  measurably saves work; task state that survives compaction and handoffs
  ([sessions](docs/SESSION.md)).

## Performance

Every push to `main` measures the installed release `craft` on Linux (x86-64 and arm64), macOS
and Windows: startup, hooks and Project Intelligence refresh. Any branch can be measured
against `main` by manual dispatch.
See the [dashboard](https://bencher.dev/perf/aliaksie-s-craft-capabilities) and
[contributing](docs/CONTRIBUTING.md#performance) for what is measured.

## Learn more

- [Documentation index](docs/README.md)
- [Project configuration](docs/PROJECT-CONFIG.md)
- [Project standards, anchors and skills](docs/ROUTING.md#project-standards)
- [Sessions, tasks and continuity](docs/SESSION.md)
- [Acceptance contracts](docs/ACCEPTANCE.md)
- [Commands](docs/COMMANDS.md)
- [Architecture](docs/ARCHITECTURE.md)
- [Contributing](docs/CONTRIBUTING.md)
- [Changelog](CHANGELOG.md)

Licensed under [MIT](LICENSE).
