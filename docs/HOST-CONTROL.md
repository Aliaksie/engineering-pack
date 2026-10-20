# Trusted host controllers

The standalone runtime exports `application/session/workers.mjs` and
`application/session/actions.mjs` under each package's `support/` directory. These explicit
JavaScript APIs accept trusted adapter functions. Project JSON cannot load adapters or
grant permission. CLI recommendations remain advice; the shipped hooks supply no native
worker, review or compaction transport. Model switching remains a host/manual operation.

An adapter must verify real conversation/standing authorization, native capability and
current policy before effects. It must atomically enforce native capacity and permission
at execution, since another task can consume capacity after an observation. Route nested
workers through the same coordinator root/session/task; arbitrary native calls outside this
controller are outside its enforcement. No full conversation is passed to workers.

## Worker lifecycle

Construct `workerController(root, { session, task, preferencesFile }, host)` with:

| Callback | Contract |
|---|---|
| `authorize(context)` | Verify scoped dispatch/cancel authority; return `{ allowed, evidence }`. Dispatch requires a bounded evidence reference. |
| `observe(context)` | Return fresh matching host/session capacity, the complete task-tree worker inventory and settled operation lookups. |
| `dispatch(context)` | Use the exact supplied model/profile/role and bounded brief; return actual `{ workerId, model, status, occupiesSlot, evidence }`. |
| `cancel(context)` (optional) | Cancel the specified native worker using the supplied cancellation key; return an actual observation, including whether its slot remains occupied. |

Call `dispatch({ id, offer, scope, parent?, reuseWorker?, brief })`; `brief` contains `goal`,
`sources`, `acceptance`, `restrictions` and `returnShape`. The offer must have a current
recorded decision, matching source/policy/scope and a configured host/model mapping. Parent
assignment IDs belong to the same ledger. Reusing an observed idle worker is possible at
full capacity; a zero or reduced ceiling stops new assignments without cancelling workers.

`observe` receives `{ tree, root, session, task, requests }`. Return
`{ host, capacity, occupiedSlots, taskWorkers, allowed, sessionId, observedAt, source,
workers, requests }`. Total occupied slots include main and other tasks; `workers` must
cover this complete task tree. Entries are `{ id, status, occupiesSlot, model, evidence }`.
Snapshots expire after 30 seconds. Status may be `running`, `idle`, `completed`, `failed`,
`cancelled`, `released`, `not-started` or `unknown`; running/idle require occupied slots.
Completion/failure/cancellation alone never implies release.

Operation lookups use the original dispatch key or cancellation key:
`{ key, status: "started", workerId, settled: true, evidence }` must identify a worker in
the returned inventory. A negative lookup is `{ key, status: "not-started", fenced: true,
evidence }`: the adapter must prevent any delayed native execution of that key. Absence
from an inventory or a transient "not found" is insufficient. A slot-release observation
must likewise mean the operation is settled and cannot later acquire a slot.

The private ledger persists intent before native calls, with one lock across processes,
and conservatively counts unacknowledged reservations. Network calls occur outside the
lock. Duplicate assignment IDs never redispatch; changed content under the same ID is
rejected. Lost acknowledgements retain capacity until `reconcile()` obtains authoritative
evidence. Generation checks prevent late callbacks from overwriting newer observations.
Adapters must preserve operation keys across crashes and make native effects idempotent
or provide a durable lookup/fence. There is no automatic retry or time-based release.

`status()` and CLI `worker-status` expose recovery evidence. Each tree retains at most 64
assignment records in 256 KiB; reaching the bound rejects new work instead of deleting
unresolved evidence. Native model mismatches stay visible and occupied. Worker lifecycle
never completes main-task acceptance.

## Review and compaction

Construct `actionController(root, { session, task, preferencesFile, bundle }, host)` with
`authorize`, `observe` and `execute`; optional `lookup` reconciles an uncertain result.
`observe` returns `{ sessionId, observedAt, facts }` using the action preflight facts from
[session controls](SESSION.md). `authorize` must return `{ allowed: true }` only after
verifying the actual scope. `execute` receives `{ root, session, task, key, action, scope,
source, checkpoint }`. Call `execute(offerId)` only for an approved review or compaction.

The controller rechecks active task, source, policy and prerequisites, persists a single
attempt, then calls the adapter outside the lock. Compaction saves the current checkpoint
first. Results use `{ state, evidence, source }`, where state is `started`, `completed`,
`not-started` or `uncertain`. Negative outcomes require `fenced: true`. Completed compaction
additionally requires the supplied `checkpoint`, `resumed: true` and a real
`acceptanceEvidence` reference. These are adapter observations, not semantic judgments by
the runtime. Review completion also leaves delivery acceptance separate.

Malformed/lost results stay uncertain. Repeated execution returns the stored receipt;
it never repeats a native action. `reconcile(offerId)` calls `lookup` with the original
key/source/checkpoint. A lookup must use durable native receipts or a fence, not inference
from an absent response. Neither controller makes native integration verified: exercise
actual host round trips in an isolated installation and retain the resulting evidence.
