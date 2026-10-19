---
name: workflow-deliver-change
description: "Implement an authorized feature or product-code fix through scoped coding, verification and local completion."
---

<!-- Generated from packages/; edit the portable source and rebuild. -->

Read the shared [package instructions](../../AGENTS.md) when using this capability.

# Carry evidence through delivery

Use this flow when the requested outcome includes changing product behavior through code.
For testing-only requests, use `craft-engineering:workflow-verify-feature` when available;
verification does not by itself authorize implementing or repairing the product.

Start from what already exists: a request, accepted plan, partial implementation or review
finding. Preserve one compact task record containing scope, acceptance conditions, decisions,
current paths/revision and verification evidence. Keep this record private by default; use
conversation, existing task state or verified ignored/outside-checkout storage. Do not stage
internal plans or leak their references into shipped artifacts. Do not repeat discovery or create a new
document for every stage. For cross-repository work, retain ownership of facts, generation,
dependencies, tests, CI and outputs before adding infrastructure. Resolve missing ownership
through `craft-delivery:inspect-architecture`; reassess when later evidence changes scope.

Use `craft-delivery:inspect-architecture` for a boundary that affects the solution and
`craft-delivery:plan-change` for material unresolved implementation choices. Use
`craft-delivery:route-work` to select independent practices and stack guidance when needed,
then `craft-delivery:apply-change` for the authorized implementation. Local work remains usable
without a remote tracker or model worker.

Before source edits, establish a safe change boundary through `craft-delivery:apply-change`,
respecting consumer branching/workspace policy, repository protection and unrelated work.
Applying this guidance does not require installing repository Git hooks.

Before implementation, carry forward the applicable accepted decisions and read the selected
engineering detail for the affected behavior. Select security/testing detail by the actual
concerns, carrying forward still-current selections. Reuse current evidence, but do not bypass this
step because the edit looks small. Use `craft-delivery:inspect-architecture` to settle missing
decision context; use `craft-delivery:write-adr` only when a consequential choice needs a record.
This is a workflow prerequisite, not a claim that every host can enforce document reads.

Delegate only useful independent work under the existing authorization and host capabilities.
Give each worker an outcome, ownership scope, base identity, constraints and checks. Read-only
lookup can share source; concurrent writers should use separate branches/worktrees where
supported. Coordinate overlapping paths and shared test resources. Review returned changes
and verify the combined state before calling the task complete; worker success is not an
integration result. If commits are in scope, group changes by coherent task outcome rather
than by worker, and follow consumer commit strategy, message and merge conventions under
existing authorization. Preserve coherent, verified review/rollback boundaries; retain resulting
commit IDs privately. Neither implementation nor delegation grants commit or remote-action scope.

Carry the chosen evidence/execution approach and checkpoint boundary from planning. Reassess
when risk, evidence or phase changes; do not poll model/session advice every turn. Use
`craft-engineering:foundations` for bounded batching, worker returns and continuation detail.

Exercise the acceptance conditions with real checks. Structural validity does not establish
user-facing quality: review presentation or usefulness when the declared artifact criteria
require it. Update inaccurate maintained context through `craft-delivery:maintain-context`. Use `craft-delivery:review-code` for a requested or
risk-justified review, keeping its scope aligned with the current changes. Selecting independent
review does not grant permission to spawn an agent. A committed snapshot excludes working-tree
edits; inspect those directly rather than creating an unauthorized commit.

Correct in-scope defects and repeat affected checks after a correction. Keep evidence attached
to the version it covers. Stop a dependent stage when a decision or capability is missing,
while completing useful independent work; preserve the partial result instead of inventing a
pass or retrying without a stopping condition. After an environment failure, diagnose the
missing prerequisite once; retry only when evidence or conditions change. Continue independent
authorized work without adding unrelated network/browser preflights.

At a completed stage, use the bundled `session-advice` action when a next-step suggestion
would help. Supply the current task/revision and observed verification, not a guessed status
from tool success. Continue necessary authorized work; a suggestion is not an extra approval
gate. Use native context indicators; file-size estimates do not establish session occupancy.

Complete local delivery with the changed behavior, acceptance-to-check results and material
limits. Record generation/check results, relevant presentation review, local revision/commit
and remote delivery independently. Mark unavailable checks with the missing prerequisite;
hook success proves none of these outcomes. A failed push leaves successful local work intact
and remote delivery incomplete. Unknown usage stays unknown. If the user also requested an
external or installation action, continue with
`craft-delivery:workflow-ship-change` under that existing authorization and explicit target.
