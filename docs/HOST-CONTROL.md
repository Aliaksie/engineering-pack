# Host controls

Worker delegation, review and compaction are native phases of the bundled `craft` executable.
Each phase is one JSON request on standard input to `<package-root>/bin/craft` (`bin\craft.cmd`
on Windows) with no arguments:

```json
{"formatVersion": 1, "operation": "workers", "input": {"root": "/repo", "session": "s1", "task": "T1", "phase": "status"}}
```

The response is `{"formatVersion": 1, "ok": true, "result": …}` or `{"ok": false, "error":
{"code", "message"}}` with exit status 1. Worker and action phases answer `result: {value}` or
`result: {error}`; `craft` posts any configured remote telemetry itself after writing the
response. Requests are bounded to 3 MiB.

No package ships a host transport. The integration that owns the host client starts, observes
and cancels native work and calls these phases between its own callbacks. Project JSON cannot
load an integration or grant permission, and CLI recommendations remain advice. Model switching
remains a host or manual operation.

An integration must verify real conversation or standing authorization, native capability and
current policy before each effect. It must enforce native capacity and permission atomically at
execution, since another task can consume capacity after an observation, and enforce the
read-only scope and exact model it was given. Route nested workers through the same
root/session/task; native calls outside these phases are outside their enforcement. No full
conversation is passed to workers. Phases never run while a host callback is pending, and no
lock spans one.

## Workers

Requests use `operation: "workers"` with `root`, `session`, `task`, optional `preferencesFile`
and `phase`:

| Phase | Input | Result |
|---|---|---|
| `open` | — | `{context: {tree, root, session, task}}` |
| `prepare` | `input`, `inspect` | Validates the assignment; with `inspect: true` also `{targets, existing}` |
| `admit` | `input`, `targets`, `snapshot` | `{request}` reserved for dispatch, or `{duplicate: true}` |
| `record` | `id`, `result`, optional `generation` | `{request, executionObserved, modelMatches}` |
| `inventory` | optional `id` | `{targets, request}` |
| `reconcile` | `targets`, `snapshot` | Current status after applying the observation |
| `cancel-check` | `request`, `snapshot` | The occupied request a cancellation may target |
| `status` | — | `{tree, requests, limits}`; CLI `worker-status` shows the same evidence |

An assignment is `{id, offer, scope, parent?, reuseWorker?, brief}`; `brief` contains `goal`,
`sources`, `acceptance`, `restrictions` and `returnShape`. Copy it before awaiting
authorization. The offer must have a current recorded decision, matching source, policy and
scope, and a configured host/model mapping. Parent assignment IDs belong to the same ledger.
Reusing an observed idle worker is possible at full capacity; a zero or reduced ceiling stops
new assignments without cancelling workers.

Dispatch is `prepare` → authorize → observe → `admit` → the native start, using the admitted
request's exact `key`, `model`, `role`, `profile`, `source` and `taskContract` → `record` with
`{kind: "observed", workerId, model, status, occupiesSlot, evidence}` and the request's
`generation`. If the start outcome is unknown, record `{kind: "uncertain"}` instead.
Cancellation is `inventory` → `cancel-check` → authorize → `record {kind: "cancelling"}`,
which returns the `cancelKey` → the native cancel → `record` of the observation with the
cancelling `generation`.

A snapshot is `{host, capacity, occupiedSlots, taskWorkers, allowed, sessionId, observedAt,
source, workers, requests}`. Total occupied slots include main and other tasks; `workers` must
cover this complete task tree. Entries are `{id, status, occupiesSlot, model, evidence}`.
Snapshots expire after 30 seconds. Status may be `running`, `idle`, `completed`, `failed`,
`cancelled`, `released`, `not-started` or `unknown`; running and idle require occupied slots.
Completion, failure or cancellation alone never implies release.

Operation lookups in `requests` use the original dispatch key or cancellation key:
`{key, status: "started", workerId, settled: true, evidence}` must identify a worker in the
returned inventory. A negative lookup is `{key, status: "not-started", fenced: true, evidence}`:
the integration must prevent any delayed native execution of that key. Absence from an
inventory or a transient "not found" is insufficient. A slot-release observation must likewise
mean the operation is settled and cannot later acquire a slot.

The ledger persists intent before native calls, with one lock across processes, and
conservatively counts unacknowledged reservations. Duplicate assignment IDs never redispatch;
changed content under the same ID is rejected. Lost acknowledgements retain capacity until
`reconcile` obtains authoritative evidence. Generation checks keep late results from
overwriting newer observations. Integrations must preserve operation keys across crashes and
make native effects idempotent or provide a durable lookup or fence. There is no automatic
retry or time-based release.

