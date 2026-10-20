# Architecture

Capability content lives in `packages/`. Four installation units cover engineering,
delivery, review and product stories. Languages and application/provider disciplines
remain independently selected guidance within craft-engineering.

```mermaid
flowchart TD
  CLI[cli/ and hosts/] --> APP[application/: use cases]
  APP --> ENGINE[engine/: portable JSON decisions]
  APP --> PORTS[platform/: runtime ports]
  PORTS --> NODE[platform/node: filesystem / Git / search / state]
  APP --> EXT[integrations/: external transports]
  BUILD[tooling/: build and export] --> CONTENT[packages/: guidance plus bundled runtime]
```

## Ownership

| Location | Owns |
|---|---|
| `engine/contracts/` | Project/catalog contracts and portable path validation |
| `engine/routing/` | Guidance selection, module/delegation decisions, generic operation plans |
| `engine/discovery/` | Independent lookup batches, bounded matching and compact results |
| `engine/cache/` | Reuse validity and deterministic identities |
| `engine/session/` | Context estimates, occupancy recommendations, activity and attempt budgets |
| `engine/policy/` | Model advice, context delivery, review and inbox decisions |
| `engine/checkpoint.mjs` | Small continuation contracts, source binding and comparison |
| `application/` | Observation collection and execution of concrete local use cases |
| `application/{routing,discovery,session,context-delivery}/` | Related use cases grouped by feature |
| `platform/index.mjs` | Wiring for bounded observation/storage operations |
| `platform/node/` | Concrete Node ports; repository and resource observations are grouped separately |
| `integrations/` | External Git/codehost command syntax, observation normalization and output projections |
| `hosts/` | Native host payloads, telemetry, capability declarations, controls and wire output |
| `cli/` | Arguments, help and text display |
| `tooling/export/` | Build-time platform exporters |

Folders represent features with multiple responsibilities. Small features remain files.
Top-level names identify ownership: `engine/` decides, `application/` orchestrates,
`platform/` implements runtime/OS access, `hosts/` adapts coding-agent hosts,
`integrations/` maps external systems, `cli/` handles human input/output,
`tooling/` builds and validates, and `packages/` contains agent-facing guidance.
No former runtime, adapter or script paths are retained as forwarding modules.

## Engine contract

`engine/index.mjs` exports `evaluate(request)`. Requests and responses contain only JSON:

```json
{"formatVersion":1,"operation":"attempt-status","input":{"operation":"implement","records":[]}}
```

Success returns `{ "formatVersion": 1, "ok": true, "result": ... }`. Failure returns
`ok: false` with a stable `error.code` (`UNSUPPORTED_VERSION`, `UNKNOWN_OPERATION`, or
`INVALID_INPUT`) and a diagnostic message. Callers must not branch on message prose.
Inputs are copied before evaluation; no caller-owned objects are mutated. Clocks,
repository versions, file observations, host capabilities and catalog data are explicit
inputs. Time-sensitive advice requires an explicit clock value.

The engine imports only its own portable modules. It does not read files, invoke Git,
use Node globals, inspect environment variables or interpret native host payloads.
Application use cases call the public boundary through `application/engine-client.mjs`,
which only serializes requests and unwraps responses. Catalog loading and provider facts
belong to the relevant use cases;
Node implementations do not import application logic or engine policy. Application code
imports only `platform/index.mjs`, never `platform/node/` details. The platform entry point
wires the current Node implementation without introducing a second infrastructure layer. Port callbacks stay within application/Node orchestration; they never cross the JSON engine boundary.

A future Rust implementation can replace `evaluate` while retaining observation collection,
CLI output and host adapters. No native bridge, process protocol or second implementation
is required now. JavaScript remains a supported long-term implementation. Contract replay
and dependency tests enforce the useful boundary independently of implementation language.

## Evidence and effects

Pack context measures selected guidance; current occupancy requires fresh, session-bound
host observations. Pack activity counts its own receipts, never provider usage. Missing or
incomplete observations stay unknown. See [session behavior](SESSION.md).

Discovery batches only known independent literal lookups. Cached evidence is bound to
session/task, HEAD, scoped inventory/file versions, manifests/config and implementation
identity. Unavailable evidence triggers fresh discovery. Hint reuse separately records only
successfully emitted context; lifecycle invalidation survives an in-flight delivery.

Checkpoint, activity and hint state share bounded, symlink-checked, atomic file storage,
with separate formats and lifecycles. Automatic Git snapshots cannot renew the source
binding of older continuation notes. A reset recommendation requires a fresh continuation.

Worker reservations use one bounded private ledger per coordinator root/session/task across
processes. Application controllers persist intent and release locks before calling trusted
host functions; the engine decides admission and reconciliation from explicit JSON facts.
Review/compaction execution receipts live with their existing action proposals and survive
proposal cleanup. No executable adapter is loaded from project configuration. See the
[host controller contract](HOST-CONTROL.md) for operation keys, fencing and native limits.

The engine returns plans and advice. It does not call models, switch profiles, execute
remote operations, compact a host session or publish findings. Context-delivery policy
receives lifecycle facts, not hook event names. Application code
translates lifecycle events and collects delivery evidence; hosts parse and run native hooks.
Host lifecycle controls belong to `hosts/`; external service mappings belong to
`integrations/`. Neither grants permission. Unsupported host enforcement and observations remain visible in reports.

## Packages and exports

Package names, manifests and useful bin/hook entry points remain external interfaces.
The builder copies canonical `engine/`, `application/`, `platform/`, `integrations/` and `cli/` into
each package's `support/`, and rebases shared imports in package entry points. Installed
packages run after relocation without authoring scripts or sibling package dependencies.
Full bundles also expose these directories at their root. No legacy runtime paths or
fallback loaders are retained. Generated host glue remains in `platform-runtime/`;
that name describes an installed host interface rather than the old source tree.

The optional inbox remains under `tools/craft-review-inbox/` because it owns separate
consumer configuration, remote pagination, checkout and review-artifact lifecycles. Its
shared status and authorization decisions now use the engine, and external command
recognition uses integrations. It runs from the repository checkout and stays excluded
from default exports. Its separate I/O is retained rather than merging remote inbox state
with session checkpoint state.

Builds validate source, stage exports and refuse unmanaged or symlinked output. The canonical pack version is `package.json.version`; package and runtime
metadata are synchronized projections checked before every build. Deterministic checks establish packaging and contract correctness; they do
not establish model quality or an efficiency improvement.

## Supporting telemetry

`application/telemetry/events.mjs` defines a bounded JSON envelope and metadata allowlists.
Application use cases emit at operation boundaries. `platform/index.mjs` exposes local
storage, scoped identity, version, clock and bounded HTTP ports; their Node implementations
remain under `platform/node/`. `integrations/otel/` maps sanitized envelopes to OTLP JSON.
The engine imports neither telemetry runtime nor providers. Provider objects never cross
its JSON boundary or enter the application event contract. No separate top-level telemetry
subsystem or event bus is needed. See [observability and privacy](OBSERVABILITY.md).

Telemetry trace scopes use public platform context ports; Node implements async context
isolation in its existing telemetry module. The portable event/context contract remains
plain JSON. Application boundaries own spans and reason codes; transport mappings own
OTLP serialization. See the [observability coverage matrix](OBSERVABILITY.md#coverage-and-deliberate-limits)
for diagnostics, bounded loss behavior and the zero-model-cost contract.
