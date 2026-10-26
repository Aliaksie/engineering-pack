# ADR 0002: Module map and deterministic boundary rules

Status: Accepted — 2026-10-24

## Context

[ADR 0001](0001-rust-core-runtime.md) makes Rust the owner of the runtime. Without a declared
map, nothing stops a new top-level folder, a new cross-module import or new runtime JavaScript
from appearing; shape tests protect individual paths but not the map as a whole.

## Decision

The repository has one declared module map. `architecture.json` is its machine form; the table
below is the accepted decision. `npm run architecture` (part of `npm run check`) fails when they
disagree or when the tree breaks a rule.

The runtime is three Rust crates (`engine` ← `platform` ← `application`, with `application`
also using `engine`). No runtime JavaScript ships: `hosts/` holds the host tables that the build
reads and `craft` embeds, and package hook files only name a native handler. Build tooling,
content, schemas, docs, tests and examples sit around the crates.

## Module map

| Module | Kind | Depends on | Shipped | Responsibility |
|---|---|---|---|---|
| `engine/` | rust-crate | — | yes | Pure decisions, contracts, identities and transitions; the routing catalog |
| `platform/` | rust-crate | engine | yes | Filesystem, state, locks, Git, processes, search, SQLite, HTTP, syntax providers |
| `application/` | rust-crate | engine, platform | yes | Use cases, CLI, host-neutral lifecycle handling, `craft` executable |
| `hosts/` | host-adapter | — | no | Host hook mapping, session and feature capability tables read by the build and embedded in `craft` |
| `packages/` | content | — | yes | Capability guidance and hook declarations |
| `tooling/` | build-tooling | * | no | Build, export, release, validation, benchmarks, eval drivers |
| `schemas/` | schema | — | yes | Published authoring schemas |
| `docs/` | docs | — | yes | Maintained documentation |
| `tests/` | tests | * | no | Repository behavior, replay and architecture tests |
| `examples/` | examples | * | no | Consumer skeletons and eval fixtures |
| `.github/` | ci-config | * | no | CI workflows and benchmark harnesses |

`*` means build-time code that may read anything; no shipped module may depend on it.

## Rules

| ID | Rule |
|---|---|
| A1 | Every tracked or new untracked top-level path is declared (module or `rootFiles`) |
| A2 | Files match the module's `allow` shapes |
| A3 | Kinds come from the closed list above; responsibilities are one sentence; this table equals `architecture.json` |
| A4 | Cargo workspace members are exactly the `rust-crate` modules; path dependencies follow `dependsOn`; external crates are declared; workspace lints and `unsafe_code = "forbid"` apply |
| A5 | `craft-engine` production code uses no filesystem, process, network, environment or ambient clock |
| A6 | Cross-module JavaScript imports follow `dependsOn` and target declared entry points; unused entry points are removed |
| A7 | Runtime JavaScript is reachable from a declared runtime root or a build-time consumer; unreachable code is deleted |
| A8 | JavaScript per module never exceeds its line budget, and budgets are lowered as code is removed; host adapters import no other module |
| A9 | Rust modules are named for their domain: no `utils`, `common`, `helpers`, `misc`, `shared`, `core`; no empty modules |
| A10 | Each host adapter file stays within the module's line limit |
| A11 | Exports contain no module declared as not shipped |

A budget never rises to move behavior into JavaScript; new runtime behavior belongs in `craft`.

Installed commands run `<package-root>/bin/craft` (`bin\craft.cmd` on Windows); session binding,
task state and activity are read and written natively. Discovery writes its reuse receipt under
the shared lock only if the state it observed is still current; a concurrent receipt is
preserved with a diagnostic.

A host hook runs `craft hook <host> <lifecycle>` through a generated launcher that only selects
the bundled executable: `bin/craft` for POSIX shells, including Git Bash on Windows, and
`bin/craft.cmd` for cmd and PowerShell. `craft` reads the host payload (64 KiB bound), derives
host-neutral lifecycle facts with the embedded host mapping table `hosts/hooks.json`, which the
build also reads, writes the host wire output and posts any remote telemetry export. Only
shell-scoped hooks evaluate the command text and only delivery hooks the edited file and prompt.
Without a bundled runtime for the platform, the launcher answers as a failed runtime would: a
shell enforcement hook denies, observation stays silent and delivery reports its context as
unavailable. Executable checksums are verified when bundles are assembled, not per hook call.

Worker, review and compaction controls are native phases (`workers`, `actions`, `control`),
each one JSON request to `craft`. The host transport that starts, observes and cancels native
work belongs to an integration the consumer owns; no package ships one. Rust owns the worker
ledger, action claims and receipts, and shares the binding lock with task-start during
admission; transport callbacks run between phases, outside every lock. `craft statusline
<host>` reads the host's status-line payload and writes the line; the per-host session
capability table is data in `hosts/`, read by the build and embedded in `craft`.

Native transport journals and asynchronous task admission use retained native leases.
Rust records the calling host pid and an opaque ticket on the shared lock protocol;
ordinary state writers reclaim a recognized lease only after observing its owner dead.
Live, unknown, malformed and legacy ownerless locks remain busy. Native journal phases
validate the bounded record and its revision before writing. Integrations hold transport
observations and tickets; they do not open state files or own lock cleanup. Host feature
support is a single data table read by the builder and embedded in the native engine.

The checker reads only the working tree, Git's file list and the manifest. It needs no network
or toolchain beyond Node and Git. Each rule has a negative fixture in
`tests/architecture-rules.test.mjs`.

## Adding a top-level module

Add one only if all of these hold:

1. It owns a responsibility no existing module owns, stated in one sentence.
2. It differs from every existing module in toolchain or language, shipping unit, or position in
   the dependency graph.
3. It cannot be a submodule of an existing module without breaking a rule above.

A new Rust crate must also name the dependency rule it enforces that a module inside an existing
crate cannot (ADR 0001: crates only for useful boundaries).

Steps: amend this ADR's table and rationale, add the `architecture.json` entry, add a negative
fixture if the change introduces a rule, and pass `npm run architecture`. Removing a module is
the same change in reverse.

## Consequences

Structure changes become explicit, reviewed edits to two files instead of side effects. The JS
budgets stop JavaScript from growing back. A new runtime entry point or shipped file type needs
a manifest update, which is intended friction.
