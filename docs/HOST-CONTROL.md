# Trusted host controllers

The standalone runtime exports `hosts/workers.mjs` and
`application/session/actions.mjs` under each package's `support/` directory. These explicit
JavaScript APIs accept trusted adapter functions. The worker adapter invokes native phases
for ledger reads, admission, reconciliation and result recording. Project JSON cannot load
adapters or grant permission. CLI recommendations remain advice; the shipped hooks supply no native
worker, review or compaction transport. Explicit native bindings are provided for Codex
App Server, Claude Agent SDK and ACP agents. Model switching remains a host/manual operation.

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

Call `dispatch({ id, offer, scope, parent?, reuseWorker?, brief })`; the assignment is copied
before awaiting authorization. `brief` contains `goal`,
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

New action offers bind the task-contract digest as well as source and effective policy.
Scope, risk or engineering-contract amendments invalidate the old offer even without a
source edit. Historical offers without this binding cannot authorize a structured task.
Native worker/review dispatch receives `taskContract`; worker status projects its original
binding and `freshness` (`current`, `stale` or `unknown`) without rewriting the lifecycle
receipt. A completed worker may therefore have stale evidence and still occupy a slot.
Reconcile native effects separately from verifying the amended task's acceptance.
Bound worker requests use worker-ledger format 2 so their contract survives offer
eviction. Readers retain version-1 requests with unknown bindings; old version-1-only
controllers reject version 2. Stop incompatible controllers before upgrading and never
downgrade the ledger to resume an older writer.

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

## Codex App Server adapter

