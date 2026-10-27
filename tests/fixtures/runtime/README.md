# Frozen runtime contracts

`engine-contracts.json` records requests and responses from the JavaScript engine
at `09d74ead129008575f8483f68bd5412d2627c839`. The source was verified unchanged
against that revision when capturing the fixtures. Expected responses are static;
do not regenerate them from a replacement implementation.

Investigate suspicious baseline behavior and fix confirmed defects with regression
coverage before migration. For an intentional contract correction, update the affected
fixture explicitly and explain the correction; frozen output does not justify a known bug.

`cargo test -p craft-engine --test engine_contracts` replays the corpus through the Rust
engine. Error codes, JSON results (including key order) are compared; diagnostic messages
are not. Validators compare their outcome only, because the JavaScript validators echoed
their input. Every case is replayed. Cases without a Rust counterpart were removed: the
JavaScript dispatcher envelope and the hash of an `undefined` value, arguments that
dispatcher left `undefined`, the routing catalog as a request input (the catalog is embedded
now; the route and session corpora replay those decisions) and the services decision before
built-in mappings (`services-contracts.json` replays it with them). The acceptance-view and
feature-outcome cases were removed as copies of `view-contracts.json` and
`feature-contracts.json`.

The corpus holds empty-input requests, UTF-8 hashes, digest ordering, routing,
continuation, blocked attempts, checkpoint binding, context selection and budgets,
repository scopes and snapshots, settings, model advice and service plans over explicit
observations and a fixed clock. Empty-input rejection is not successful
operation coverage; each use case still needs its own success, state and installed checks.

`cli-contracts.json` freezes 25 executable cases after verifying CLI, application,
platform, engine and review executable sources unchanged against the same baseline.
Run `npm run build` then `node --test tests/runtime-cli-replay.test.mjs` to replay
source and relocated AGENTS exports from disposable working directories with isolated
user configuration. These cover the command catalog, help precedence, repeated Unicode
file arguments, JSON/verbose routing, parser and required-input failures and disabled
Project Intelligence with an unavailable binary. Compare exit
status, stdout/stderr separation, structured results and native error codes; recorded
diagnostic prose is evidence only. These checks do not establish signal cancellation,
enabled native execution, automatic host hook dispatch or other platform support.

`host-contracts.json` freezes 120 host translation cases from the same baseline,
after verifying hosts and their runtime dependencies unchanged. The native hook
translation replays them (`cargo test -p craft-application frozen_host_contracts`). Cases cover five hook-capable hosts and the guidance-only
AGENTS host, all seven lifecycle events, deny/context support, native response
schemas, Unicode, string tool arguments, alias precedence, invalid arguments and
session bounds. Optional undefined event fields compare as absent JSON fields.
These fixtures protect the thin adapter boundary; they do not establish automatic
hook dispatch, full application lifecycle, cancellation or native platform support.

`state-contracts.json` fixes four JSON byte representations verified against the
unchanged baseline state-file implementation. `cargo test -p craft-platform state::`
replays them on disposable state. The scenarios cover exact UTF-8 limits,
null and Unicode, key order, pretty printing, invalid existing records, exclusive
and unresolved locks, interrupted atomic publication, and routing generation
companion migration with source-change detection. Failure checks preserve existing
bytes and verify temporary-file cleanup. These checks complement private-state
integration tests; they do not establish crash durability, full payload schemas,
concurrent-process safety or acceptance on other operating systems.

`settings-contracts.json` freezes 133 JavaScript engine responses for `effective-settings`,
`validate-project` and `worker-budget`, captured at `470b316` with error messages omitted.
They cover precedence and origins, every project validation family, preference bounds,
null coalescing, integral floats, and worker freshness at a fixed clock. The Rust
engine replays this corpus (`cargo test -p craft-engine frozen_settings_cases_replay`).

`services-contracts.json` freezes 63 JavaScript responses for the `operation` and
`prepare-services` use cases at `7352220`: effective settings from a project, built-in
CLI mappings and the engine decision, with error messages omitted. The Rust replay lists
seven intentional corrections by case ID: built-in arguments are not substituted again,
placeholders inside text take only scalar parameters, a change number is a string or safe
integer, and built-in mappings come only from the operation's own service.

`models-contracts.json` freezes 83 JavaScript responses for the `model-advice` use case at
`dc5c447`: effective settings from a project and the engine decision over the request, with
error messages omitted. The Rust engine replays every case unchanged.

