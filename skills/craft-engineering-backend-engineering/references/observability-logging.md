# Observe outcomes and diagnose causes

Start with an operational question: are users completing the operation, which dependency is
failing, or where is time spent? Choose metrics, structured events and traces that answer it.
An alert needs a useful action and a threshold tied to service expectations, not merely any error.

Measure attempts and completed outcomes separately. Include latency distributions and relevant
resource saturation; averages can hide a slow tail. Correlate a request or durable operation across
boundaries with safe identifiers. Avoid unbounded labels such as raw URLs, user text or unique
request IDs in metric dimensions.

Control what leaves the process. Do not log credentials, session material or unnecessary personal
data. Redact before emission and review exception payloads and third-party tracing defaults.
Decide retention, access and sampling according to the consumer's policy.

```text
Example event fields: operation_kind, result_category, elapsed_ms, dependency_name,
correlation_token. Keep payload bodies outside the event unless a specific policy permits them.
```

Separate diagnostic telemetry from required audit records. A dropped diagnostic event may be
acceptable; an action requiring durable audit evidence may need to fail or defer when recording
fails. Verify that chosen behavior, including exporter failure, queue pressure and restart.
Telemetry should describe the operation without accidentally changing its success contract.
