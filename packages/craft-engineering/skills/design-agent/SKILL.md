---
name: design-agent
description: Design model-driven control, retrieval, memory and tool boundaries using the consumer’s actual runtime.
---

# Design a bounded model-driven system

Separate deterministic application responsibilities from model judgment. Establish the accepted
input, useful output and trusted tool effects. Authorize each effect outside the model, validate
arguments and domain constraints, and define how invalid output or repeated calls are handled.
A parseable response is not permission to act.

Give the run budgets for elapsed time, attempts, tokens and effects. Specify cancellation and
partial-completion behavior. Persist enough operation identity to reconcile uncertain external
results; avoid retrying a write blindly after a timeout. Record safe correlation and configuration
identity without storing secrets or unnecessary prompts.

For retrieval, name the evidence missing from the task and first consider direct context or
bounded lookup. Keep source/revision with retrieved claims. Distinguish retrieval failure from a
model ignoring supplied evidence. Define behavior for stale, conflicting or unavailable sources.
Memory requires tenant boundaries, retention/deletion and a freshness rule; a conversational summary
cannot create policy or authorization.

Use [evaluate-agent](../evaluate-agent/SKILL.md) when changing model, prompt, retrieval or tool
behavior. Choose [Bedrock](references/bedrock.md) or [Foundry](references/foundry.md) only when the
affected agent configuration identifies that provider. Hosting on a cloud does not establish the
model provider, and these references do not authorize connection or deployment.
