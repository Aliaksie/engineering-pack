---
name: apply-change
description: Make and verify a scoped source change in the target repository.
---

# Deliver the requested behavior

Establish what the user expects to change. Before implementation edits, inspect branch, index,
working tree and relevant existing changes; establish a safe change boundary under consumer
branching/workspace policy and repository protection. Preserve unrelated work. Use a task branch
or worktree when policy, protection, isolation or concurrency requires it. Follow the target
repository's instructions, installed versions and actual build configuration. Local request
text is sufficient; a tracker is optional.

Use the current acceptance criteria as the implementation boundary. For a defect, compare the
expected and observed behavior and reproduce the failure when practical. If reproduction is
not available, identify the source or experiment supporting the diagnosis and its limits.
A small, understood edit can proceed directly. Resolve substantial uncertainty with
`craft-delivery:plan-change` or `craft-delivery:inspect-architecture` rather than guessing.

Before the first edit, establish the applicable project constraints. Reuse a current decision
map or batch-find the relevant ADR index and records; read the decisions that govern the
affected contract or boundary, including status, scope and consequences. A small diff can still
violate an accepted decision. Proposed records are alternatives, and superseded records supply
context rather than the current rule. Resolve a material conflict through the consumer's
decision process before changing that constrained behavior; continue independent work.
Missing ADRs do not require a new record or block an otherwise understood edit.

Select applicable engineering guidance by concern: common principles, language, runtime,
application layer and provider are separate inputs. `craft-delivery:route-work` can produce
a bounded selection. Read the relevant selected skills and references before editing the
behavior they govern; selecting a name or reading an index is not reading its detailed guidance.
Reuse already-read content when its source and scope remain current. Repository policy remains
authoritative for project choices. If a suggested
package is absent, work from available project standards and report the gap.

Implement a coherent change, then exercise the behavior it could break. Keep internal plans,
checklists and session records out of the staged diff and shipped references. Code and tests
already explain mechanics and contracts; add prose only for a reader need they do not meet. Prefer existing
checks; add a regression when it demonstrates a meaningful failure or contract. Test-first
work is useful when it clarifies behavior, not a required artifact for every edit. Choose
failure, permission, compatibility or concurrency cases when those risks actually change.
Use each check's real arguments and working directory, preserve its exit status and separate
pre-existing failures from regressions.

Inspect the final diff for accidental scope, obsolete comments and changed user-facing facts.
Use `craft-delivery:maintain-context` where documentation became inaccurate and
`craft-delivery:review-code` for a requested review or a risk that merits one. A correction
invalidates evidence for the affected earlier version; repeat the affected check, not an
unrelated full campaign.

Finish with the behavior delivered, acceptance evidence, files or interfaces affected and
remaining limits. If interrupted, preserve a compact progress note and verify the checkout
before resuming. Follow consumer commit strategy and existing user authorization or standing
consumer policy; implementation alone does not authorize commits. When commits are authorized
and useful, keep them coherent and verified and record their IDs privately. Otherwise preserve
a reviewable diff. Use `craft-engineering:foundations` for Git/workspace and commit detail.
Push, force-push, history rewriting, merge and deployment require their own existing scope.
