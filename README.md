# Craft Capabilities

A portable engineering standards and evidence layer for AI coding agents, installed into
Claude Code, Codex, Gemini, Cursor, Copilot, JetBrains AI Assistant or any Agent Skills host
from one source.

Craft brings your project's declared standards to the agent when the work reaches them,
reports what can be checked without a model, keeps observed evidence apart from what the
agent claims, and carries decisions and rules across compaction and handoffs. It adds nothing
where it has nothing to say. It complements the host: reasoning, search, orchestration, model
choice and permissions stay with the host and with you.

## The problem, in one example

A project keeps its domain free of infrastructure. The rule is in an architecture document
the agent may not read, or reads once and loses after compaction. Asked to add discount
codes, the agent imports the database adapter straight into the domain.

With Craft, the project declares its modules and what each may depend on:

```json
{
  "formatVersion": 1,
  "modules": [
    { "name": "domain", "path": "src/domain", "dependsOn": [] },
    { "name": "adapters", "path": "src/adapters", "dependsOn": ["domain"] }
  ],
  "standards": [{ "id": "rings", "anchor": "hexagonal-architecture" }]
}
```

When the agent writes `src/domain/order.ts` with a new `import { db } from "../adapters/db"`,
the result of that edit tells it (Claude Code, verbatim):

```text
Project standards not met by what was just written (deterministic anchor check; report only,
fix or tell the user why not): [{"standard":"rings","anchor":"hexagonal-architecture",
"findings":[{"rule":"undeclared-dependency","line":1,"message":"imports ../adapters/db from
module adapters; module domain does not declare dependsOn adapters"}]}]
```

The edit is reported, not blocked: the agent fixes it or tells you why not. Only imports the
change adds are reported, compared with the file at `HEAD`. The same check runs outside the
agent, for CI or a Git hook: `craft standards --root . --file src/domain/order.ts` prints the
finding and exits 2.

## What else it does

- **Your standards, in scope.** `standards` names a file, a folder or an established
  standard (`conventional-commits`, `madr`, `gherkin`, …) and where it applies. Routes name
  the standards in scope, one line each; the content stays in your project. Ten anchors have
  structural checks without a model, such as the commit message after a `git commit`.
- **Your declarations decide.** A project anchor with Craft's name replaces Craft's entry, a
  standard you declare for a kind of work replaces Craft's default anchors for it, and your
  own skills can replace or disable package skills in routes. `craft standards` and the routes
  show which source supplied each piece. Craft does not merge or rewrite guidance text.