`hosts/codex/control.mjs` (installed as `support/hosts/codex/control.mjs`) implements the
callback contracts above against the [official Codex App Server protocol](https://developers.openai.com/codex/app-server/).
It uses native thread/turn, inline review, interrupt and compaction operations. This is an
explicit client integration, not a hook that controls an already-running desktop or CLI.
Other native bindings use the same portable controllers through the common entry below.

An integration calls `createCodexControls({ root, session, task, threadId, capacity, client,
authorize, observations, verifyResume })` with an initialized, exclusively owned client.
`client` supplies `request(method, params)` and `subscribe(listener)` returning an unsubscribe
function. `openCodexControls` instead accepts explicit `processOptions: { command, args,
cwd, env, timeoutMs, onRequest }`, starts the configured executable with `app-server --listen
stdio://`, initializes it and resumes only the supplied coordinator thread. It never
discovers an active UI thread. The real coordinator's session ID must match `session`.

The owner supplies an isolated native profile/environment and existing authentication;
neither entry point installs a plugin, grants trust or edits user settings. The stdio
process inherits the caller's environment when `env` is omitted. Supply the intended
environment explicitly to avoid falling back to active configuration. The owner must
control enabled MCP servers, plugins, hooks and network tools as well as filesystem
permissions: a read-only filesystem sandbox alone does not authorize external effects.
Native server approval/input requests are rejected unless `onRequest` explicitly handles
them through the actual host authorization mechanism. No approval is inferred from a
Craft offer, native timeout or missing callback.

`capacity` allocates the exclusive client's total lane, including one coordinator slot;
it is not a discovered provider/account limit. The adapter inventories loaded threads,
counts other loaded work conservatively and shares the existing task-tree admission ledger.
Nested assignments must use the same controller object. The worker lane starts independent
read-only threads with exact configured models, bounded briefs and native subagent tools
disabled. It supports read-only extraction/review workers; implementation workers that need
write access remain unsupported. This avoids inheriting the coordinator transcript or
silently widening permissions. Model observations identify native thread configuration,
not per-token execution or billing telemetry.

The returned object exposes `workers`, `actions`, `nativeStatus()`, `output(key, cursor?)`,
`release(workerId)` and `close()`. `output` reads bounded pages of the actual native turn
items; the caller retains those outputs for review/evaluation. It does not interpret findings
as acceptance. `release` unsubscribes only settled work; the slot remains occupied until a
native observation establishes the thread is unloaded. Acknowledged interruption likewise
holds capacity until the matching native turn settles. `close` releases local ownership;
it does not certify workers stopped or delete their native history. Owned stdio entries return
a promise from `close()`; await it before deleting the process working directory. Shutdown
has a deadline and reports unverified closure if that deadline expires.

`authorize` must verify current scoped authority and return `{ allowed, evidence }` for
each native effect, including cancellation/release. `observations` supplies the fresh
session-bound action facts expected by the generic controller. Cumulative native token
notifications do not supply current context occupancy. Compaction is available only with
`verifyResume`, which checks the supplied source/checkpoint after native completion and
returns a real `acceptanceEvidence` reference. The adapter waits for the compaction item
and matching completed turn, resumes the exact coordinator, then asks for that evidence.
An RPC acknowledgement alone remains a started action.

Native operation intent is retained in bounded private storage before each effect. One
exclusive owner lock prevents competing clients for the same task tree. There is no
automatic stale-lock takeover after a crash: verify the former owner/native work stopped
before explicit local recovery. Operation records are not evicted, retries are not automatic,
and absent native history never becomes a negative fence. Losing a start acknowledgement
before its native identity is persisted can leave an unresolved reservation requiring manual
reconciliation. This conservative failure mode avoids duplicate model work.

The protocol mappings have deterministic process/adapter tests. Actual installed-version,
profile, authorization and model round trips still require the separate integration campaign.

## Common entry and other native hosts

Every standalone export ships `support/hosts/control.mjs`. Use
`createHostControls(adapter, options)` with a verified existing client, or
`await openHostControls(adapter, options)` to explicitly start a configured native process.
Both expose worker/action controllers, capability inspection, native output, status and
cleanup. Construction never installs packages or changes trust.

| Export | Native binding | Limits |
|---|---|---|
| Codex | Owned App Server client | Exact coordinator/session and fresh native inventory |
| Claude | Consumer-supplied official SDK `query` | Actual session/model/result stream; read-only tools |
| Cursor | ACP v1 via `agent acp` | Advertised exact model and verified read-only mode |
| Copilot | ACP v1 via `copilot --acp --stdio` | Optional capabilities/commands must be advertised |
| Gemini overlay / extension | ACP v1 via `gemini --acp` | Same controls; extension still registers no hooks |
| JetBrains | Explicit `underlyingHost` | Native AI Chat rules provide no execution API |
| AGENTS | Explicit `underlyingHost` | Guidance provides no universal execution protocol |

JetBrains and AGENTS return unsupported manual controllers without `underlyingHost`.
With one, the actual native host owns mappings, session identity, authorization and controls.
A JetBrains client using Claude binds `underlyingHost: "claude"`; it does not invent a
JetBrains native chat dispatcher.

ACP creation requires an initialized `client`, `protocolVersion: 1`, actual
`agentCapabilities`, native `coordinatorState` and matching real `coordinatorId`/`session`.
Set `readOnlyMode` from the host's verified read-only modes. Workers start fresh sessions,
apply advertised mode/model options and verify acknowledgements. Legacy advertised modes
are supported; missing model configuration fails closed. The owner must enforce tool
authority and prevent native delegation outside the shared controller.

Opening ACP additionally requires explicit `processOptions`: executable, optional argv/env,
cwd, deadlines and native request handling. It advertises no client filesystem/terminal
tools. Optional `authenticate` selects an advertised method; it does not create credentials
or perform interactive login. Coordinator resume/load requires the advertised capability.
No active editor session is discovered. Omitted environments inherit caller configuration;
use an explicitly isolated profile and review its hooks, plugins and MCP connections.

ACP prompt promises and startup waits have owners and bounds. Cancellation is a notification;
the matching prompt result settles it. Optional native close must acknowledge slot release.
Compaction sends only advertised `compact`/`compress`, requires a completed compaction entity
and successful prompt result, then native resume/load and checkpoint-bound acceptance.
Historical replay cannot prove a new compaction. Missing commands are never sent as model
prompts. Fresh native `usage_update` occupancy stays separate from cumulative billing.

Claude receives the installed official SDK `query` function; the package has no npm
dependency and never searches for or installs an SDK. `sdkOptions` allow explicit environment,
executable, plugin/settings sources, budgets and diagnostics, with permission/tool controls
fixed to the bounded read-only lane. Follow-up work resumes only an observed idle mapped
worker. Interrupt acknowledgement and final result settlement stay separate. Review resumes
the exact coordinator with the approved scope. Compaction checks `supportedCommands()`
before releasing `/compact` through streaming input, requires a compact boundary and
successful result, then runs a separate resumed query carrying source/checkpoint. The owner
supplies real `verifyResume` acceptance evidence; model text alone never establishes it.

Native admission rechecks the persisted active task, checkout binding and source after asynchronous
prerequisites, under the same binding lock used by task-start. Compaction reconciliation checks
fresh authority again before submitting resume; reading a receipt does not authorize another query.

Managed bindings check release prerequisites before persisting release intent. A premature
release can be retried after completion. `reconcileRelease(workerId)` inspects driver observations
for an attempted release and never repeats the effect. If the native transport cannot establish
release after a lost acknowledgement, the slot remains uncertain and occupied.

These bindings share bounded private intent storage, exclusive ownership and no automatic
effect retries or stale-lock takeover. An unresolved coordinator action blocks another
native coordinator action even after restart. Output is bounded and marks truncation;
missing native usage/cost fields stay unknown. Native success is not delivery acceptance.

Each `SETUP.json` and adapter report describes feature support, activation requirements
and evidence limits. Portable commands remain available where Node/shell
execution exists. Hooks retain each export's actual mappings. Metadata and protocol tests
do not establish installed activation, ordinary-work adoption or model quality.

Sources: [Claude sessions](https://code.claude.com/docs/en/agent-sdk/sessions),
[permissions](https://code.claude.com/docs/en/agent-sdk/permissions),
[Cursor ACP](https://cursor.com/docs/cli/acp),
[Copilot ACP](https://docs.github.com/en/copilot/reference/copilot-cli-reference/acp-server),
[Gemini ACP](https://geminicli.com/docs/cli/acp-mode/),
[ACP setup](https://agentclientprotocol.com/protocol/v1/session-setup),
[configuration](https://agentclientprotocol.com/protocol/v1/session-config-options) and
[prompt lifecycle](https://agentclientprotocol.com/protocol/v1/prompt-turn).
