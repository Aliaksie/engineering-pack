# Craft Capabilities

This repository authors portable AI Capability Packages. Canonical content lives in
`packages/`; development-host packaging belongs to adapters. Node 20+, ESM, no npm dependencies.

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
| `npm run route -- ...` | Resolve modules, skills and execution without running work |
| `npm run locate -- ...` | Batch bounded literal searches into compact source evidence |
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
| `runtime` | Deterministic routing, tool plans and opt-in checkpoint state |
| `tools/craft-review-inbox` | Optional personal GitHub inbox; excluded from default exports |
| `adapters` | Host payloads, manifests, discovery paths, hook output and feature reports |
| `scripts` | Authoring validation, builds and eval-result checks |
| `examples/project` | Consumer configuration skeleton; no live connections |
| `schemas` | Published local authoring schema; runtime validator is dependency-free |
| `dist` | Generated, ignored platform exports; never edit by hand |

## Rules

- Preserve project-specific configuration in the consumer. Do not assume a language, UI
  framework, cloud, tracker, repository layout or architecture.
- Load common principles, language, application layer and provider guidance independently.
  Examples in a language do not make its compiler rules universal.
- Keep vendor model aliases, permission grants and cache paths out of shared instructions.
- Shared hooks accept normalized input and return allow/deny/context data. Adapters serialize host
  responses. Unsupported enforcement must be visible in the adapter report.
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
