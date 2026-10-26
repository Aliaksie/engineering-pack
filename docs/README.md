# Documentation

To install a built bundle, start with [adapters](ADAPTERS.md#installation-by-host).
To configure a target project, read [routing](ROUTING.md) and [integrations](INTEGRATIONS.md).
Contributors can start with [contributing](CONTRIBUTING.md) and [package format](PACKAGE-FORMAT.md).

| Document | Purpose |
|---|---|
| [Architecture](ARCHITECTURE.md) | Engine, application, platform, host, integration and tooling ownership |
| [Rust core runtime decision](adr/0001-rust-core-runtime.md) | Runtime language ownership, incremental migration and structural simplification |
| [Project evidence preview](PROJECT-EVIDENCE.md) | Native development commands, evidence semantics and current limits |
| [Naming](NAMING.md) | Repository, package, skill, workflow and tool conventions |
| [Engineering guidance](ENGINEERING-GUIDANCE.md) | Shared concepts and language/runtime specialization |
| [Playbook catalog](PLAYBOOKS.md) | Logical identities and selection rules |
| [Session status](SESSION.md) | Task lifecycle, context, activity and execution budgets |
| [Acceptance contracts](ACCEPTANCE.md) | Opt-in portable context, criterion/evidence validation and coverage limits |
| [Host controls](HOST-CONTROL.md) | Native worker, review and compaction phases and limits |
| [Routing](ROUTING.md) | Module/skill selection, execution roles and checkpoints |
| [Integrations](INTEGRATIONS.md) | Git, codehost and tracker bindings |
| [Shipping](../packages/craft-delivery/workflows/ship-change.md) | Authorized commit, push, install, deploy, publish and license phases |
| [Review](REVIEW.md) | Revision-bound local review and bot integration |
| [Package format](PACKAGE-FORMAT.md) | Manifest, skills, workflows, tools, hooks and MCP |
| [Adapters](ADAPTERS.md) | Host feature support, event mapping and installation limits |
| [Evals](EVALS.md) | Shared scenarios and recorded evidence |
| [Evaluation protocol](EVALUATION.md) | Matched baseline/candidate runs and limits on model-quality claims |
| [Tool and hook audit](authoring/TOOLS.md) | Deterministic tool effects, guard limitations and measurement scope |
| [Contributing](CONTRIBUTING.md) | Authoring and local checks |
| [Observability](OBSERVABILITY.md) | Local usage events, privacy, retention and OTLP opt-in |
| [Security](SECURITY.md) | Trust and execution boundaries |
| [Releasing](RELEASING.md) | Sharing requirements and output layout |

[Target platform installation and support](PLATFORMS.md) covers Codex, Claude, Gemini and JetBrains.
