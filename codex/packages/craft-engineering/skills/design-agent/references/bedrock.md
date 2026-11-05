# Bedrock agent boundary decisions

Confirm which Bedrock service, agent/runtime interface and installed SDK the consumer actually
uses. Distinguish model inference from agent orchestration and managed runtime hosting. Resolve
version-sensitive API details through the installed code or current official service documentation
when implementing; do not infer a request shape from an unrelated AWS example.

Map the identity allowed to invoke the model separately from identities used by tools and deployed
resources. Apply least-privilege consumer policy and keep credentials out of prompts/configuration
examples. Check region/model availability and data-handling requirements before selecting a target.

For tool use, correlate requests and results, validate tool arguments outside model judgment and
handle denied or malformed invocations. SDK retries can repeat a request after an uncertain result;
non-idempotent tool effects need their own operation identity and reconciliation. Managed runtime
session state does not automatically supply application-level exactly-once processing.

Example investigation: a tool reserves capacity and the response stream disconnects. Verify whether
the reservation exists before another run repeats it, using the application's durable token.

Observe stop reasons, timeout, throttling, stream termination and usage metadata that the chosen
interface exposes. Exercise failure and tenant-isolation cases in an authorized nonproduction
target; report unavailable provider evidence rather than treating an SDK mock as a live result.
