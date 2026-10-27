---
name: craft-delivery-workflow-deliver-change
description: "Implement an authorized feature or product-code fix through scoped coding, verification and local completion."
---

<!-- Generated from packages/; edit the portable source and rebuild. -->

Read the shared [package instructions](../../../packages/craft-delivery/AGENTS.md) when using this capability.

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

For multi-step work with the bundled local runtime, bind the task once with `task-start`
using the host session id from the session-start context, a stable task id, goal and relevant
paths. When that context offers an earlier unfinished task that the request continues, adopt it
with `task-adopt --from <from>` instead of starting it again. When it lists an open plan the
request continues, start its next ready task with `task-start --plan <plan> --ready` (or
`--task <task>`; `plan-next` shows the queue); take over a task held elsewhere, even a stale
one, only for that continuation, and `plan-release` a task this session stops working on. For work that is multi-step, likely to
span sessions or delegated, create a plan with `plan-create` and say so. Give a delegated or
separable task a `parent`, the paths it owns and its acceptance; hand a worker the task's
`plan-envelope --brief`, not the conversation. To run such tasks in parallel, `plan-assign`
each to a worker (isolated worktree, announced in the reply), have the worker start it with
the returned command, and `plan-integrate` it once completed; resolve reported conflicts in
the worktree, and escalate to the user what cannot be resolved. Honor effective
disables; do not automatically record receipts when `efficiency.attempts.enabled` is false.
Explicitly requested receipt commands remain available. Normal
`route`, `locate` and `session` calls can then reuse `--session` without repeating task JSON.
Keep the same task id across corrections and continuation; start a new id for a new outcome.
Rerun `task-start` with the same goal when stage or scope materially changes. Missing session
identity means limited operation: use bounded tools and a compact private handoff without
inventing an id or claiming reuse. Guidance-only hosts follow the same evidence discipline.

Distinguish the coordination workspace from the repositories involved. A workspace may be
one Git worktree or a non-Git folder containing several repositories. From a parent folder,
bind only the task's selected worktree roots with repeated `--repository <relative-path>`;
do not enroll every sibling. Keep one coordinator record, preserve each repository's
instructions and change boundary, and run its commands from that repository. Rebind when
repository scope changes. Combined workspace revisions identify a set of snapshots, not a
Git commit. Unavailable selected roots remain explicit.

When intermediate files are needed, use the bundled `workspace-open`/`workspace-write`
operations with the same session/task and a bounded operation scope. Reuse the returned
private scratch path instead of scattering Craft-owned files in arbitrary temp directories.
Keep scratch disposable; intentionally promote only useful bounded nonsecret evidence with
`workspace-promote`. Existing receipts/state need no duplicate files. Workers release their
own scopes with `workspace-release`; task completion cleans scratch when other owners are
inactive. Inspect retained/failed cleanup through session status. Never relocate host/tool
caches, infer authority from a path, or report cleanup without an observed result.

When selected guidance spans separately installed packages, derive their verified roots from
the loaded skill resources and repeat `--bundle <root>` on `context-report`, `task-step` and
session advice calls. Retain this root selection with the task; do not ask the user to locate
cache directories or search private host caches. A full export needs only its bundle root.
Missing or ambiguous installations remain explicit; accounting does not prove resource loading.

Use `craft-delivery-inspect-architecture` for a boundary that affects the solution and
`craft-delivery-plan-change` for material unresolved implementation choices. Use
`craft-delivery-route-work` to select independent practices and stack guidance when needed,
then `craft-delivery-apply-change` for the authorized implementation. Local work remains usable
without a remote tracker or model worker.

Before source edits, establish a safe change boundary through `craft-delivery-apply-change`,
respecting consumer branching/workspace policy, repository protection and unrelated work.
Applying this guidance does not require installing repository Git hooks.

Before implementation, carry forward the applicable accepted decisions and read the selected
engineering detail for the affected behavior. Select security/testing detail by the actual
concerns, carrying forward still-current selections. Reuse current evidence, but do not bypass this
step because the edit looks small. Use `craft-delivery-inspect-architecture` to settle missing
decision context; use `craft-delivery-write-adr` only when a consequential choice needs a record.
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
`craft-engineering-foundations` for bounded batching, worker returns and continuation detail.

Exercise the acceptance conditions with real checks. Structural validity does not establish
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

After correcting a recorded permission/tool/prerequisite blocker, use `task-step` with
`--outcome prerequisite-resolved` against its original stage and target, citing the actual
correction. This clears that blocker without claiming acceptance passed or granting new
authority. Preserve quality failures and other unresolved targets, then resume the checks.

At a meaningful acceptance boundary, use `task-step` to supply the stage, acceptance target,
approach, evidence identifier, observed outcome, brief result and next step. It records a
semantic receipt, maintains one bounded continuation and returns session advice. A shell exit
code alone does not establish acceptance; tool hooks never create these semantic receipts.
For an opted-in structured task, carry the criterion IDs into attributable evidence
records and use the current task/ledger digests when replacing evidence through `task-step`.
Assess the artifact and checked source before declaring verified support; an unavailable
check is not available evidence. Required gaps prevent structured completion; optional
gaps remain in the report. Rebind amendments and recheck changed sources instead of
refreshing old evidence identities. Legacy receipts remain agent declarations.
Use `--complete` only when the requested outcome is accepted and unresolved failures are closed.
This is a stage boundary, not a command to run after every tool call. Existing `session-advice`
and explicit receipt/checkpoint commands remain available for more detailed observations. Continue necessary authorized work; a suggestion is not an extra approval
gate. Use native context indicators; file-size estimates do not establish session occupancy.

For an optional action offer, explain its trigger, expected benefit and bounded scope, then
use the host-supported question mechanism. Continue independent authorized work while pending.
Record the real answer through `task-decision`; silence/timeout is not approval. Use an exact
existing user/standing authorization reference instead of asking again when scope is covered.
Declined/unchanged proposals stay suppressed. Immediately before an approved action, use
`task-action-check` with fresh source, policy and host observations. Its readiness is based on
caller declarations, never a permission grant or execution receipt. Use actual host controls
only when supported and authorized, then observe the result and verify integrated acceptance.
Missing native dispatch/capacity is an explicit limitation; finish useful work in main.
When a trusted controller adapter is connected, route nested assignments through the same
coordinator root, session and task ledger. Reuse observed idle workers where useful; a
completed worker still occupies capacity until the host reports release. An unknown dispatch
outcome requires reconciliation of its operation key before another attempt.

Feature usage comes from runtime receipts and controller observations. Normal delivery does
not inspect or classify feature usage; missing evidence stays unknown. Use `task-features`
and `task-feature` only when an explicit evaluation requests inspection or semantic classification.

Complete local delivery with the changed behavior, acceptance-to-check results and material
limits. Record generation/check results, relevant presentation review, local revision/commit
and remote delivery independently. Mark unavailable checks with the missing prerequisite;
hook success proves none of these outcomes. A failed push leaves successful local work intact
and remote delivery incomplete. Unknown usage stays unknown. If the user also requested an
external or installation action, continue with
`craft-delivery-workflow-ship-change` under that existing authorization and explicit target.

Package tools and shared assets: `../../../packages/craft-delivery` relative to this skill directory.
