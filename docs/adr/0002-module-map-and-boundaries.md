# ADR 0002: Module map and deterministic boundary rules

Status: Accepted — 2026-10-24

## Context

[ADR 0001](0001-rust-core-runtime.md) makes Rust the owner of the runtime. The repository
still mixes the target Rust crates with retiring JavaScript layers, and nothing stopped a new
top-level folder, a new cross-module import or new runtime JavaScript from appearing. Shape
tests protected individual paths but not the map as a whole.

## Decision

The repository has one declared module map. `architecture.json` is its machine form; the table
below is the accepted decision. `npm run architecture` (part of `npm run check`) fails when they
disagree or when the tree breaks a rule.

Final target: three Rust crates (`engine` ← `platform` ← `application`, with `application` also
using `engine`), `hosts/` as the only runtime JavaScript, and build tooling, content, schemas,
docs, tests and examples around them. `cli/` and `integrations/` dissolve into their owners;
the JavaScript inside the crate folders is legacy and retires by wave W4 of the migration.

## Module map

| Module | Kind | Depends on | Legacy JS depends on | Shipped | Responsibility |
|---|---|---|---|---|---|
| `engine/` | rust-crate | — | — | yes | Pure decisions, contracts, identities and transitions |
| `platform/` | rust-crate | engine | — | yes | Filesystem, state, locks, Git, processes, search, SQLite, HTTP, syntax providers |
| `application/` | rust-crate | engine, platform | engine, integrations, platform | yes | Use cases, CLI, host-neutral lifecycle handling, `craft` executable |
| `cli/` | retiring | application, platform | — | yes | Legacy command parsing and display, moving into `application` |
| `integrations/` | retiring | — | — | yes | Legacy external mappings, dissolving into `application`, `platform` and review owners |
| `hosts/` | host-adapter | application, platform | — | yes | Host payloads, native invocation, host wire output; no dependencies after migration |
| `packages/` | content | application | — | yes | Capability guidance and thin package entry points |
| `tools/` | optional-tool | application, integrations | — | no | Optional review inbox; owner undecided |
| `tooling/` | build-tooling | * | — | no | Build, export, release, validation, benchmarks, eval drivers |
| `schemas/` | schema | — | — | yes | Published authoring schemas |
| `docs/` | docs | — | — | yes | Maintained documentation |
| `tests/` | tests | * | — | no | Repository behavior, replay and architecture tests |
| `examples/` | examples | * | — | no | Consumer skeletons and eval fixtures |
| `.github/` | ci-config | * | — | no | CI workflows and benchmark harnesses |

`*` means build-time code that may read anything; no shipped module may depend on it.

## Rules

| ID | Rule |
|---|---|
| A1 | Every tracked or new untracked top-level path is declared (module or `rootFiles`) |
| A2 | Files match the module's `allow` shapes; legacy JavaScript only where `legacy.allow` permits |
| A3 | Kinds come from the closed list above; responsibilities are one sentence; this table equals `architecture.json` |
| A4 | Cargo workspace members are exactly the `rust-crate` modules; path dependencies follow `dependsOn`; external crates are declared; workspace lints and `unsafe_code = "forbid"` apply |
| A5 | `craft-engine` production code uses no filesystem, process, network, environment or ambient clock |
| A6 | Cross-module JavaScript imports follow `dependsOn` (or `legacy.dependsOn`) and target declared entry points; unused entry points are removed |
| A7 | Runtime JavaScript is reachable from a declared runtime root or a build-time consumer; unreachable code is deleted |
| A8 | Runtime JavaScript per module never exceeds its line budget, and budgets are lowered as code is removed. After migration, host adapters import no other module |
| A9 | Rust modules are named for their domain: no `utils`, `common`, `helpers`, `misc`, `shared`, `core`; no empty modules |
| A10 | Each host adapter file stays within the module's line limit |
| A11 | Exports contain no module declared as not shipped |

A budget rises only for the JavaScript forwarder that moves an entry point into `craft`: one
native request per invocation plus handing native results back to the remaining JS (for example,
telemetry export until bounded HTTP is native). Each such raise is recorded with its slice and is
recovered when the forwarded module retires. Behavior never moves into JavaScript this way.

W1h forwards `route` while its existing session binding and activity remain with the W2 owner. CLI transport adds one line
(337→338); application telemetry transport adds five lines (1320→1325) to carry the existing
command trace and pseudonym across the process boundary. Both retire with their JS owners.

W1i moves bounded discovery into Rust. Its temporary session bridge resolves task scope and
validates the session observation. The native writer uses the shared lock and writes only if
that observation is still current; a concurrent receipt is preserved with an explicit diagnostic.
The bridge retires when W2 takes session validation and binding.

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
the same change in reverse; mark it `retiring` first when its contents move over time.

## Consequences

Structure changes become explicit, reviewed edits to two files instead of side effects. The JS
budgets make the migration's progress visible and stop runtime JavaScript from growing. Entry
point and root lists are verbose during migration; they shrink with each wave and disappear with
the legacy code. A new runtime entry point or shipped file type needs a manifest update, which is
intended friction.
