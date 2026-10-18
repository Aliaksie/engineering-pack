---
name: craft-delivery-workflow-deliver-change
description: "Carry an authorized change from its current stage to verified local completion."
---

<!-- Generated from packages/; edit the portable source and rebuild. -->

Read the shared [package instructions](../../../packages/craft-delivery/AGENTS.md) when using this capability.

# Carry evidence through delivery

Start from what already exists: a request, accepted plan, partial implementation or review
finding. Preserve one compact task record containing scope, acceptance conditions, decisions,
current paths/revision and verification evidence. Do not repeat discovery or create a new
document for every stage.

Use `craft-delivery-inspect-architecture` for a boundary that affects the solution and
`craft-delivery-plan-change` for material unresolved implementation choices. Use
`craft-delivery-route-work` to select independent practices and stack guidance when needed,
then `craft-delivery-apply-change` for the authorized implementation. Local work remains usable
without a remote tracker or model worker.

Before implementation, carry forward the applicable accepted decisions and read the selected
engineering detail for the affected behavior. Reuse current evidence, but do not bypass this
step because the edit looks small. Use `craft-delivery-inspect-architecture` to settle missing
decision context; use `craft-delivery-write-adr` only when a consequential choice needs a record.
This is a workflow prerequisite, not a claim that every host can enforce document reads.

Delegate only useful independent work under the existing authorization and host capabilities.
Give each worker an outcome, ownership scope, base identity, constraints and checks. Read-only
lookup can share source; concurrent writers should use separate branches/worktrees where
supported. Coordinate overlapping paths and shared test resources. Review returned changes
and verify the combined state before calling the task complete; worker success is not an
integration result. If commits are in scope, group changes by coherent task outcome rather
than by worker, and follow the consumer's message and merge conventions.

Exercise the acceptance conditions with real checks. Update inaccurate maintained context
through `craft-delivery-maintain-context`. Use `craft-delivery-review-code` for a requested or
risk-justified review, keeping its scope aligned with the current changes. Selecting independent
review does not grant permission to spawn an agent. A committed snapshot excludes working-tree
edits; inspect those directly rather than creating an unauthorized commit.

Correct in-scope defects and repeat affected checks after a correction. Keep evidence attached
to the version it covers. Stop a dependent stage when a decision or capability is missing,
while completing useful independent work; preserve the partial result instead of inventing a
pass or retrying without a stopping condition.

Complete local delivery with the changed behavior, acceptance-to-check results and material
limits. If the user also requested an external or installation action, continue with
`craft-delivery-workflow-ship-change` under that existing authorization and explicit target.

Package tools and shared assets: `../../../packages/craft-delivery` relative to this skill directory.