`lookup-contracts.json` freezes 42 JavaScript responses for the `lookup` use case at
`dc47bf2`, including the three non-empty `lookup` cases retired from `engine-contracts.json`
with the JavaScript engine operation. The Rust replay lists three intentional corrections:
an empty module name and a module selector combined with a file now fail instead of listing
every module or silently ignoring the file.

`route-contracts.json` freezes 120 JavaScript responses for the `route` use case at
`173123c`: effective settings, then the application route over CLI options, with error
messages omitted. The Rust engine replays every case unchanged, including key order.

`locate-contracts.json` freezes 66 bounded literal search cases at `b27ec99` and
`discovery-contracts.json` freezes 92 discovery planning, reuse and result cases at `266b8d1`,
including the JavaScript engine discovery fixtures they retired.

`context-contracts.json` freezes 54 JavaScript responses for the `context-report` use case at
`20ab5c9`. Each case stores its bundle trees as file maps (`{repeat, count}` for large files)
and indexes the supplied roots; physical roots appear as `<bundle:N>`. The Rust replay rebuilds
the trees and runs the `craft context-report` command, comparing output bytes and the session
activity counters.

`checkpoint-contracts.json` freezes 1765 JavaScript engine responses at `c27951a` for
repository scopes/snapshots/checkouts, checkpoint metadata, records, comparison and change
detection, session-state validation, activity counters, task binding/contract/source/context
and acceptance coverage. Valid metadata, engineering contracts, evidence ledgers and session
states are mutated field by field; a mutated case stores `{$mutate: {base, at, value|remove}}`
against `documents`, and validator responses omit their echoed input. `acceptance:` reasons
are compared exactly; other error prose is not. The Rust replay
(`cargo test -p craft-engine --test checkpoint_contracts`) lists five intentional
corrections: impossible calendar dates such as `2026-02-30`, which `Date.parse` rolled over,
are rejected in acceptance observation times and feature outcome times.

`telemetry-contracts.json` freezes 51 JavaScript `telemetry-report`, `telemetry-capture` and
`telemetry-forget` invocations at `888dfea` over eight seeded stores: empty and keyless projects,
valid and invalid envelopes of every reported feature, loss diagnostics, attribution, malformed,
unsupported, oversized and non-UTF-8 lines, a backup file, session filtering, capture
deduplication, capacity, explicit forgetting, invalid keys and an oversized log. Receipts
use `<ROOT>`, `<W>` (worktree state directory) and `<T>` (capture time); `@N` is the Nth capture
ID returned in a case. The Rust replay (`cargo test -p craft-application --test
telemetry_contracts`) has one intentional correction: an event dated `2026-02-30`, which
`Date.parse` rolled over, is an invalid record.

`attempt-contracts.json` freezes 185 JavaScript engine responses at `6864ab2` for attempt
receipts and appends: every field of a valid attempt mutated, repository source states, and
deterministic append walks whose every step stores the saved attempt IDs and counters or the
refusal (resolution rules, open-attempt capacity, closed-history eviction, counter overflow).
It includes the seven `attempt-receipt`/`attempt-append` cases retired from
`engine-contracts.json`. The Rust replay (`cargo test -p craft-engine --test attempt_contracts`)
has no corrections.

`task-contracts.json` freezes 235 JavaScript engine `task-start` responses at `7d8a0ac`: input
validation of every field, new and repeated tasks, every change dimension (stage, files, risks,
repositories, engineering contract, checkout) with its acceptance reset, and the returned intent
and task context. It includes the two legacy lifecycle starts used by
`engine/tests/acceptance_contract.rs` and the five `task-start` cases retired from
`engine-contracts.json`. Results compare as serialized JSON, so key order is contractual, and
error messages compare exactly. The Rust replay (`cargo test -p craft-engine --test task_contracts`)
lists one message correction: a non-string task file is reported as JSON rather than
`[object Object]`.

`session-contracts.json` freezes 430 JavaScript engine responses at `72f1303` for session
advice (input, policy and usage validation, freshness, pressure thresholds, recommendation
order, notices), attempt status, feature reports, capability classes, the status observation
plan and unified status over explicit observations (activity, checkpoint freshness, guidance,
current profile, model advice and escalation, blockers, recorded offers). Effective settings
are stored once per name. Results compare as serialized JSON and error messages exactly. The
Rust replay (`cargo test -p craft-engine --test session_contracts`) lists two corrections: a
previous notice time without `Z` or an offset, which JavaScript read in the machine's time zone,
is invalid; and a stage without a guidance observation, which the application never produces,
reports a diagnostic instead of a JavaScript `TypeError`.

