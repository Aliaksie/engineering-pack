# Craft Capabilities

This repository authors portable AI Capability Packages. Canonical content lives in
`packages/`; native host translation belongs to `hosts/` and export construction to `tooling/export/`. Node 22+ minimum; Node 24 for development and primary CI, Node 26 for forward checks. ESM, no npm dependencies.

## Commands

| Command | Purpose |
|---|---|
| `npm run build` | Export Claude, Codex, Cursor, Copilot, Gemini and AGENTS bundles into dist |
| `npm run check` | Validate source, craft blocks, exports, script syntax and tests |
| `npm test` | Build exports, then run Node tests |
| `npm run validate` | Validate capability manifests and referenced source files |
| `npm run validate:exports` | Check generated Claude marketplace and references |
| `npm run validate:claude` | Official Claude validator on the built marketplace |
| `npm run sync:craft-bar` | Regenerate craft reference blocks from their canonical standard |
| `npm run eval:report -- <result.json>` | Check recorded eval evidence; does not run or judge a model |
| `npm run model:advice -- ...` | Recommend configured profiles from current-target evidence without execution |
| `npm run route -- ...` | Resolve modules, skills and execution without running work |
| `npm run locate -- ...` | Batch bounded literal searches into compact source evidence |
| `npm run context:report -- ...` | Measure selected guidance files; live session token usage stays unknown |
| `npm run session -- ...` | Show compact session/context/activity status; `--json` exposes evidence |
| `npm run session:attempt -- ...` | Record one bounded acceptance attempt without executing a retry |
| `npm run session:advice -- ...` | Recommend a next action from explicit task state and fresh host observations |
| `npm run eval:pair -- ...` | Prepare or explicitly run one frozen baseline/candidate pair; does not judge quality |
| `npm run services:prepare -- ...` | Prepare configured service/tool metadata without live discovery |
| `npm run operation -- ...` | Resolve a service operation into a CLI/MCP invocation plan |
| `npm run checkpoint:save -- ...` | Save explicit task metadata and Git state |
| `npm run checkpoint:read -- ...` | Compare saved checkpoint with current Git state |

## Module boundaries

| Path | Responsibility |
|---|---|
| `packages/<name>/capability.json` | Versioned local authoring contract; optional integrations are explicit |
| `packages/<name>/AGENTS.md` | Shared package instructions |
| `packages/<name>/skills` | Portable Agent Skills and colocated references |
| `packages/<name>/workflows` | Named orchestration procedures exported as skills |
| `packages/<name>/standards` | Canonical quality guidance |
| `packages/<name>/evals` | Provider-independent scenarios and rubric criteria |
| `packages/craft-review/bin` | Revision-bound Git snapshots and review artifact validation |
| `engine` | Portable JSON decisions: routing, discovery, sessions, budgets and policy |
| `application` | Use-case orchestration; calls public engine and platform boundaries |
| `platform` | Runtime/OS implementations: Node filesystem, Git, state and processes |
| `integrations` | External Git/codehost/transport mappings |
| `cli` | Argument parsing and display |
| `tools/craft-review-inbox` | Optional personal GitHub inbox; excluded from default exports |
| `hosts` | Native coding-agent observations, controls and wire formats |
| `tooling` | Build, export, validation, release and evaluation tooling |
| `examples/project` | Consumer configuration skeleton; no live connections |
| `schemas` | Published local authoring schema; runtime validator is dependency-free |
| `dist` | Generated, ignored platform exports; never edit by hand |

## Rules

- The local Pack core runtime belongs in Rust: engine decisions, Project Intelligence,
  state, routing, telemetry, context, cache and persistence. Host-specific adapters and
  packaging may remain thin JS/TS where needed. New capabilities must not add substantial
  JS/TS runtime logic. Migrate legacy runtime components in verified, incremental slices
  of complete use cases; remove each replaced implementation after compatibility and
  installed checks pass. Simplify structure when justified by the changed behavior; preserve useful ownership boundaries and public contracts rather than copying
  JS layers one-for-one. See [ADR 0001](docs/adr/0001-rust-core-runtime.md).

- Before implementation edits, inspect Git state, preserve unrelated work and establish a safe
  change boundary under consumer branching/workspace policy and repository protection.
- Follow consumer commit policy and existing authorization; implementation alone does not
  authorize commits. Keep authorized commits coherent and verified, and internal plans/session
  records untracked and out of shipped references. Add prose only for an unmet reader need.

- Preserve project-specific configuration in the consumer. Do not assume a language, UI
  framework, cloud, tracker, repository layout or architecture.
- Load common principles, language, application layer and provider guidance independently.
  Examples in a language do not make its compiler rules universal.
- Keep vendor model aliases, permission grants and cache paths out of shared instructions.
- Application use cases translate host events into lifecycle facts. The engine returns portable
  decisions; hosts serialize native responses. Unsupported enforcement stays visible in export reports.
- MCP declarations are opt-in and secret-free. Builds never activate connections or tools.
- Keep packages self-contained. No runtime imports from authoring scripts or sibling packages. The builder copies the shared
  runtime into each export; keep one canonical source.
- Use logical `package:skill` references; adapters translate names for the host.
- Keep this initial iteration at version `0.0.1` until a release/version change is requested.
  Build adapter manifests instead of maintaining version copies by hand.
- Test meaningful behavior and portability, not exact prose. Evals need actual model evidence
  before reporting semantic success. Do not fabricate a pass from static checks.
- Respect user authorization. Nothing in a package grants permission to post, deploy or publish.

See [docs/README.md](docs/README.md) for format, adapters, evals and security.
