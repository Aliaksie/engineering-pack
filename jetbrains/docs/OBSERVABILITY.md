# Observability and privacy

Engineering Pack does not send telemetry remotely by default.

Local structured usage telemetry is enabled by default for CLI commands, session views,
and supported native hooks. It describes pack operations, not the engineering result's
quality. Paired evaluations remain the separate source of quality evidence.

Standalone Engineering, Stories and Review packages declare observational hooks for
session start, prompts, tools, compaction and turn end. Delivery records these events
through its existing routing/checkpoint hooks. Review's publication policy remains a
separate hook; multiple loaded packages produce separate hook invocation records.
Claude, Codex, Cursor, Copilot and Gemini CLI exports
register the events their adapter supports. These hooks record `host.hook` and
`host.observed` events without routing, checkpoint writes or injected context. They do
not observe token usage or every host operation. Host loading and hook trust remain
required; workspace overlays require merging their generated hook configuration.
Observational hooks produce no permission decisions or injected context. Oversized input
and runtime observation failures return silently with exit 0; bounded diagnostics are
recorded when telemetry remains available. Policy hooks retain their existing failure
behavior. Missing Node or broken imports remain native process failures.
AGENTS, Gemini native extension and JetBrains exports remain guidance-only.
An explicit CLI/session command records its own operation even without automatic hooks;
it does not prove hook discovery. Direct wrapper tests establish runtime behavior,
not automatic host invocation.

## What is recorded

Each schema-versioned JSON event contains a stable event name and kind (operation,
observation or diagnostic), timestamp, pack version, trace ID, operation ID, optional
parent operation ID, scoped session correlation, known host when available, and bounded
metadata. Operation failures and diagnostics carry stable reason codes; raw errors never
enter the event contract. Missing version metadata is explicitly null, not a fabricated
release version. Observed durations and success/failure outcomes accompany instrumented
operations. Selected catalog skills, guidance counts/bytes/token estimates, discovery
batch/evidence counts, cache reuse, cumulative recorded-attempt counts, session advice, checkpoints and local
review validation provide operational evidence. Unknown observations are omitted or
explicitly unknown. A guidance selection does not prove the agent loaded or followed it.

Generated hook/session entry points attach the canonical package name from their export,
using an allowlist of the four bundled packages. Nested package scopes keep separate traces;
compatible project/session/host identity remains correlated. Generic CLI/API callers without
a package declaration remain unattributed to a package. Hook root observations include
`inputStatus=complete|invalid|oversized|read-failure`. Invalid, interrupted and oversized input
uses the process working directory and invocation correlation; no partial payload is parsed
to recover a session or project. Input diagnostics retain package, lifecycle and input status
when their telemetry scope is available. This identifies the entry point, not verified native
host dispatch. Raw task identities remain excluded; existing session/task state owns them.

Event families are `routing.completed`, `guidance.selected`, `discovery.completed`,
`cache.observed`, `context.observed`, `session.observed`, `session.advice`, `model.advice`,
`attempt.recorded`, `checkpoint.saved`, `checkpoint.recovered`, `review.completed` and
`host.observed`. Top-level `cli.command`, `host.hook`, `host.session` and `review.command`
operations contain meaningful child spans: project/input reads, search/cache lookup,
state updates and integration planning. `diagnostic` events record safe failure codes.
These observe meaningful pack boundaries rather than every function.
Review events describe local preparation/validation, never remote posting.

Telemetry intentionally excludes prompts, transcripts, code, file contents, file paths,
search terms, tool output bodies, commands, environment variables, credentials, raw
exceptions and arbitrary request/result fields. Metadata uses allowlists; unknown keys
and unrecognized enum values are dropped before either sink. Custom consumer skill IDs
are omitted unless present in the pack catalog. Host model names and consumer profile,
task, repository and account names are not collected.

