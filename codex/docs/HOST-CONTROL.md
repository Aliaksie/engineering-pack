# Host controls

Worker delegation, review and compaction are native phases of `craft`. Each phase is one
JSON request on standard input to `<package-root>/bin/craft` with no arguments (max 3 MiB):

```json
{"formatVersion": 1, "operation": "workers", "input": {"root": "/repo", "session": "s1", "task": "T1", "phase": "status"}}
```

The response is `{"formatVersion": 1, "ok": true, "result": …}`, or `{"ok": false, "error":
{"code", "message"}}` with exit status 1.

No package ships a host transport. An integration you own starts, observes and cancels host
work and calls these phases between its own callbacks; no lock is held across a callback.
Before each effect it must verify real authorization, native capability and current policy,
enforce capacity and permissions atomically at execution, and use the exact scope and model
it was given. Effects must be idempotent or have a durable lookup or fence. There is no
automatic retry and no time-based release.

## Workers

`operation: "workers"` with `root`, `session`, `task`, optional `preferencesFile`, `phase`:

| Phase | Input | Result |
|---|---|---|
| `open` | — | `{context: {tree, root, session, task}}` |
| `prepare` | `input`, `inspect` | Validated assignment; with `inspect` also `{targets, existing}` |
| `admit` | `input`, `targets`, `snapshot` | `{request}` reserved for dispatch, or `{duplicate: true}` |
| `record` | `id`, `result`, optional `generation` | `{request, executionObserved, modelMatches}` |
| `inventory` | optional `id` | `{targets, request}` |
| `reconcile` | `targets`, `snapshot` | Status after applying the observation |
| `cancel-check` | `request`, `snapshot` | The occupied request a cancellation may target |
| `status` | — | `{tree, requests, limits}` (same as `craft worker-status`) |

An assignment is `{id, offer, scope, parent?, reuseWorker?, brief}`, where `brief` has
`goal`, `sources`, `acceptance`, `restrictions` and `returnShape`. Its offer needs a current
recorded decision with matching source, policy, scope and task contract, plus a configured
host/model mapping.

- **Dispatch:** `prepare`, then authorize and observe, then `admit`, then start natively with
  the admitted `key`, `model`, `role`, `profile`, `source` and `taskContract`, then `record`
  `{kind: "observed", workerId, model, status, occupiesSlot, evidence}` with the request's
  `generation`. If the outcome is unknown, record `{kind: "uncertain"}`.
- **Cancel:** `inventory`, then `cancel-check`, then authorize, then `record {kind:
  "cancelling"}` (returns `cancelKey`), then cancel natively, then `record` the observation.

A snapshot is `{host, capacity, occupiedSlots, taskWorkers, allowed, sessionId, observedAt,
source, workers, requests}`, valid for 30 seconds. Occupied slots include the main session
and other tasks, and `workers` covers the whole task tree. Worker status is `running`, `idle`,
`completed`, `failed`, `cancelled`, `released`, `not-started` or `unknown`; completion never
implies release. A lookup in `requests` is `{key, status: "started", workerId, settled: true,
evidence}` or `{key, status: "not-started", fenced: true, evidence}`, and the integration
must then prevent any late execution of that key. Absence from an inventory is not evidence.

Intent is persisted before native calls. Duplicate IDs never redispatch, and changed content
under the same ID is rejected. Lost acknowledgements keep capacity until `reconcile`.
Generations stop late results from overwriting newer ones. A tree holds at most 64
assignments; at the bound new work is rejected rather than evidence deleted. Worker results
never complete the main task's acceptance.

## Review and compaction

`operation: "actions"` with `root`, `session`, `task`, optional `preferencesFile` and
`bundle`, and `phase`:

| Phase | Input | Result |
|---|---|---|
| `find` | `id` | The offer, with any stored `executionReceipt` |
| `claim` | `id`, `observation` | `{receipt, offer}`, or `{duplicate: true, receipt}` |
| `finish` | `id`, `receipt`, `result` | `{executed, uncertain, receipt}` |

Claim only an approved offer. `observation` is `{sessionId, observedAt, facts}`, observed
within 30 seconds. The claim rechecks task, source, policy and prerequisites and persists one
attempt; compaction saves a checkpoint first. Execute outside every lock with the receipt's
`key`, `source` and `checkpoint`. Results are `{state: started|completed|not-started|uncertain,
evidence, source}`. Negative outcomes need `fenced: true`. A completed compaction also needs
the receipt's `checkpoint`, `resumed: true` and an `acceptanceEvidence` reference. A lost
result is finished as `uncertain`. A repeated claim returns the stored receipt and never
repeats the action.

## Ownership and admission

`operation: "control"`. Every phase except `support` takes `root`, `session` and `task`;
journal phases also take `host` and `coordinator`.

| Phase | Input | Result |
|---|---|---|
| `support` | `host` | The host's feature and execution support table |
| `source` | `source` | Fails unless the task is active and the source unchanged |
| `admission-acquire` | `pid`, optional `source` | An admission ticket that keeps task-start from switching tasks |
| `admission-check` | ticket, optional `source` | Rechecks the binding |
| `admission-release` | ticket | Releases it; do so on every outcome |
| `open` | `pid` | `{context, ticket, revision, journal}` |
| `begin` | ticket, `expected`, `input`, `kind`, optional `nativeId` | New intent: `{revision, journal}` |
| `persist` | ticket, `expected`, `journal`, optional `withdrawn` | `{revision, journal}` |
| `close` | ticket | Releases journal ownership |

The journal records each native operation (`worker`, `cancel`, `review`, `compact`,
`release`) as intent before its effect. Each key is attempted at most once. `persist` may
advance phases and record native IDs but cannot erase an attempt or change its source or
checkpoint. Every write needs the current `revision`; a journal holds at most 192
operations. Tickets record the integration's own pid: a lease is reclaimed only after that
process is observed dead, and unresolved intent stays for the next owner to reconcile.

Repository tests drive every phase with a simulated host; they do not prove a real
integration, its authorization or model round trips.
