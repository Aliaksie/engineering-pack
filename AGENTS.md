# Craft Capabilities

This repository authors portable AI Capability Packages. Canonical content lives in
`packages/`; the runtime is the Rust `craft` executable (`engine`, `platform`, `application`);
host tables live in `hosts/` and export construction in `tooling/export/`. Tooling and tests
are Node ESM with no npm dependencies: Node 22+ minimum, Node 24 for development and primary
CI, Node 26 for forward checks. Rust 1.89+ builds the runtime.

## Commands

| Command | Purpose |
|---|---|
| `npm run build` | Export Claude, Codex, Cursor, Copilot, Gemini and AGENTS bundles into dist |
| `npm run check` | Validate source, craft blocks, exports, script syntax and tests |
| `npm test` | Build exports, then run Node tests |
| `npm run validate` | Validate capability manifests and referenced source files |
| `npm run validate:exports` | Check generated Claude marketplace and references |
| `npm run architecture` | Check the declared module map, dependency rules and runtime JS budgets |
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
| `npm run acceptance:view -- ...` | Show the QA, product or architecture view of a contract and evidence ledger |
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
| `engine` | Rust crate: pure decisions over explicit facts (routing and its catalog, discovery, sessions, budgets, policy) |
| `application` | Rust crate: the `craft` executable and use-case orchestration over engine and platform |
| `platform` | Rust crate: filesystem, state, Git, processes, search and bounded HTTP |
| `hosts` | Host hook mapping, session and feature tables read by the build and embedded in `craft` |
| `tooling` | Build, export, validation, release and evaluation tooling |
| `examples/project` | Consumer configuration skeleton; no live connections |
| `schemas` | Published local authoring schema; runtime validator is dependency-free |
| `dist` | Generated, ignored platform exports; never edit by hand |

## Rules

- Runtime behavior belongs in Rust: engine decisions, Project Intelligence, state, routing,
  telemetry, context, cache and persistence. JavaScript is for build tooling, tests, build-time
  host tables and hook declarations; never add runtime logic to it. See
  [ADR 0001](docs/adr/0001-rust-core-runtime.md).

- Top-level modules, their kinds and dependencies are declared in `architecture.json` and
  [ADR 0002](docs/adr/0002-module-map-and-boundaries.md). Change both together; never add a
  top-level folder, cross-module import or runtime JS budget increase without them.

- Investigate suspicious behavior rather than preserving it. Fix confirmed defects with
  regression coverage. State formats, digest identities and error codes are compatibility
  contracts: document intentional corrections and update the affected fixtures explicitly.

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
- `craft hook` translates host payloads into lifecycle facts, the engine returns portable
  decisions and `craft` writes the host's native response. Unsupported enforcement stays
  visible in export reports.
- MCP declarations are opt-in and secret-free. Builds never activate connections or tools.
- Keep packages self-contained. No runtime imports from authoring scripts or sibling packages.
  The builder bundles the native runtime into each export; keep one canonical source.
- Use logical `package:skill` references; adapters translate names for the host.
- Keep the current version (`package.json`) until a release/version change is requested.
  Build adapter manifests instead of maintaining version copies by hand.
- Test meaningful behavior and portability, not exact prose. Evals need actual model evidence
  before reporting semantic success. Do not fabricate a pass from static checks.
- Respect user authorization. Nothing in a package grants permission to post, deploy or publish.

See [docs/README.md](docs/README.md) for format, adapters, evals and security.
