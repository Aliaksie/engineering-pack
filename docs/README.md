# Documentation

New here? Start with [installation](INSTALL.md), then [commands](COMMANDS.md). To configure a
project, read [routing](ROUTING.md) and [integrations](INTEGRATIONS.md). Contributors start
with [contributing](CONTRIBUTING.md) and [architecture](ARCHITECTURE.md).

## Using it

| Document | Purpose |
|---|---|
| [Installation](INSTALL.md) | Channels, per-host installation, first use, updates and local builds |
| [Commands](COMMANDS.md) | Every `craft` command, its purpose and effects |
| [Routing](ROUTING.md) | Module and guidance selection, lookup, execution roles and model advice |
| [Sessions](SESSION.md) | Task lifecycle, checkpoints, state storage, reuse and session advice |
| [Acceptance contracts](ACCEPTANCE.md) | Criteria, evidence, coverage and reader views |
| [Review](REVIEW.md) | Revision-bound local review and bot integration |
| [Shipping](../packages/craft-delivery/workflows/ship-change.md) | Authorized commit, push, install, deploy, publish and license phases |
| [Integrations](INTEGRATIONS.md) | Git, codehost and tracker bindings |
| [Project evidence](PROJECT-EVIDENCE.md) | Opt-in local index of declarations and imports |
| [Observability](OBSERVABILITY.md) | Local usage events, privacy, retention and OTLP opt-in |
| [Security](SECURITY.md) | Trust and execution boundaries, read-only review guard |

## Extending it

| Document | Purpose |
|---|---|
| [Architecture](ARCHITECTURE.md) | Components, interaction and constraints |
| [Rust core runtime](adr/0001-rust-core-runtime.md) | Why the runtime is one native executable |
| [Module map](adr/0002-module-map-and-boundaries.md) | Declared modules and the rules the checker enforces |
| [Adapters](ADAPTERS.md) | Export layouts, hook support and context injection per host |
| [Host controls](HOST-CONTROL.md) | Native worker, review and compaction phases for integrations |
| [Package format](PACKAGE-FORMAT.md) | Manifest, skills, workflows, tools, hooks and MCP |
| [Playbook catalog](PLAYBOOKS.md) | Logical identities, selection rules and shared engineering concepts |
| [Naming](NAMING.md) | Package, skill, workflow and configuration naming |
| [Evaluation](EVALUATION.md) | Package scenarios, paired runs and limits on model-quality claims |
| [Contributing](CONTRIBUTING.md) | Prerequisites, local checks, CI and diagnostics |
| [Releasing](RELEASING.md) | Channels, publication and versioned releases |
