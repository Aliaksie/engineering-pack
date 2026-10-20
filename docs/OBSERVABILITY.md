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

Configuration belongs to the consumer in `.capabilities/telemetry.json`, separately from
portable project decision configuration. No configuration file is required for defaults.

Disable local telemetry:

```json
{"local": false}
```

Default events are appended to `telemetry.jsonl` in private runtime state for the project
root selected by the CLI/hook. Run `settings` to inspect the directory: Git repositories use
worktree-specific Git metadata; non-Git folders use `.capabilities/state/`. The file rotates at 1 MiB with one 1 MiB backup named
`telemetry.jsonl.1`; events are limited to 8 KiB. New files use mode 0600 where supported.
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
more export work or diagnose their own failure.

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

## Session view and future analysis

`npm run session -- --root <project> --session <id> --task <id>` includes the pack version
and reuses existing session activity receipts. It does not reconstruct counters from
telemetry or treat selected guidance estimates as native occupancy. Native occupancy
remains unknown unless an actual supported session-bound observation is supplied.

Stable names, catalog IDs, scoped correlation, reason codes and schema versions support
future bounded aggregations of selection frequency, batching, cache reuse and escalation.
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
