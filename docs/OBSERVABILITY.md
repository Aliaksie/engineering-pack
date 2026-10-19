# Observability and privacy

Engineering Pack does not send telemetry remotely by default.

Local structured usage telemetry is enabled by default for CLI commands, session views,
and supported native hooks. It describes pack operations, not the engineering result's
quality. Paired evaluations remain the separate source of quality evidence.

## What is recorded

Each schema-versioned JSON event contains a stable event name, timestamp, canonical pack
version, random operation ID, scoped correlation ID, known host when available, and
bounded metadata. Observed durations and success/failure outcomes accompany instrumented
operations. Selected catalog skills, guidance counts/bytes/token estimates, discovery
batch/evidence counts, cache reuse, attempt counts, session advice, checkpoints and local
review validation provide operational evidence. Unknown observations are omitted or
explicitly unknown. A guidance selection does not prove the agent loaded or followed it.

Event families are `routing.completed`, `guidance.selected`, `discovery.completed`,
`cache.observed`, `context.observed`, `session.observed`, `session.advice`, `model.advice`,
`attempt.recorded`, `checkpoint.saved`, `checkpoint.recovered`, `review.completed` and
`host.observed`. These observe meaningful pack boundaries rather than every function.
Review events describe local preparation/validation, never remote posting.

Telemetry intentionally excludes prompts, transcripts, code, file contents, file paths,
search terms, tool output bodies, commands, environment variables, credentials, raw
exceptions and arbitrary request/result fields. Metadata uses allowlists; unknown keys
and unrecognized enum values are dropped before either sink. Custom consumer skill IDs
are omitted unless present in the pack catalog. Host model names and consumer profile,
task, repository and account names are not collected.

Session correlation uses HMAC with a random project-local key. Raw host session IDs are
never persisted in telemetry. Without a real session ID, correlation is limited to the
current invocation and marked `identityScope=invocation`. IDs are pseudonymous, not a
guarantee of anonymity; activity timing and selected skills are still local usage data.

## Local configuration and storage

Configuration belongs to the consumer in `.capabilities/telemetry.json`, separately from
portable project decision configuration. No configuration file is required for defaults.

Disable local telemetry:

```json
{"local": false}
```

Default events are appended to `.capabilities/state/telemetry.jsonl` under the project
root selected by the CLI/hook. The file rotates at 1 MiB with one 1 MiB backup named
`telemetry.jsonl.1`; events are limited to 8 KiB. New files use mode 0600 where supported.
The correlation key is `.capabilities/state/telemetry-key.json`. Do not commit this state;
add `.capabilities/state/` to the consumer's ignore rules. Deleting the key resets future
correlation; delete both JSONL files to remove retained local events.

Writers use an exclusive lock and skip a busy writer. Readers skip malformed lines and
unknown schema versions within bounded files. Symlinked and oversized files are rejected.
Telemetry may drop events on concurrency, invalid configuration, permission, full disk
or exporter failure. A stale `telemetry.jsonl.lock` suppresses writes; remove it only when
no writer is active. Telemetry is supporting diagnostics, not a required audit log.
Failures do not change an engineering operation's result, permission or original error.

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
Malformed configuration disables telemetry rather than breaking engineering work.

The integration maps sanitized events into OTLP/HTTP JSON internal spans with
`service.name=engineering-pack`, `service.version` and optional `engineering-pack.host`
resource attributes. Each operation has an independent trace; the session correlation
attribute joins related operations without inventing trace ancestry. Timestamp and ID
encoding follow the [OTLP specification](https://opentelemetry.io/docs/specs/otlp/).
The implementation uses Node HTTP transport without an SDK or npm dependency.

Only fresh events are exported, with at most 32 queued events per process/flush, a 256 KiB
request bound, a 16 KiB response bound and a 300 ms transport deadline. CLI and installed
hook/session entry points await a bounded flush. There are no retries, daemon, metrics,
automatic instrumentation or historical replay. Direct application API callers can
establish `withTelemetry(...)` and await `flushTelemetry()` themselves; instrumentation
without an active scope is inert except for the standalone session operation.
Remote export is best effort and can lose events on process exit. Local retention and
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