Session correlation uses HMAC with a random project-local key. Raw host session IDs are
never persisted in telemetry. Without a real session ID, correlation is limited to the
current invocation and marked `identityScope=invocation`. A failed local correlation-key
read also uses an explicit invocation identity and reports `TELEMETRY_IDENTITY_UNAVAILABLE`;
it does not claim session correlation. IDs are pseudonymous, not a
guarantee of anonymity; activity timing and selected skills are still local usage data.

## Local configuration and storage

Configuration belongs to the consumer in `.craft/telemetry.json`, separately from
portable project decision configuration. No configuration file is required for defaults.

Disable local telemetry:

```json
{"local": false}
```

Default events are appended to `telemetry.jsonl` in private runtime state for the project
root selected by the CLI/hook. Run `settings` to inspect the directory: Git repositories use
worktree-specific Git metadata; non-Git folders use `.craft/state/`. The file rotates at 1 MiB with one 1 MiB backup named
`telemetry.jsonl.1`; events are limited to 8 KiB. New files use mode 0600 where supported.
CLI/hook application scopes collect at most 32 local events, then append that group under
one lock/open/write/close sequence. A partial group flushes synchronously when its scope
finishes, including operation exceptions and rejected promises; changing project, session,
host or package scope flushes earlier pending records first. Direct `createTelemetry` callers
retain immediate append behavior unless they explicitly select `batchLocal` and own `finish()`.
Diagnostics remain immediate and never enter the remote export queue through this path.
Groups rotate together when necessary, so rotation can occur up to one group earlier than
with individual appends. The same two-file/1 MiB-per-file bounds still apply.
The correlation key is `telemetry-key.json` in the same directory. Git metadata keeps
state out of commits without generated configuration or ignore edits. Exclude the non-Git
fallback directory before importing that folder into Git. Deleting the key resets future
correlation; delete both JSONL files to remove retained local events.

Writers use an exclusive lock and retry contention for up to 25 ms per append before
skipping a busy writer. Permission-denied locks are skipped immediately. Readers skip malformed lines and
unknown schema versions within bounded files. Symlinked and oversized files are rejected.
Telemetry may drop events on concurrency, invalid configuration, permission, full disk
or exporter failure. A stale `telemetry.jsonl.lock` suppresses writes; remove it only when
no writer is active. Telemetry is supporting diagnostics, not a required audit log.
Failures do not change an engineering operation's result, permission or original error.
Skipped/failed local appends are counted in memory and reported by a local
`TELEMETRY_EVENTS_DROPPED` diagnostic when storage recovers. These reports never enqueue
more export work or diagnose their own failure. The bounded in-memory count is cleared only
after its diagnostic append succeeds; later emissions or finish calls can retry an unsuccessful
diagnostic. Export queue drops likewise accumulate until their local diagnostic succeeds.
These are process-local observations, not durable loss counters across process exit.
A failed group append counts its member events, not just one failed write. Abrupt termination
can lose a partially buffered group; reads inside an unfinished scope may not yet see its
pending events. Explicit captures read the already-flushed log snapshot. Normal shipped
entry points finish their scopes before returning. Batching reduces repeated filesystem and
lock work; it does not turn this best-effort stream into a durable audit log.

## Optional OpenTelemetry export

Opt in by specifying the complete collector traces endpoint:

```json
{
  "local": true,
  "remote": {
    "enabled": true,
    "endpoint": "http://127.0.0.1:4318/v1/traces"
  }
}
```

Remote endpoints require HTTPS except for loopback HTTP. URL credentials, query strings
and fragments are rejected. This small foundation supports an unauthenticated collector;
use a locally managed collector for backend credentials and authenticated forwarding.
No environment variables are read into event attributes or exporter configuration.
To disable both sinks, set `local=false` and `remote.enabled=false` (or omit `remote`).
Invalid configuration disables remote export and records a local
`TELEMETRY_CONFIG_INVALID` diagnostic when storage is usable. An explicit `local=false`
still suppresses local writes. Malformed JSON cannot establish an opt-out; repair the
configuration to disable telemetry. Configuration failure never rejects engineering work.

