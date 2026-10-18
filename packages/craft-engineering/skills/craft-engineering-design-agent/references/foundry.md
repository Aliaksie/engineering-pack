# Foundry agent boundary decisions

Identify the actual Foundry product interface, SDK version, deployment and agent/session model.
Names and object lifecycles vary across interfaces; use installed contracts and current official
provider documentation when implementing. Azure hosting alone does not identify an agent API.

Map invocation identity, tool identity and resource authorization separately. Preserve consumer
policy for endpoints, regions and data retention. Connection declarations are not evidence that
an account is authenticated or that a tool is permitted for this task.

Own the lifecycle of runs, conversations and stored files. Define cleanup/retention and isolation
between tenants. Bound polling, tool-call loops and retries; treat cancellation as a request whose
external effects may need reconciliation. Validate generated arguments and tool outputs before
using them as trusted state.

Example investigation: a run enters a tool-required state while the client loses its response.
Recover the run and durable application operation identity before submitting a second write or
starting an independent run that could repeat the effect.

Record outcome, stop/failure reasons and observable usage without secrets. Verify actual tool
access and failure recovery against an authorized target. Distinguish local shape tests, provider
execution and deployed end-to-end behavior in the reported evidence.
