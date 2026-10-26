---
name: craft-stories-workflow-prepare-stories
description: "Produce locally reviewable stories from feature intent, grounded facts and unresolved product decisions."
---

<!-- Generated from packages/; edit the portable source and rebuild. -->

Read the shared [package instructions](../../../packages/craft-stories/AGENTS.md) when using this capability.

# Prepare stories without losing the source intent

Start with the supplied feature and any existing evidence or drafts. Where implementation
facts matter, use `craft-stories-ground-spec` on explicit sources. Pass its cited behavior,
revision and access limits to `craft-stories-audit-specs` rather than restarting discovery.
Use `craft-stories-write-stories` to turn the understood outcomes into implementable slices.

Carry one shared packet through grounding, audit and drafting. When a structured task
contract exists, reuse its source/criterion/decision IDs and current evidence. The optional
runtime's `bin/craft acceptance-view --view product` presents acceptance gaps, dependencies and unresolved
choices without another state store. Without runtime access, carry those distinctions in
the local packet. Keep unsupported interpretations conditional; a blocking decision affects
its slice while unaffected stories can proceed.

Check the resulting drafts against the source intent: acceptance conditions should be
observable, boundaries should make dependencies clear, and proposed UX or solution choices
should not masquerade as agreed requirements. Return concise drafts with the remaining
questions. Writing locally does not authorize publication or changes to the source specification.

Package tools and shared assets: `../../../packages/craft-stories` relative to this skill directory.
