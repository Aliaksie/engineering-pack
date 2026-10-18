---
name: prepare-stories
description: Produce locally reviewable stories from feature intent, grounded facts and unresolved product decisions.
---

# Prepare stories without losing the source intent

Start with the supplied feature and any existing evidence or drafts. Where implementation
facts matter, use `craft-stories-ground-spec` on explicit sources. Pass its cited behavior,
revision and access limits to `craft-stories-audit-specs` rather than restarting discovery.
Use `craft-stories-write-stories` to turn the understood outcomes into implementable slices.

Carry a shared packet of confirmed requirements, source conflicts, assumptions and open
choices throughout the work. Keep an unsupported interpretation conditional. If one product
decision blocks a slice, make that question explicit and proceed with unaffected stories.

Check the resulting drafts against the source intent: acceptance conditions should be
observable, boundaries should make dependencies clear, and proposed UX or solution choices
should not masquerade as agreed requirements. Return concise drafts with the remaining
questions. Writing locally does not authorize publication or changes to the source specification.