- **Observed test runs, not claims.** Craft reads the runner's summary from the shell result
  the host reports; it runs no tests itself. See [evidence and claims](#evidence-and-claims).
- **One review when a turn ends.** On hosts that can continue a turn, the changed files get
  one summary: dependency findings, the checklists of `review` standards in scope and, when
  `routing.debtMarkers` is on, new TODOs, lint suppressions, skipped tests or removed
  assertions. The agent continues once to address it.
- **Continuity.** After compaction, a session gets back the checked and reviewed standards
  of its scope and, for a task, its goal, decisions, open result and test status. Subagents
  started by Claude Code, Codex or Copilot CLI receive the same standards.
- **Fewer turns.** When a prompt or a literal search asks who calls a name, where it is
  declared, where a configuration key is set or which ADR decides something, the hook
  answers from a local index ([Project Intelligence](docs/PROJECT-EVIDENCE.md)), but only when
  the answer is complete; otherwise it adds nothing. A change request gets no prompt hint
  unless a task is bound or an input is missing; the first edit names the guidance for its
  scope.
- **Not only code.** Story, specification, decision-record and exploratory-testing work get
  the same standards and evidence, with [reader views](docs/ACCEPTANCE.md#reader-views) for
  QA, product and architecture.

Every hook output has a switch; see [routing](docs/ROUTING.md#project-standards).

## Evidence and claims

| What Craft has | What it records |
|---|---|
| A test command ran and its summary shows executed tests with no failures | `passed`, observed |
| The exit code or summary shows a failure, or no test executed (filter matched nothing, all skipped) | `failed`, observed |
| A test command ran but the output has no summary Craft recognizes | `unknown`, never `passed` |
| No test run was observed on the current files | `not-run` |
| The agent says verification passed | the agent's statement, as stated |

Checks during editing only report. Two places can refuse an unsupported success claim:

- In a Craft task, a verification step that says "passed" is refused while the latest
  deciding run on the current files failed. With no run observed, or only `unknown` ones,
  the step is recorded as the agent states it: Craft has nothing to contradict it with.
- With an opt-in [acceptance contract](docs/ACCEPTANCE.md), completion is refused while a
  required criterion lacks current passing evidence. Asking for the observed run is refused
  when none was observed on the current files. Other evidence stays as the reporter
  assessed it: Craft checks its currency and consistency, not its truth.

## Without and with configuration

**Without configuration**, Craft supplies workflows and engineering guidance chosen by the
kind of work and the files involved, observes test runs, answers certain lookups and resumes
tasks after compaction. It does not know your architecture: nothing is checked against
modules or standards you have not declared, and Craft's default anchors stay off.

**A configured project** declares in `.craft/project.json` its modules and dependencies,
standards with scope and level, its own skills and anchors, and opt-ins such as
`anchors.defaults` and `routing.debtMarkers`. [Project configuration](docs/PROJECT-CONFIG.md)
lists every field with its default; [routing](docs/ROUTING.md) and
[integrations](docs/INTEGRATIONS.md) explain them.

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
macOS on Apple Silicon, Linux x86-64 and Windows x86-64. See [adapters](docs/ADAPTERS.md) for
what each host supports.

## What is shown and what is not yet

- **Shown by tests on every change:** each check, routing decision and hook response has
  deterministic fixtures, and the scenario above covers declared standards and anchors,
  routing, checks, test evidence, resume after compaction and `doctor`.
- **Measured:** the runtime cost of `craft` on Linux, macOS and Windows (see Performance).
  In short model probes on Codex, answers were equally correct with and without Craft. Craft was
  cheaper where a hook answered a lookup directly and cost extra turns where the model read
  guidance first; those reads have been cut since, and cost is watched as a guardrail.
- **Not yet shown:** that agents keep declared standards more often, or leave less debt, with
  Craft than without it. That is the goal and a hypothesis; the checks verify what they
  check, not that outcomes improve. Results will be published when measured, losses
  included ([evaluation](docs/EVALUATION.md)).

## What it does not do

`craft` never calls a model, switches models, starts workers, compacts your session or posts
to a code host. Standards checks report; they do not block tools. A held stop asks the agent
to continue once. The one hook that denies is craft-review's shell guard: with
`review.readOnly: true` it denies common publishing commands, and while the project
configuration is invalid it denies every shell command
([read-only sessions](docs/REVIEW.md#read-only-sessions)). Plans and recommendations are
advice; your host's permissions and your authorization still decide what happens. Nothing is
sent remotely unless you configure an OpenTelemetry endpoint; bounded local usage metadata is
on by default and can be disabled ([observability and privacy](docs/OBSERVABILITY.md)).

## Direction

- **Verified standards:** more of the project's own standards checked without a model,
  including the project's own check commands.
- **The consumer decides:** organization and user layers above the project, with precedence
  that stays inspectable.
- **Trust and less debt, cost as a guardrail:** Craft should earn its place through standards
  kept and debt avoided, without costing more where it adds nothing.
- **Project Intelligence and continuity:** more structural answers without exploration, and
  task state that survives compaction and handoffs ([sessions](docs/SESSION.md)).

## Performance

Every push to `main` measures the installed release `craft` on Linux, macOS and Windows:
startup, hooks and Project Intelligence refresh. Any branch can be measured against `main` by
manual dispatch.
See the [dashboard](https://bencher.dev/perf/aliaksie-s-craft-capabilities) and
[contributing](docs/CONTRIBUTING.md#performance) for what is measured.

## Learn more

- [Documentation index](docs/README.md)
- [Project standards, anchors and skills](docs/ROUTING.md#project-standards)
- [Sessions, tasks and continuity](docs/SESSION.md)
- [Acceptance contracts](docs/ACCEPTANCE.md)
- [Commands](docs/COMMANDS.md)
- [Architecture](docs/ARCHITECTURE.md)
- [Contributing](docs/CONTRIBUTING.md)
- [Changelog](CHANGELOG.md)

Licensed under [MIT](LICENSE).