`step-contracts.json` freezes 208 JavaScript engine responses at `8ca3cc4` for the `task-step`
receipt (input validation, every outcome, verification, completion with and without an
engineering contract, structured evidence replacement, continuation, attempt capacity) and
the action offers: fact validation, review, compact and delegate eligibility, offer identity,
suppression and capacity eviction, explicit decisions and readiness checks. It includes the
legacy lifecycle step used by `engine/tests/acceptance_contract.rs` and the recommendation-retention
case formerly in `action-execution.test.mjs`, and absorbs the nine `task-step`, `action-propose`
and `action-decide` cases retired from `engine-contracts.json`. Results, including the updated state, compare as
serialized JSON and error messages exactly. The Rust replay
(`cargo test -p craft-engine --test step_contracts`) has no corrections.

`review-tool-contracts.json` freezes the JavaScript review tools at `a84479e`: 25
`prepare-review` runs over a repository built from recorded steps with fixed identities and
dates (merge-base ranges, remote identities, excluded run state, binary, renamed, non-ASCII
and spaced paths, unresolvable references, missing merge base, argument and existing-output
failures), 70 `check-review` validations and 12 publication plans. The native
`review-prepare`, `review-check` and `review-publication` commands replay it
(`cargo test -p craft-application --test review_tools`), comparing exit status, artifacts,
mode, stdout and validation messages; usage, Git and JSON parser prose are classes, not text.
Two cases are corrections: a remote change ID must be a digit string or a safe positive
integer, where JavaScript accepted `[7]` and `1e20` after `String()` conversion. It covers
the nine review cases retired from `engine-contracts.json` and the three review cases
retired from `cli-contracts.json`.

`statusline-contracts.json` freezes the JavaScript status-line entry (`support/session.mjs`)
at `d9c34b6`: 108 runs of the Claude, Codex and AGENTS exports over Claude status-line
payloads (usage counts, capacity, model and session identities, size and JSON failures),
declared task data, argument failures and the recorded telemetry. `craft statusline`
replays it (`cargo test -p craft-application --test statusline`), comparing exit status,
stdout and telemetry without the nested command envelope; usage, missing-file and JSON
parser prose are classes. Corrections: a Claude read with an empty payload reports what an
empty declaration reports (JavaScript failed to parse it, and waited on a terminal); a
non-object payload, declaration or declared host is refused (JavaScript accepted arrays
and scalars, spread a string host into indexed fields and threw a `TypeError` on a `null`
payload); a `null` declared host counts as absent, as before.

`review-contracts.json` freezes the JavaScript read-only review guard at `0ecc73d`: 439 shell
command lines with whether they publish (direct commands, quoting, separators, Git global
options, `gh`/`glab` request actions and API writes, `-c` shell scripts, substitutions and the
16-level nesting bound) and 32 policy decisions over `review.readOnly`, tool and command shape.
It includes the commands of the former JavaScript policy unit tests and covers the two retired
`review-publication-policy` cases from `engine-contracts.json`. The Rust replay
(`cargo test -p craft-engine --test review_contracts`) has no corrections.

`followup-contracts.json` freezes the JavaScript review inbox status derivation at `563d7a3`
over 185 stateless GitHub pull request observations (no stored record or local draft):
contribution kinds, later commits, replies, empty and commented approvals, requested changes,
staleness boundaries, participating and resolved threads, summaries, ties, deleted accounts,
pending reviews, own and draft pull requests. Field names follow the native result. The
native `review-followup` check replays it (`cargo test -p craft-engine --test
followup_contracts`). Eight cases are corrections: bot comments and reviews are not replies;
a head other than the one in the reviewer's latest review needs verification whatever the
commit date says; an unreadable time, a truncated connection or a head mismatch leaves the
pull request undecided instead of being ignored; a pull request that is no longer open is
`closed`. It covers the `inbox-status` and `inbox-policy` cases retired from
`engine-contracts.json`; the inbox command guard they served is gone.

`view-contracts.json` freezes the JavaScript acceptance views (`qa-projection`,
`product-projection`, `architecture-projection`) at `c632948` over 157 inputs: the manual-QA
and product-views examples, a two-criterion contract, every execution/support combination,
binding changes, blockers, probe and focus validation branches, size and count bounds, and
the representative engine cases. Inputs are the operation input objects; `craft
acceptance-view` reads the same documents from files. The Rust replay (`cargo test -p
craft-engine --test view_contracts`) compares results as serialized JSON and reasons exactly.
Three cases are corrections: a QA view without a contract reports `qa:missing-contract`
instead of `qa:invalid-probes`. It includes the six projection cases from `engine-contracts.json`.
