# Craft Capabilities

Engineering practices and workflows for AI coding agents, together with your project's own
rules and skills, installed into Claude Code, Codex, Gemini, Cursor, Copilot, JetBrains AI
Assistant or any Agent Skills host from one source.

Agents and hosts change; your engineering standards should not. Craft ships useful defaults,
lets the project declare what "good" means for it, brings the right part to the agent when
the work reaches it, and checks what can be checked. A small native tool, `craft`, makes
those decisions without calling a model, so the result is the same on every host and every run.

## Why it exists

An agent starts every session from general knowledge. Your commit format, architecture
rules, ADR template and review checklist live in documents it may not read, or read once
and lose after compaction. "Tests pass" is often a claim, not an observed run. Writing a
plugin, hooks and skills for each host to fix that is infrastructure most teams should not
have to maintain.

Craft takes the approach Spring Boot takes for applications: working conventions by default,
customization where your project differs.

- **Defaults.** Engineering guidance and workflows for planning, implementation, testing,
  review, stories and shipping. Each task gets only the guidance its declared stack and kind
  of work need, not everything at once.
- **Your rules and skills.** `.craft/project.json` declares your standards (a file, a folder
  or an established name such as `conventional-commits`), where they apply and how strictly,
  and your own skills. On conflict, yours win; `craft standards` and the routes show which
  source supplied each piece of guidance.
- **Evidence instead of claims.** Checks that need no model run on what the agent writes.
  Test runs are read from the runner's own summary. What was not checked is reported as
  unknown, never as passed.

It is not only for code. Story, specification, decision-record and exploratory-testing work
get the same standards, checks and evidence, with reader views for QA, product and
architecture.

## How it helps

A project declares its modules and a few standards:

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

**With Craft**:

- The route for the change names the standards in its scope, one line each; their content
  stays in your project.
- When the agent writes a domain file that adds an import from `src/adapters`, the same
  tool result says the domain module does not declare that dependency. Nothing is blocked.
- The commit message is checked against Conventional Commits in the commit's tool result.
- A test command that ran no test is recorded as failed, and in a Craft task a verification
  step that claims a pass is refused while the latest run on the current files failed.
- When the turn ends, the agent gets one summary for the changed scope: the migration
  checklist if a migration changed, dependency findings and, if enabled, new TODOs, lint
  suppressions or skipped tests.
- After compaction, the hard rules of the session's scope come back with the work.

The same declarations reach subagents the host starts and Craft's worker briefs. Every hook
output has a switch; see [routing](docs/ROUTING.md#project-standards).

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

2. Declare one standard in `.craft/project.json`:

   ```json
   {
     "formatVersion": 1,
     "standards": [
       { "id": "commits", "anchor": "conventional-commits" },
       { "id": "api", "file": "docs/api-style.md", "summary": "REST naming and error envelope" }
     ]
   }
   ```

   `craft standards` lists what the project declares. `craft standards --commit-message <file>`
   runs the same check as the hooks and exits non-zero when it is not met, so a Git
   `commit-msg` hook or CI can use it too.

3. Give the agent a real task through a workflow, for example in Claude Code:

   ```text
   /craft-delivery:workflow-deliver-change Fix the rounding error in the order total
   ```

   Codex and other hosts use the package-prefixed name, such as
   `$craft-delivery-workflow-deliver-change`.

No configuration is required to start. Modules, anchors, your own skills and tracker or
code-host bindings are described in [routing](docs/ROUTING.md) and
[integrations](docs/INTEGRATIONS.md).

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

Claude Code and Codex are tested end to end; the other hosts get the same declarations
through their own hook and skill support, as far as each host allows. The native executable
is bundled for macOS on Apple Silicon, Linux x86-64 and Windows x86-64. See
[adapters](docs/ADAPTERS.md) for what each host supports.

## What is shown and what is not yet

- **Shown by tests on every change:** each check, routing decision and hook response has
  deterministic fixtures, and one scenario takes a task through every standards feature on
  Claude Code and Codex: declared standards and anchors, routing, checks, test evidence,
  resume after compaction and `doctor`.
- **Measured:** the runtime cost of `craft` on Linux, macOS and Windows (see Performance).
  In short model probes on Codex, answers were equally correct with and without Craft. Craft was
  cheaper where a hook answered a lookup directly and cost extra turns where the model read
  guidance first; those reads have been cut since, and cost is watched as a guardrail.
- **Not yet shown:** that agents keep declared standards more often, or leave less debt, with
  Craft than without it. That is the goal and a hypothesis; the checks above verify what
  they check, not that outcomes improve. Results will be published when measured, losses
  included ([evaluation](docs/EVALUATION.md)).

## What it does not do

`craft` never calls a model, switches models, starts workers, compacts your session or posts
to a code host. Checks report; they do not block tools. Plans and recommendations are advice;
your host's permissions and your authorization still decide what happens. Nothing is sent
remotely unless you configure an OpenTelemetry endpoint; bounded local usage metadata is on
by default and can be disabled ([observability and privacy](docs/OBSERVABILITY.md)).

## Direction

- **Verified standards:** the project's own standards, kept and visibly so. More of them
  checked without a model, including the project's own check commands.
- **The consumer decides:** your declarations win on conflict, and precedence is inspectable.
  Next: organization and user layers above the project.
- **Trust and less debt, cost as a guardrail:** Craft should earn its place through standards
  kept and debt avoided, without costing more where it adds nothing.
- **Project Intelligence:** a local index of declarations, callers, imports, document
  sections and configuration keys that lookups answer from, on by default
  ([project evidence](docs/PROJECT-EVIDENCE.md)).
- **Continuity:** task state, decisions and hard rules that survive compaction and handoffs.

## Performance

Every push to `main` measures the installed release `craft` on Linux, macOS and Windows:
startup, hooks and Project Intelligence refresh. Any branch can be measured against `main` by
manual dispatch.
See the [dashboard](https://bencher.dev/perf/aliaksie-s-craft-capabilities) and
[contributing](docs/CONTRIBUTING.md#performance) for what is measured.

## Learn more

- [Documentation index](docs/README.md)
- [Project standards, anchors and skills](docs/ROUTING.md#project-standards)
- [Commands](docs/COMMANDS.md)
- [Architecture](docs/ARCHITECTURE.md)
- [Contributing](docs/CONTRIBUTING.md)
- [Changelog](CHANGELOG.md)

Licensed under [MIT](LICENSE).
