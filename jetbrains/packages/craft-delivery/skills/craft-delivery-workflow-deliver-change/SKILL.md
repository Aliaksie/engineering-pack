---
name: craft-delivery-workflow-deliver-change
description: "Implement an authorized feature or product-code fix through scoped coding, verification and local completion."
---

<!-- Generated from packages/; edit the portable source and rebuild. -->

Read the shared [package instructions](../../AGENTS.md) when using this capability.

# Carry evidence through delivery

Use this flow when the requested outcome includes changing product behavior through code.
For testing-only requests, use `craft-engineering-workflow-verify-feature` when available;
verification does not by itself authorize implementing or repairing the product.

Keep a tiny, understood correction direct: inspect the affected code and constraints,
make the scoped edit, run an adequate check and report the result. Size alone does not
require setup, a structured contract, an ADR, a worker or a tracker. For larger tasks,
carry the existing criterion IDs and checked source through the flow below; a passing
unrelated check cannot certify completion.

Start from what already exists: a request, accepted plan, partial implementation or review
finding. Preserve one compact task record containing scope, acceptance conditions, decisions,
current paths/revision and verification evidence. Keep this record private by default; use
conversation, existing task state or verified ignored/outside-checkout storage. Do not stage
internal plans or leak their references into shipped artifacts. Do not repeat discovery or create a new
document for every stage. For cross-repository work, retain ownership of facts, generation,
dependencies, tests, CI and outputs before adding infrastructure. Resolve missing ownership
through `craft-delivery-inspect-architecture`; reassess when later evidence changes scope.

For multi-step work, work spanning sessions, delegation, several repositories or Craft
scratch files with the bundled local runtime, read [task runtime](../../standards/task-runtime.md)
once: task binding, plans and workers, workspace scope, acceptance receipts and action offers.

Use `craft-delivery-inspect-architecture` for a boundary that affects the solution and
`craft-delivery-plan-change` for material unresolved implementation choices. Use
`craft-delivery-route-work` to select independent practices and stack guidance when needed,
then `craft-delivery-apply-change` for the authorized implementation. When the request
supplies a story or acceptance criteria to build, carry them through
`craft-delivery-implement-story`. Local work remains usable
without a remote tracker or model worker.

Before source edits, establish a safe change boundary through `craft-delivery-apply-change`,
respecting consumer branching/workspace policy, repository protection and unrelated work.
Applying this guidance does not require installing repository Git hooks.

Before implementation, carry forward the applicable accepted decisions and read the selected
engineering guidance for the affected behavior. Select security/testing detail by the actual
concerns, carrying forward still-current selections and current evidence. Load a skill's
references only when the change touches what they cover; a tiny correction rarely needs them.
Use `craft-delivery-inspect-architecture` to settle missing decision context; use
`craft-delivery-write-adr` only when a consequential choice needs a record.
This is a workflow prerequisite, not a claim that every host can enforce document reads.

If commits are in scope, group changes by coherent task outcome rather than by worker,
and follow consumer commit strategy, message and merge conventions under
existing authorization. Preserve coherent, verified review/rollback boundaries; retain resulting
commit IDs privately. Neither implementation nor delegation grants commit or remote-action scope.

Exercise the acceptance conditions with real checks, sized to the boundary and the consumer's
verification policy as in `craft-delivery-apply-change`. Structural validity does not establish
user-facing quality: review presentation or usefulness when the declared artifact criteria
require it. Update inaccurate maintained context through `craft-delivery-maintain-context`. Use `craft-delivery-review-code` for a requested or
risk-justified review, keeping its scope aligned with the current changes. Selecting independent
review does not grant permission to spawn an agent. A committed snapshot excludes working-tree
edits; inspect those directly rather than creating an unauthorized commit.

Correct in-scope defects and repeat affected checks after a correction. Keep evidence attached
to the version it covers. Stop a dependent stage when a decision or capability is missing,
while completing useful independent work; preserve the partial result instead of inventing a
pass or retrying without a stopping condition. After an environment failure, diagnose the
missing prerequisite once; retry only when evidence or conditions change. Continue independent
authorized work without adding unrelated network/browser preflights.

Complete local delivery with the changed behavior, acceptance-to-check results and material
limits. Record generation/check results, relevant presentation review, local revision/commit
and remote delivery independently. Mark unavailable checks with the missing prerequisite;
hook success proves none of these outcomes. A failed push leaves successful local work intact
and remote delivery incomplete. Unknown usage stays unknown. If the user also requested an
external or installation action, continue with
`craft-delivery-workflow-ship-change` under that existing authorization and explicit target.
