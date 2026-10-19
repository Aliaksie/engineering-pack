---
name: craft-delivery-maintain-context
description: "Keep project instructions and maintained documentation accurate after a concrete change."
---

<!-- Generated from packages/; edit the portable source and rebuild. -->

Read the shared [package instructions](../../packages/craft-delivery/AGENTS.md) when using this capability.

# Repair the reader's map

Identify which reader-facing facts the change invalidated: entry points, commands,
configuration, ownership, public behavior or operational decisions. Start with the change's
existing evidence, then batch searches for those facts and their canonical locations.
A permitted economical worker can return cited candidate passages; the main reasoner chooses
what belongs in maintained documentation and writes the explanation.

Update the source of each fact. Regenerate generated sections through their supported tool
rather than editing rendered copies. Repair maintained indexes and links that now misdirect
readers. Keep always-loaded instructions brief and link conditional details from the place
where a reader needs them. Do not impose a new documentation hierarchy or settings policy.

Choose the lightest useful home: code for structure, tests for observable contracts, a nearby
comment for local rationale, and maintained docs for setup or cross-cutting explanations.
Use `craft-delivery-write-adr` when a consequential choice needs a separate rationale record.
Code and tests are already documentation of mechanics and observable contracts. Add a page
only for an unmet reader need; update an existing canonical explanation before creating another.
Do not generate task completion reports, per-step summaries or a parallel description of the
implementation as repository deliverables. A change that leaves documented facts accurate
does not require a prose edit.

Check the revised paths and commands against the actual project, distinguish a command that
was inspected from one that ran. Keep internal planning notes and session records out of
version control and shipped references, not just out of public navigation. Remove stale
plan-path/task-ID references from affected artifacts; preserve necessary rationale in domain
terms without copying the plan. Explicitly requested versioned plans are an exception.
Write for people: make the changed instruction easy to find, use short explanations and avoid
repeating implementation detail that will drift.

Package tools and shared assets: `../../packages/craft-delivery` relative to this skill directory.
