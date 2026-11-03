---
name: implement-story
description: "Implement a supplied story while tracing its acceptance criteria to actual checks."
---

<!-- Generated from packages/; edit the portable source and rebuild. -->

Read the shared [package instructions](../../AGENTS.md) when using this capability.

# Carry a story into code

Obtain the story from the supplied text or file. For a remote reference, use the consumer's
configured read through `craft-delivery:resolve-service`; there is no default tracker.
Unavailable access can be handled with pasted text rather than inventing task details.

Extract the actor, desired outcome, agreed acceptance conditions and open product decisions.
Separate a required solution constraint from an illustrative implementation suggestion.
Turn vague success claims into observable checks, retaining uncertain interpretations as
questions or explicit assumptions. Continue unaffected work when a question blocks only one part.

Use `craft-delivery:apply-change` for implementation. Keep a short criterion-to-evidence map:
what was changed, how it was checked and what remains unverified. Do not silently expand the
story to unrelated improvements or mark an unresolved product choice complete. Updating a
tracker status or posting the result is a distinct action requiring authorization.
