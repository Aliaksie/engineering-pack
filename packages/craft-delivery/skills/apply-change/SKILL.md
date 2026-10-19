---
name: apply-change
description: Make and verify a scoped source change in the target repository.
---

# Deliver the requested behavior

Establish what the user expects to change and inspect the existing working tree before
editing. Before implementation edits, inspect the branch and consumer-protected/default
branches; create or select a task branch under the existing local-work authorization. Use a
worktree when isolation is needed. Do not implement on a protected/default branch or detached
HEAD without an explicit task-specific exception. Preserve unrelated edits and follow the target repository's instructions, installed
versions and actual build configuration. Local request text is sufficient; a tracker is optional.

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
before resuming. When local commits are authorized by the user or standing consumer policy,
commit each completed, verified coherent unit and record its ID in the internal task record.
Use `craft-engineering:foundations` for Git/worktree discipline. Do not delay all checkpoints
until a long task ends, commit broken intermediate steps merely to mark progress, or combine
unrelated work. An explicit no-commit instruction keeps delivery as a reviewed working diff.
Without commit authorization preserve the diff and report that limit; do not invent permission.
Push, deployment and history rewriting require their own existing scope.
