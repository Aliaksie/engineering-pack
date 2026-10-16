---
name: plan-change
description: Turn a substantial requested change into executable steps with acceptance and verification evidence.
---

# Make the next change executable

A useful plan lets another implementer start without repeating the investigation. Begin with
the requested outcome and identify the decisions that still affect behavior. Reuse accepted
plans and current evidence instead of producing a new plan merely because this skill was loaded.

Inspect applicable instructions, the relevant implementation and real check commands.
Use `craft-delivery:inspect-architecture` for an unresolved boundary. Batch missing fact
lookups; a permitted economical worker may supply a compact, cited inventory, while the main
reasoner chooses the approach and assesses risks. Recheck sources affected by later edits.

Write only the detail needed for execution:

- State the observable success conditions and explicit exclusions from the request.
- Describe the approach and any alternative whose trade-off could change that choice.
- Order the implementation steps around actual dependencies; name likely paths and owners.
- Attach verification to outcomes, including material rejection, failure and recovery cases.
- Identify assumptions, blockers and the decision needed to remove each blocker.

When parallel work is worthwhile and authorized, separate independent assignments by outcome
and owned paths, name their dependencies and integration owner, and choose isolated workspaces
for concurrent writers. Keep overlapping changes sequential. Plan coherent commit boundaries
only when committing is requested; worker count does not determine commit count. Use the
available engineering Git/context references for worktree and handoff details.

For persisted data or public interfaces, account for old and new consumers during transition,
change ordering and the limits of recovery. Distinguish undoing a code edit from reversing an
external effect. Preserve durable rationale in `craft-delivery:write-adr` only when it needs
its own maintained record.

Keep a small plan in the conversation when sufficient. Use the requested file location;
otherwise `.plan/<slug>.md` can hold working notes, without linking them from shipped docs.
If the request authorizes implementation, proceed with `craft-delivery:apply-change` once
its dependencies are understood. A planning-only request ends with a reviewable plan.
