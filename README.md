# Craft Capabilities

Portable engineering playbooks and deterministic tools for investigating, planning,
implementing, verifying and reviewing software changes with a coding agent. Choose a workflow,
load the relevant guidance, and keep decisions grounded in the target project's code and policy.
Exports are available for Claude Code, Codex, Cursor, Copilot, Gemini CLI and AGENTS/Agent Skills.

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

## Quick start

From a source checkout, use Node 20+ and Git. There are no npm dependencies to install:

```text
npm run check
```

This validates the source, builds all six exports under `dist/`, and runs the tests.
Use `npm run build` when you only need to regenerate the bundles.

For Claude Code, add the generated marketplace and install a package:

```text
claude plugin marketplace add ./dist/claude
claude plugin install craft-engineering@craft-capabilities
```

In a Claude Code session in your target project, invoke `/craft-engineering:foundations`
with a concrete task and the relevant files. Install other packages from the table above
when you need their workflows. The package READMEs describe their entry points.

For Codex, Cursor, Copilot, Gemini or a plain Agent Skills host, follow
[installation by host](docs/ADAPTERS.md#installation-by-host). Building creates files;
installation, hook trust and optional service connections are separate steps.
The source repository URL itself is not a Claude marketplace because `dist/` is generated
and excluded from Git.

## Try the tools without installing a host bundle

Run these from the source checkout:

```text
npm run route -- --root examples/project --intent review --file services/orders/Order.java
npm run locate -- --root . --path runtime --term planOperation --max-matches 5
```

The first returns a loading plan for the example module: guidance, owner, checks and execution
role. The second returns bounded source matches with paths, lines and snippets. Neither runs
a model or contacts a service. The [example configuration](examples/project/README.md)
contains illustrative modules and tool names; it is not a running application.

## Route work and resolve tools

```sh
npm run operation -- --root examples/project --operation task.read --params '{"id":"TEAM-123"}'
```

This returns an explicit invocation plan for the example task tool; it does not execute it.
The JSON quoting shown is for a POSIX shell; adapt it to your shell. In an exported standalone package the CLI is at
`support/runtime/cli.mjs`; a full bundle also has `runtime/cli.mjs`.

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
indexes the full design. The initial version stays **0.0.1**.

## Contributing and optional tools

Canonical content lives in `packages/`; shared runtime code lives in `runtime/`; native host
packaging belongs in `adapters/`. Read [contributing](docs/CONTRIBUTING.md) for authoring checks
and [the architecture](docs/ARCHITECTURE.md) for these boundaries. The
[logical catalog](docs/PLAYBOOKS.md) explains selective loading.

GitHub Actions checks Linux, macOS and Windows and uploads generated bundles after the
matrix succeeds. See [sharing and releasing](docs/RELEASING.md) for artifact access.

The optional [personal review inbox](tools/craft-review-inbox/README.md) provides a GitHub
watch list, local review state and posted-text evidence. It is excluded from default exports.

## License

[MIT](LICENSE).
