---
name: react-engineering
description: "Implement and verify React interface behavior using the project’s rendering and state conventions."
---

<!-- Generated from packages/; edit the portable source and rebuild. -->

Read the shared [package instructions](../../AGENTS.md) when using this capability.

# React Engineering

Confirm the application actually uses React and identify its rendering mode, installed versions and design system. Describe the user action and visible outcome before selecting hooks or component structure. The same UI requirements may be implemented differently in another framework.

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
verification. When a reference materially informs the result, cite `react-engineering/<reference>`
so a reader can inspect it. Citation counts are not a quality measure.
<!-- craft-bar:end -->

## Choose detail

- [app-structure](references/app-structure.md): Boundaries and data flow.
- [component-patterns](references/component-patterns.md): Component contracts and effects.
- [state-management](references/state-management.md): State ownership and async freshness.
- [failure-and-loading](references/failure-and-loading.md): Loading, empty, stale and failed interactions.
- [accessibility](references/accessibility.md): Semantics, keyboard and focus.
- [styling-discipline](references/styling-discipline.md): Responsive layout and project tokens.
- [testing-react-components](references/testing-react-components.md): Interaction and integration checks.
- [visual-regression](references/visual-regression.md): Rendered comparisons and interpretation.

Return a proportionate decision or focused change, the evidence that supports it and any unresolved limitation. Opening this skill grants no authority to change external systems.
