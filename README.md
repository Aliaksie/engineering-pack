# Engineering Pack

A portable set of engineering practices and workflows for reliable AI-assisted software
engineering. Agents can change; engineering standards should belong to the team and repository.

```text
Engineering practices → packages/ + portable engine/
                                ↓
                      hosts/ + tooling/export/
                  ┌─────────┼─────────┬───────────┐
                Codex     Claude    Gemini    JetBrains rules
                  └─────────┴─────────┘
                     external agents in IDEs
```

## Install

Use this repository's ready-built `distribution` branch. No source checkout or local build
is required. Bundled CLI commands and hooks require Node 20+ on PATH; Git operations also
require Git. These commands work after the first successful CI publication from
`main`; see [installation and updates](docs/PLATFORMS.md) for verification and limitations.

### Codex CLI

```sh
codex plugin marketplace add Aliaksie/engineering-pack --ref distribution
codex plugin add craft-engineering@engineering-pack
```

Install other packages from the same marketplace as needed; see [Codex setup](docs/PLATFORMS.md#codex-cli).

### Claude Code

```sh
claude plugin marketplace add Aliaksie/engineering-pack#distribution
claude plugin install craft-engineering@craft-capabilities
```

### Gemini CLI

```sh
gemini extensions install https://github.com/Aliaksie/engineering-pack --ref distribution
```

### JetBrains AI

Download the [distribution ZIP](https://github.com/Aliaksie/engineering-pack/archive/refs/heads/distribution.zip)
and follow the [project-rule setup](docs/PLATFORMS.md#jetbrains-native-ai-chat). External agents
inside JetBrains use their native integration above.

## Philosophy

Keep vendor-neutral knowledge in one core and platform packaging thin. Prefer deterministic
checks over prompt rules, repository evidence over generic instructions, and a small set of
composable skills over exhaustive micro-skills. Load language and framework references only
when relevant. Humans own engineering decisions and authorization; agents validate changes
and report evidence and limits. Bound review to the changed revision and material findings;
do not rerun AI review indefinitely looking for fresh issues.

## Adding a portable skill

Add `packages/<package>/skills/<name>/SKILL.md` with `name` and `description` front matter,
and register its directory in that package's `capability.json`. Describe the goal, trigger,
context, process, validation, expected output and failure/escalation conditions. Keep host
commands in adapters. Colocate detailed references only when useful; use `package:skill`
for logical references. Run `npm run check`; the existing builder exports every target.
No vendor copies need manual maintenance. See [contributing](docs/CONTRIBUTING.md).


## Portable core

This is an initial **0.0.1** release. Export layouts and deterministic behavior are tested
locally; live host loading and broad improvements in model output remain unverified.

| Package | Purpose |
|---|---|
| [craft-engineering](packages/craft-engineering/README.md) | Common principles, language discipline, UI, backend and agent engineering |
| [craft-delivery](packages/craft-delivery/README.md) | Planning, implementation, routing, service setup and shipping |
| [craft-review](packages/craft-review/README.md) | Revision-bound review for local diffs, GitHub PRs and GitLab MRs, including bots |
| [craft-stories](packages/craft-stories/README.md) | Code grounding, specification audits and user stories |

Start with the workflow you need. The shared router selects engineering practices and
language, runtime, framework, layer and provider guidance independently. Java does not imply
Spring; TypeScript does not imply Node.js. Story authoring can run without implementation
guidance. Installation does not load the whole collection into each request.

Contributors use Node 20+ and `npm run check`. Use `npm run build` to regenerate all eight bundles without running checks. Building does
not install plugins, trust hooks or connect services. For Cursor, Copilot and plain Agent
Skills hosts, see [additional adapters](docs/ADAPTERS.md#installation-by-host).

## Try the tools without installing a host bundle

Run these from the source checkout:

```text
npm run route -- --root examples/project --intent review --file services/orders/Order.java
npm run locate -- --root . --path engine/routing --term planOperation --max-matches 5
npm run context:report -- --root examples/project --intent review --file services/orders/Order.java
```

The first returns a loading plan for the example module: guidance, owner, checks and execution
role. The second returns bounded source matches with paths, lines and snippets. Neither runs
a model or contacts a service. The [example configuration](examples/project/README.md)
contains illustrative modules and tool names; it is not a running application.

The context report measures selected guidance files with disclosed token estimates; it does
not measure live session usage. For manual testing, route `--intent explore` or use
`craft-engineering:workflow-explore-testing` to prepare a charter or retain observed findings
without requiring source access. `verify` selects verification reasoning; `run-check` plans
declared module commands without claiming execution. See [routing](docs/ROUTING.md).

Optional [session status and advice](docs/SESSION.md) suggests next actions from explicit task
milestones and context recovery from fresh host observations. Native usage/session controls
remain primary; unknown usage stays unknown. One shared advisor ships with every adapter.

## Route work and resolve tools

```sh
npm run operation -- --root examples/project --operation task.read --params '{"id":"TEAM-123"}'
```

This returns an explicit invocation plan for the example task tool; it does not execute it.
The JSON quoting shown is for a POSIX shell; adapt it to your shell. In an exported standalone package the CLI is at
`support/cli/main.mjs`; a full bundle also has `cli/main.mjs`.

Consumer `.capabilities/project.json` configures modules, Git/codehost/task tools, execution
profiles and optional checkpoints. Declare runtime/framework/provider evidence independently
at project or module level; absent runtime evidence remains an explicit unknown. MCP is one
transport; it is not required. Local task text needs no tracker. Unknown integrations stay unknown rather than being guessed.

Use `npm run locate -- --root <project> --path src --term Name --term key` for bounded
batched lookup, and `npm run services:prepare -- --root <project>` for one service metadata
summary. Reuse scoped discovery evidence across stages; tool/model selection details are in
[routing](docs/ROUTING.md) and [integrations](docs/INTEGRATIONS.md).

Package validation and routing fixtures verify deterministic behavior. They do not prove
better model results. Broader paired workflow evaluation remains pending; see the
[evaluation protocol](docs/EVALUATION.md).

See [routing](docs/ROUTING.md), [integrations](docs/INTEGRATIONS.md),
[hook support](docs/ADAPTERS.md) and [review bots](docs/REVIEW.md).
[AGENTS.md](AGENTS.md) holds contributor instructions; [documentation](docs/README.md)
indexes the full design. The canonical pack version is **0.0.2**.

## Contributing and optional tools

Canonical content lives in `packages/`; portable decisions live in `engine/` and use cases
in `application/`. Filesystem, Git and state access live in `platform/`; external transport
mappings live in `integrations/`. CLI handling lives in `cli/`, native host translation
in `hosts/`, and build/export/validation/release tools in `tooling/`. Read [contributing](docs/CONTRIBUTING.md) for authoring checks
and [the architecture](docs/ARCHITECTURE.md) for these boundaries. The
[logical catalog](docs/PLAYBOOKS.md) explains selective loading.

GitHub Actions checks Linux, macOS and Windows and uploads generated bundles after the
matrix succeeds. See [sharing and releasing](docs/RELEASING.md) for artifact access.

The optional [personal review inbox](tools/craft-review-inbox/README.md) provides a GitHub
watch list, local review state and posted-text evidence. It is excluded from default exports.

Engineering Pack does not send telemetry remotely by default. Bounded local usage metadata
is enabled by default; see [observability and privacy](docs/OBSERVABILITY.md) to inspect
what is collected, disable it, or opt into OTLP export.

## License

[MIT](LICENSE).
