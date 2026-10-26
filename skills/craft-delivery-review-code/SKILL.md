---
name: craft-delivery-review-code
description: "Review an explicit code-change scope directly or through revision-bound review tooling."
---

<!-- Generated from packages/; edit the portable source and rebuild. -->

Read the shared [package instructions](../../packages/craft-delivery/AGENTS.md) when using this capability.

# Choose the evidence that matches the scope

Identify whether the user wants committed base/head changes, staged edits, unstaged edits,
untracked content or a stated combination. Do not let a tool's preferred input silently narrow
the requested review.

For committed revisions, use `craft-review-review-change` when available. Its immutable snapshot
covers those revisions, not later working-tree edits. For uncommitted work, inspect the actual
index and working tree, including relevant untracked files. Record HEAD and the inspected scope
without describing that evidence as an immutable snapshot. Never create a commit to make a
review tool usable unless committing is already authorized.

Select affected module and stack context with `craft-delivery-route-work` when useful.
Resolve an architectural constraint through `craft-delivery-inspect-architecture` only when
it changes the review conclusion. If a companion package is unavailable, review directly
against project instructions and available engineering guidance, noting the limitation.

Report actionable findings with a location, triggering condition and concrete consequence.
Keep uncertain risks and verification gaps distinguishable from demonstrated defects. An empty
finding list does not certify behavior that was never exercised. Review does not itself grant
permission to modify source or publish comments.

Package tools and shared assets: `../../packages/craft-delivery` relative to this skill directory.
