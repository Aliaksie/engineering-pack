# Deterministic tools and hooks

Tools use dependency-free Node ESM. Git and `gh` are explicit prerequisites for operations
that need them. A routing or publication plan grants no permission to execute it.

## Runtime and review tools

| Tool | Input and result | Effects |
|---|---|---|
| Router and catalog | Intent, paths, declarations and risks → explained loading plan | Reads configuration; does not execute tasks or contact services |
| Batched locator | Independent literals/scopes → compact path/line/snippet evidence; dependent requests stay deferred | Bounded local inventory/scan; optional session/task-scoped reuse and receipts; no model/network |
| Pack context report | Routed guidance selection → paths, hashes, bytes and estimated tokens | Reads selected package resources; report-only budgets; never infers live occupancy |
| Session status/advice | Explicit task state, host observations and saved receipts → next-action advice | Reads bounded local state and needed repository/resource evidence; no compaction, reset or model call |
| Attempt receipt | Bounded operation/target/approach/outcome/evidence → retry and escalation history | Explicit local state write and bounded Git reads; no retry or model switch |
| Service preparation | Configured bindings and optional host-observed tool names → compact operation map | Pure metadata preparation; no discovery/probes, credentials, persistent cache or invocation |
| Catalog validator | Catalog and optional target set → validated contract or error | Pure validation; source validation checks registered files |
| Service bindings | Operation, parameters and binding → CLI argv or MCP plan | No subprocess or network call |
| Checkpoints | Session metadata and Git state → saved state or comparison | Local Git reads and atomic state writes; opt-in, locked and size-bounded |
| Review snapshot | Explicit committed base/head → immutable identities and diff | Bounded local Git reads; excludes working-tree changes |
| Review result checker | Snapshot and result → structural verdict | Reads artifacts; does not judge findings or publish them |

`cli/main.mjs` exposes routing, lookup, service plans, context reporting, session status/advice,
attempt receipts and checkpoint actions. Installed packages expose `support/cli/main.mjs`.
`engine/` owns portable decisions; application use cases orchestrate `platform/` operations.
`hosts/` owns native callbacks and `tooling/export/` constructs installed bundles.
`prepare-review.mjs` writes a new snapshot artifact exclusively; `check-review.mjs` validates
recorded results. No tool in this table posts a review. Checkpoints preserve explicit state,
not fresh verification or proof of task completion.
Recovery compares bounded working-tree content and index fingerprints as well as HEAD and
status. `changeDetection.complete` reports inspection coverage; `changed: null` means that
unchanged state could not be established. File/byte/time limits, failed reads, submodules and
legacy checkpoints can leave coverage incomplete. Consumers must not treat unknown as unchanged.
Snapshot diffs compare the merge base to the requested head. Result checks enforce matching
base/head identities, changed-file membership and required fields, but do not establish that
a line exists or that a finding is correct. Working-tree review needs direct inspection;
snapshot tooling must not be represented as covering uncommitted edits.

## Hooks and enforcement limits

Delivery hooks supply compact hints for explicit prompt routing; edit hints require the
consumer's `routing.editHints` opt-in. Session-start hooks can prepare configured service
metadata without live discovery. Optional
checkpoint events save explicit metadata and flag changed trees after recovery. Host adapters
report whether each mapped event can inject context or deny a tool call. Observing an event
does not imply enforcement. Installation enables neither delegation nor checkpoint writes.

The read-only review guard recognizes simple direct publishing commands, quoted arguments,
command separators and Git global options. It is a limited guardrail. Indirect execution
such as `sh -c`, command substitution, aliases, executable paths and shell-specific expansion
are outside its recognizer. Use host tool permissions and isolation for actual read-only
enforcement. Malformed-input and unsupported-host behavior must remain visible in adapters.

## Optional review inbox

`tools/craft-review-inbox/bin/inbox.mjs` provides config, doctor, scan, tick, render, checkout, mark
and posted-text recording commands. It is excluded from default exports. JSON configuration
contains explicit identities and repository bindings; credentials stay in the consumer's tools.
Remote calls are reads through Git and `gh`; local writer actions require explicit invocation.

Local writes use a lock, reject symlinked destinations and atomically replace each file.
Multi-file updates are not transactional. Repository pagination completes before closure
inference; incomplete scans do not advance freshness. Checkouts use fresh scratch paths and
verify the requested head. Reviewed events require a matching head; actual posted text comes
from file/stdin rather than being assumed equal to a draft.

## Verification and diagnostics

Tests cover argument boundaries, pagination, partial failures, staleness, locks, Unicode/CRLF,
working directories and relocated exports. They establish deterministic behavior on the
runner, not model benefit or live-host compatibility.

`npm run hooks:report -- --output <new-file>` checks exported source hashes, samples a fresh
Node wrapper process and records a bounded hand-labeled policy corpus. It does not execute
the corpus commands or install hooks. Exit 0 means report collection succeeded, even if
policy cases expose misses. Process wall time includes startup and serialization, excluding
host dispatch, model calls and network. Neither sample latency nor corpus counts establish
production rates. CI uploads runner-specific reports; hosted/native-host verification and
representative production measurements remain pending until actual evidence is collected.
