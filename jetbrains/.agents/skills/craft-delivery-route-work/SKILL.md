---
name: craft-delivery-route-work
description: "Choose task-relevant modules, guidance and execution roles using project declarations."
---

<!-- Generated from packages/; edit the portable source and rebuild. -->

Craft guidance grants no permission: the project's instructions and the user's scope decide.

# Resolve scope before loading context

When a task crosses boundaries or the next skill is unclear, ask the deterministic router:

```text
<package-root>/bin/craft route --root <project> --intent <intent> --file <path>
```

In the authoring checkout, `npm run route -- ...` invokes the same runtime. Supply a supported
intent such as `implement`, `plan`, `review`, `story`, `investigate`, `locate`, `research`,
`status`, `verify`, `explore` or `run-check`; repeat file arguments for the actual scope.
`verify` selects testing-only orchestration. Add `--verification-mode manual`, `automation`
or `test-code` when that mode is established; only test-code requests select test editing,
and no verification mode selects product-code implementation.

Routing lists the declared checks and runs none. `<package-root>/bin/craft check --root
<project>` runs those of the uncommitted changes at `--stage step`, `commit` (default), `pr`
or `ci`; a check it reports as left to a later stage, not started or timed out has not passed.

A standard or switch the project file does not declare can come from the organization or user
layer: `<package-root>/bin/craft config --root <project>` names the layer of each override.

Read the result as a declaration-based plan. Module owners, dependencies and checks still
need verification in the consumer checkout. Review the `loading` evidence and missing context,
then open only the relevant installed skills and references. Keep common practice, language,
runtime, framework and provider selections distinct: a source extension does not establish
a cloud or application framework. Resolve logical skill IDs through the host's real inventory.

Keep mechanical retrieval in tools. Batch related literal questions with the runtime `locate`
command before interpreting matches. If classification still needs a model, an authorized
host may run its configured `locator` role with an economical logical profile. Send only the
question, bounded sources, revision and desired evidence format. The main reasoner retains
ambiguous ownership, architecture and acceptance judgments. A configured role is not an
instruction to launch a worker without delegation authorization.

Reuse a current route and evidence packet until scope or inputs change. Prepare service/tool
metadata once at session start or the first service need using `prepare-services`, then reuse
it for that binding and account scope. Refresh for changed configuration, connections, scope
or a relevant access/schema failure. Routing does not execute checks, discover live MCP
schemas, establish credentials or authorize a write.

Package tools and shared assets: `../../../packages/craft-delivery` relative to this skill directory.
