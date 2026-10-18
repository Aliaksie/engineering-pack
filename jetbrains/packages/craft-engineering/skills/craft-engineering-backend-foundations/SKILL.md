---
name: craft-engineering-backend-foundations
description: "Establish backend operation contracts and state ownership before changing service behavior."
---

<!-- Generated from packages/; edit the portable source and rebuild. -->

Read the shared [package instructions](../../AGENTS.md) when using this capability.

# Frame a backend change

Identify the caller, authenticated actor, state owner and externally visible outcome. Read the
consumer's transport, persistence and runtime configuration without inferring a framework or cloud.
Describe what must remain true across rejection, timeout, duplicate requests and partial effects.

Choose deeper [backend engineering](../craft-engineering-backend-engineering/SKILL.md) guidance for the actual risk:
failure recovery, concurrent work, distributed state, telemetry, boundaries or performance. Add the
relevant [language page](../craft-engineering-foundations/SKILL.md) independently. A local calculation may need none
of the distributed-system machinery.

Check validation and authorization before protected effects. Identify the transaction or conditional
write that upholds a durable invariant. For an external action, define effect identity, cancellation
and reconciliation when the response is lost. Verify the smallest useful boundary and report where
integration or deployment evidence remains unavailable.
