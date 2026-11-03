---
name: workflow-review-change
description: "Coordinate source identification, regression investigation and review-result checks for a requested change."
---

<!-- Generated from packages/; edit the portable source and rebuild. -->

Read the shared [package instructions](../../AGENTS.md) when using this capability.

Establish the requested comparison with `review-change`, then select only the engineering
references needed for its languages, layers and risks. Gather diff and caller evidence in
bounded batches. Use an immutable committed snapshot when it represents the requested scope;
inspect dirty-tree work directly when it does not.

Reuse `craft-engineering:locate-code` for bounded discovery when installed. Check applicable
accepted decisions and, where supplied and accessible, linked task criteria, contracts, CI
results for the reviewed head and unresolved prior findings. A missing tracker or companion
skill does not stop direct review; disclose consequential gaps. Assess the raw change rather
than inheriting the implementation author's conclusion.

Investigate candidate regressions and return supported findings, unresolved questions and
actual check outcomes. The report classifies issues high/medium/low and recommends approval,
changes or further verification. An empty list alone does not satisfy required checks. Validate structured
snapshot results with the bundled checker. A changed head invalidates affected claims and requires renewed inspection. Hand a draft to
an authorized publisher only when posting is in scope; keep the current revision identity
with that handoff.
