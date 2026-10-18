---
name: craft-engineering-security-engineering
description: "Assess changed trust boundaries, authorization decisions and interpreter inputs across software stacks."
---

<!-- Generated from packages/; edit the portable source and rebuild. -->

Read the shared [package instructions](../../../packages/craft-engineering/AGENTS.md) when using this capability.

# Security Engineering

Draw the path from an externally controlled value or identity to a protected effect. Identify where trust changes and which component can enforce the rule. Base findings on a reachable failure or applicable policy; terminology alone is not evidence of a vulnerability.

**Which references bind a change:** Select the pages whose contracts or risks are affected; size alone does not determine applicability. Read their decision criteria before relying on them.

<!-- craft-bar:begin — generated from packages/craft-engineering/standards/craft-bar.md. Edit the source, not this block. -->
## Applying the references

Choose a reference because the current task needs its decision criteria. Read that page
before using its detailed advice; an unchanged copy already in context can be reused.
The index is a menu, not a checklist to load in full.

Establish the consumer's constraints from its accepted instructions, configuration and
contracts. Compare those constraints with what the code actually does. Report disagreement
instead of converting a prevalent pattern into a requirement. Examples illustrate an idea;
they do not select the consumer's language, runtime or provider.

Connect a material recommendation to the affected behavior, supporting evidence and useful
verification. When a reference materially informs the result, cite `security-engineering/<reference>`
so a reader can inspect it. Citation counts are not a quality measure.
<!-- craft-bar:end -->

## Choose detail

- [input-sanitization](references/input-sanitization.md): Domain checks and interpreter-specific handling.
- [browser-trust-boundary](references/browser-trust-boundary.md): Sessions, rendering and trusted enforcement.
- [ai-code-safety](references/ai-code-safety.md): Untrusted model context, generated code and tool authority.

Return a proportionate decision or focused change, the evidence that supports it and any unresolved limitation. Opening this skill grants no authority to change external systems.

Package tools and shared assets: `../../../packages/craft-engineering` relative to this skill directory.
