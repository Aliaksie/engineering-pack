# Observability and privacy

Nothing is sent remotely unless you configure an OpenTelemetry endpoint. Local usage
telemetry is on by default for `craft` commands, hooks and the status line. It describes
pack operations, never the quality of the engineering result, and it makes no model calls.

On hook-capable hosts the engineering, stories and review packages declare observational
hooks; delivery records the same event through its routing and checkpoint hooks. They
record one `host.hook` event per hook and never decide, inject context or block. A direct
command records its own operation but does not prove that hooks run.

## What is recorded

Each event is schema-versioned JSON: name and kind (operation, observation or diagnostic),
timestamp, pack version, trace and operation IDs, a pseudonymous session correlation, the
host when known, outcome, duration, a stable reason code on failure and allowlisted
attributes such as selected catalog skills, guidance size, lookup and cache counts, advice and
checkpoints. Hook events also record the package and whether the input was complete,
invalid, oversized or unreadable.

Never recorded: prompts, transcripts, code, file contents, paths, search terms, tool output,
commands, environment variables, credentials, raw errors, model names, and consumer
profile, task, repository or account names. Unknown attributes are dropped before storage
or export.

Session correlation is an HMAC with a random project-local key; raw session IDs are never
stored. Without a session ID, events correlate only within one invocation
(`identityScope=invocation`). Pseudonymous is not anonymous: timing and selected skills are
still usage data.

## Local storage

Events go to `events.jsonl` under `.craft/state/telemetry/` (`craft settings` shows the
state directory). The file rotates at 1 MiB with one backup. The
correlation key is `key.json` beside it: delete the key to reset correlation, and the log
files to remove events.

Telemetry is best effort, not an audit log. Lock contention, a full disk or a bad
configuration can drop events. Drops are reported by a later `TELEMETRY_EVENTS_DROPPED`
diagnostic when possible. Telemetry failures never change a command's result.

Configure it in `.craft/telemetry.json`. To disable local telemetry:

```json
{"local": false}
```

## OpenTelemetry export

```json
{
  "local": true,
  "remote": { "enabled": true, "endpoint": "http://127.0.0.1:4318/v1/traces" }
}
```

The endpoint must be HTTPS, or HTTP on loopback, without credentials, query or fragment.
For authenticated backends, forward through a local collector. Invalid configuration
disables export and records `TELEMETRY_CONFIG_INVALID` locally.

Each `craft` invocation is one trace with its real child operations, exported as OTLP/HTTP
JSON spans (`service.name=engineering-pack`). `craft` posts the batch itself after writing
its output: at most 32 events, 256 KiB, a 300 ms deadline and no retries. HTTPS certificates
are verified by the operating system. Each flush records a local `telemetry.export` result;
a failed export never claims delivery.

## Reports and captures

```sh
craft telemetry-report  --root <project> [--session <id>]
craft telemetry-capture --root <project> --session <id>
craft telemetry-report  --root <project> --capture <capture-id>
craft telemetry-forget  --root <project> --capture <capture-id>
```

The report counts retained observations, failures, drops, failed exports and unreadable
records, without raw events or session identifiers. `observed` means a retained event exists.
`unobserved` means unknown, never "not used". Complete history is never claimed.

A capture keeps one report snapshot for an evaluation milestone: up to eight snapshots,
64 KiB in total, never replaced automatically. Captures hold counts only. They cannot replace
retained model outputs and judgments ([evaluation](EVALUATION.md)).

## Limits

- Disabled telemetry produces no diagnostics; an unusable state directory cannot record its
  own failure.
- Failures before `craft` runs (no executable for the platform, a damaged bundle) leave no
  record.
- Telemetry cannot show that guidance was loaded or followed, that a worker ran, or that
  quality improved. Host context occupancy stays unknown unless the host reports it.
