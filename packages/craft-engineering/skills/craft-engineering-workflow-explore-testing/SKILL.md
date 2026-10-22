---
name: craft-engineering-workflow-explore-testing
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

Carry supplied criterion IDs through the charter, each probe and the final findings.
Map a probe to its affected flow/risk, expected requirement or explicit question,
preconditions and test-data assumptions, actions, observations and retained artifact.
Inspect configured automation and what it asserts; filenames and a green suite do not
prove affected-flow coverage. Prioritize changed transitions and adjacent recovery or
permission flows, and state what remains unexplored.

Keep implementation and QA evidence together under the same criterion IDs. Preserve a
contradictory result with both reporters and artifacts; do not silently select a pass.
Bind observations to the actual build and environment, including relevant configuration.
For a non-Git application, record an explicit build/version-and-environment identity;
missing identity leaves freshness unknown. A changed build invalidates earlier evidence.
An unexecuted probe is `not-run`; missing execution access is `not-available` with its
prerequisite (the task may be blocked). Neither is passed. Do not copy a current identity
onto an old observation. Keep observations in artifacts, not invented execution receipts.

The optional runtime's `qa-projection` operation reads the existing contract and evidence
ledger plus bounded probe detail; it neither executes probes nor persists a separate QA
state. Without runtime access, retain the same facts in the charter/report. Required
manual acceptance uses declared manual checks; automation does not certify a different
manual observation. Testing-only scope grants no product edits or remote mutations.