Each tree retains at most 64 assignment records in 256 KiB; reaching the bound rejects new
work instead of deleting unresolved evidence. Native model mismatches stay visible and
occupied. Worker lifecycle never completes main-task acceptance.

New action offers bind the task-contract digest as well as source and effective policy.
Scope, risk or engineering-contract amendments invalidate the old offer even without a source
edit. Historical offers without this binding cannot authorize a structured task. Worker status
projects each request's original `taskContract` and `freshness` (`current`, `stale` or
`unknown`) without rewriting the lifecycle receipt, so a completed worker may have stale
evidence and still occupy a slot. Reconcile native effects separately from verifying the
amended task's acceptance. Bound worker requests use worker-ledger format 2 so their contract
survives offer eviction; readers retain version-1 requests with unknown bindings. Never
downgrade the ledger to resume an older writer.

## Review and compaction

Requests use `operation: "actions"` with `root`, `session`, `task`, optional `preferencesFile`
and `bundle`, and `phase`:

| Phase | Input | Result |
|---|---|---|
| `find` | `id` | The offer, including any stored `executionReceipt` |
| `claim` | `id`, `observation` | `{receipt, offer}`, or `{duplicate: true, receipt}` |
| `finish` | `id`, `receipt`, `result` | `{executed, uncertain, receipt}` |

Claim only an approved review or compaction. `observation` is `{sessionId, observedAt, facts}`
using the action preflight facts from [session controls](SESSION.md), observed within the last
30 seconds. The claim rechecks the active task, source, policy and prerequisites under the
binding lock and persists a single attempt; compaction saves the current checkpoint first. The
integration then executes outside every lock with the receipt's `key`, `source` and
`checkpoint` and the offer's `action` and `scope`.

Results use `{state, evidence, source}`, where state is `started`, `completed`, `not-started` or
`uncertain`. Negative outcomes require `fenced: true`. Completed compaction additionally
requires the receipt's `checkpoint`, `resumed: true` and a real `acceptanceEvidence`
reference. These are host observations, not semantic judgments by the runtime. Review
completion leaves delivery acceptance separate.

A malformed or lost result stays uncertain: finish with `{state: "uncertain"}`, and if that
cannot be persisted either, keep the receipt key for recovery. A repeated claim returns the
stored receipt; it never repeats a native action. To reconcile, `find` the stored receipt and
finish it with a lookup of the original key, source and checkpoint. A lookup must use durable
native receipts or a fence, not inference from an absent response.

## Ownership and admission

Requests use `operation: "control"`. Every phase except `support` takes `root`, `session` and
`task`; journal phases (`open`, `begin`, `persist`, `close`) also take `host` and `coordinator`.

| Phase | Input | Result |
|---|---|---|
| `support` | `host` | The host's feature and execution support table |
| `source` | `source` | Fails unless the task is active and the source unchanged |
| `admission-acquire` | `pid`, optional `source` | An opaque admission ticket |
| `admission-check` | ticket, optional `source` | Rechecks the binding while the ticket is held |
| `admission-release` | ticket | Releases the binding fence |
| `open` | `pid` | `{context, ticket, revision, journal}` |
| `begin` | ticket, `expected`, `input`, `kind`, optional `nativeId` | `{revision, journal}` with a new intent |
| `persist` | ticket, `expected`, `journal`, optional `withdrawn` | `{revision, journal}` |
| `close` | ticket | Releases journal ownership |

An admission ticket holds the active-task binding fence across an asynchronous native effect,
so task-start cannot switch tasks underneath it. Release it on every outcome.

The journal records each native operation as intent before its effect. `begin` takes
`{key, source?, checkpoint?, releaseWorker?}` and a kind of `worker`, `cancel`, `review`,
`compact` or `release`; a key is attempted at most once. `persist` may advance phases and
record native identities but cannot replace ownership, erase an attempted effect, append an
unadmitted operation or change its source or checkpoint; it may withdraw only a release intent
whose effect was observed not to be submitted. Every write requires the current `revision`. A
journal holds at most 192 operations and evicts none.

Tickets record the integration's own pid. A recognized lease is reclaimed only when its owner
process is observed dead; live, unknown, malformed and legacy ownerless locks remain busy.
Reclaiming never replays an operation: unresolved intent stays for the next owner to
reconcile. Integrations hold transport observations and tickets; they do not open state files
or clean up locks.

## Verification

Each `SETUP.json` and adapter report describes feature support, activation requirements and
evidence limits. Repository tests drive every phase with a simulated host; they do not
establish an installed integration, its authorization or model round trips. Exercise those in
an isolated installation and retain the resulting evidence.
