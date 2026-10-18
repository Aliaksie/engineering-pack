---
name: workflow-explore-testing
description: "Prepare or conduct bounded exploratory testing from requirements and an accessible application, including without source code."
---

<!-- Generated from packages/; edit the portable source and rebuild. -->

Read the shared [package instructions](../../AGENTS.md) when using this capability.

# Explore an outcome and retain evidence

Start from the user's outcome, acceptance conditions and authorized application/environment.
Agree a bounded charter: the behavior to investigate, important risks, available time or stop
condition, test identities/data and excluded actions. Source access and an automated runner
are optional. Missing requirements stay questions rather than invented expected behavior.

Choose observations that could expose a consequential failure: transitions, invalid input,
permissions, recovery or accessibility where relevant. Adapt the next probe to what was
observed; an exhaustive generic checklist is not the goal. Use the consumer's available
browser or other tools only within scope. Creating data, submitting forms and retrying an
operation may change remote state; an exploratory charter does not grant those permissions.

If execution is unavailable, return the charter and proposed probes with prerequisites.
Do not invent observations, screenshots or pass results. If execution is available, retain
the build/version and environment, relevant preconditions/data, action, expected basis,
observed result and a bounded artifact reference. Preserve unexpected results before reruns.
Stop a probe when its next action would exceed authorization or contaminate shared data.

Return useful findings with reproduction steps, expected versus observed behavior and impact;
separate defects supported by evidence from uncertain requirements. Summarize explored and
unexplored scope, blocked probes and remaining risk. Reuse this evidence for acceptance or
automation decisions; exploratory success does not establish exhaustive coverage. Publish
defects or change tracker state only under existing explicit authorization.