The integration maps sanitized events into OTLP/HTTP JSON internal spans with
`service.name=engineering-pack`, `service.version` and optional `engineering-pack.host`
resource attributes. Each CLI command, native hook/session invocation or review command creates a trace.
Child operations preserve the real parent relationship through synchronous and
asynchronous calls; the OTLP mapping uses those IDs. Nested operations targeting a
different project or explicit session/host start an isolated scope and re-read that
consumer's sink configuration. Unspecified identity fields may inherit an existing
compatible scope. If a real session ID becomes available after input parsing, later
events can enrich the invocation identity while retaining the same trace. Earlier
events keep their original identity scope. No incoming remote trace context is invented. Timestamp and ID
encoding follow the [OTLP specification](https://opentelemetry.io/docs/specs/otlp/).
The implementation uses Node HTTP transport without an SDK or npm dependency.

Only fresh events are exported, with at most 32 queued events per process/flush, a 256 KiB
request bound, a 16 KiB response bound and a 300 ms transport deadline. CLI and installed
hook/session entry points await a bounded flush. There are no retries, daemon, metrics,
automatic instrumentation or historical replay. Direct application API callers can
await `flushTelemetry()` themselves. Root-aware application operations establish or
reuse a compatible project scope; root-less operations require `withTelemetry(...)`.
Remote export is best effort and can lose events on process exit. Queue overflow emits local `EXPORT_QUEUE_FULL` diagnostics; when possible
the bounded queue retains a completed root instead of a leaf so exported children have
a root. Each flush writes a local `telemetry.export` result with batch size, duration,
safe failure code and known acceptance/rejection counts. Failed exports do not claim
that events were delivered. Local retention and
remote backend retention are separate; the collector/backend owner controls the latter.

## Retained observation report

Run `npm run telemetry:report -- --root <project> [--session <id>]`, or use
`node <package>/support/cli/main.mjs telemetry-report` in a standalone export.
The bounded report counts retained feature observations and their failure/deferred outcomes,
reported local/queue drops and failed exports. It exposes retained file bytes, backup presence,
skipped malformed/unsupported/oversized records and invalid event envelopes. Session filtering
uses the existing project correlation key; the application report never creates a key or state.
Like other CLI commands, the CLI invocation itself records ordinary operational telemetry
after reading the report. The report does not replay events to a collector.

Storage and invalid-record counts describe the project files even with a session filter.
The retained event window describes the selected valid events. Invocation-scoped observations
cannot be attributed to a requested session and are excluded from its report. The report emits
no raw event bodies or session identifiers. It reads at most the current file and its backup;
concurrent writers are not locked, so the result is a best-effort snapshot.
Package counts and hook input-status counts expose available attribution; legacy or undeclared
metadata appears as `unknown`. Missing package metadata is never inferred from event names.

`observed` means a retained operational event exists; `unobserved` leaves applicability,
enablement and support unknown. These counts cannot establish semantic acceptance, guidance
loading, task attribution, worker dispatch or compaction execution. Attempt observations are
counted individually; cumulative attempt attributes are never added together. Complete history
is always false and total lost events stays unknown: rotation/deletion, disabled sinks and
failures before a diagnostic can be retained cannot be reconstructed. Backup presence proves
retained backup data, not a count of rotations or lost events. Keep campaign artifacts separately
when a complete evaluation record is required.

## Explicit bounded report retention

At an evaluation or audit milestone, retain the current report for a known real session:

```sh
npm run telemetry:capture -- --root <project> --session <id>
npm run telemetry:report -- --root <project> --capture <returned-capture-id>
npm run telemetry:forget -- --root <project> --capture <returned-capture-id>
```

Standalone packages expose the same `telemetry-capture`, `telemetry-report --capture` and
`telemetry-forget` commands through `support/cli/main.mjs`. Report reading accepts either a
live session filter or a retained capture ID. A capture requires existing session-attributed
observations and an existing correlation key; it never invents a session for invocation-only data.

Captures live in `telemetry-captures.json` in the existing private runtime directory. The
store is bounded to eight immutable report snapshots and 64 KiB total, with exclusive writer
locking and atomic replacement. Unchanged snapshots deduplicate. New snapshots never replace
old ones automatically: capacity or contention produces an explicit error. Use the returned
capture IDs to read or explicitly forget selected snapshots; forgetting changes only the
capture store at the application boundary. As with other CLI commands, these invocations also
record ordinary operational telemetry when enabled. Capture is an explicit local write and can
preserve already-retained data even when automatic local telemetry has since been disabled.

Snapshots contain bounded report counts, windows, storage/loss metadata, capture time and a
project-scoped session pseudonym. They exclude raw events, raw session/task identities, code
and output bodies. Report observations stay unchanged when rolling logs rotate or the
correlation key is removed; explanatory limitation text comes from the reading runtime. The
store validates its versioned shape and content-derived IDs before use. Those IDs detect
accidental corruption; they do not authenticate evidence against the filesystem owner.

Capture is manual, does not add a hook-time scan or automatic archive, and does not launch a
model or replay data to a collector. It preserves only the observations still retained at
capture time, with the original unknown total-loss/complete-history status. It cannot restore
earlier rotation losses or substitute for frozen tasks, source/export identities, actual model
outputs and judgments in the separate evaluation campaign. Preserve required campaign
artifacts outside this bounded store before explicitly removing captures. Deleting the capture
store removes all captured reports; deleting logs or the correlation key alone does not.

## Session view

`npm run session -- --root <project> --session <id> --task <id>` includes the pack version
and reuses existing session activity receipts. It does not reconstruct counters from
telemetry or treat selected guidance estimates as native occupancy. Native occupancy
remains unknown unless an actual supported session-bound observation is supplied.

Stable names, catalog IDs, scoped correlation, reason codes and schema versions support
bounded retained-observation reports.
No dashboard or analytics service is installed. Telemetry cannot establish a percentage
improvement in engineering quality.


## Portable observation contract

`application/telemetry/events.mjs` is the provider-neutral JSON validation boundary.
`traceContext` contains only `traceId` (32 hex characters), `operationId` (UUID string)
and an optional distinct `parentOperationId`. Events add `schemaVersion=1`, `name`,
`kind`, UTC `timestamp`, `sessionId`, `packVersion`, bounded allowlisted `attributes`,
optional known `host`, `outcome`, observed `durationMs` and a recognized `reasonCode`.
The schema-1 additions are compatible with reading earlier JSONL records. The OTLP
mapper accepts legacy records without trace context as independent roots.

Reasons are deterministic codes such as `CONFIG_INVALID`, `INVALID_INPUT`,
`SEARCH_FAILED`, `SEARCH_INCOMPLETE`, `STATE_BUSY`, `STATE_UNAVAILABLE`,
`CHECKPOINT_UNAVAILABLE`, `CONTEXT_UNAVAILABLE`, `HOST_CAPABILITY_UNAVAILABLE`, `HOST_OBSERVATION_UNAVAILABLE`,
`REVIEW_INVALID`, `ATTEMPT_BUDGET_EXHAUSTED`, `EXPORT_TIMEOUT` and
`EXPORT_PARTIAL_REJECTED`. Exporter transport details stay in the integration mapping;
Node objects, exception messages/stacks, SDK objects and callbacks are not event fields.
A Rust implementation can produce the same JSON without adopting the Node context
storage implementation. Unknown attribute keys (including prototype property names)
and unrecognized reasons are never copied as arbitrary data.

`recordedAttempts` is the existing cumulative `activity.operationsAttempted` counter for
that session/task; it continues past retained-history eviction. It is not a model-quality
score, per-profile retry ordinal, or count of retained receipts. Existing attempt policy
and session accounting remain the owners of retry/escalation decisions.

## Coverage and deliberate limits

The following applies to shipped entry points and explicitly scoped application APIs.
All observations use the same allowlist before storage or export.

| Runtime area | Success | Failure | Trace correlation | Local | OTEL |
|---|---|---|---|---|---|
| CLI entry/exit and input/config reads | yes | stable codes | root + children | yes | opt-in |
| Native hooks and native session adapter | yes | malformed input, config, evaluation | root + children | yes | opt-in |
| Routing and selected guidance | yes | validation/routing codes | routing parent | yes | opt-in |
| Discovery, bounded search and batches | yes | failure/incomplete evidence | cache/search children | yes | opt-in |
| Cache reuse/invalidation | yes | unavailable/busy state | caller parent | yes | opt-in |
| Session/context and host observations | yes | unavailable/stale observations | caller parent | yes | opt-in |
| Capability/model advice and attempts | decisions + recorded counts | unavailable/budgets/validation | caller parent | yes | opt-in |
| Checkpoint reads/save/recovery | yes | unavailable/busy/invalid state | caller parent | yes | opt-in |
| Review preparation/validation/planning | yes | validation/artifact failures | root + children | yes | opt-in |
| Service/integration plans | yes | unavailable inventories/mappings | caller parent | yes | opt-in |
| Session state reads/updates | yes | I/O/validation/busy state | caller parent | yes | opt-in |
| Export and telemetry infrastructure | flush result | loss/timeout/rejection codes | source trace where known | yes, when enabled/usable | deliberately local only |

Deliberate limits:

- Fully disabled telemetry produces no diagnostic stream. A disabled local sink cannot
  retain exporter diagnostics; a physically unusable state directory cannot report its
  own failure durably. In-memory loss counters are bounded and may be lost at process
  exit. No stderr/model-context fallback or unbounded retry is added.
- Telemetry/export diagnostics bypass the export queue to avoid recursive failures.
  Remote data remains best effort and may be incomplete; inspect local queue/flush
  diagnostics before interpreting remote totals.
- Pure engine decisions, trivial getters, CLI help and formatting are not independent
  traces. Root-less application API users must supply a consumer root/identity with
  `withTelemetry`; helpers do not guess a project or authorize effects. Export flush
  occurs at the owning process/lifecycle boundary.
- The separate personal inbox under `tools/craft-review-inbox` is excluded from pack
  distributions and retains its own scan/transport diagnostics. Its shared integration
  decision is observable when called inside a scope; this pass does not change that
  separate tool's command lifecycle or execute remote probes.
- Failures before a runnable entry point exists (missing Node, syntax/import corruption)
  remain native process errors. Unknown host occupancy stays unknown. Selection events
  describe pack decisions and do not prove native guidance loading or engineering quality.

## Performance and model-cost contract

Observability is deterministic infrastructure. It does not invoke AI models and therefore
adds no model-token or model-call cost. Its runtime overhead is limited to bounded local
processing/storage and, when explicitly enabled, bounded best-effort OTEL export.

Event selection, codes, privacy filtering, correlation and export use ordinary code.
Timestamps and unique IDs are runtime observations; serialization is deterministic for
those explicit inputs. There is no model-based classification, sampling, summarization
or analysis, and no extra guidance/context is returned for observability. Normal
operations perform bounded local work and enqueue remote events without waiting on
network I/O. Only the owning CLI/hook/session lifecycle awaits the bounded flush. Export
failure cannot retry a model, launch a tool/worker, or schedule another agent turn.

Coverage tests assert known trace parentage, privacy, scope isolation (including async
callbacks), disabled behavior, loss diagnostics and unchanged native wire output. Tests
also spy on process/network/model-access paths to ensure default observability does not
invoke them. No benchmark framework or model-based evaluator is installed.
