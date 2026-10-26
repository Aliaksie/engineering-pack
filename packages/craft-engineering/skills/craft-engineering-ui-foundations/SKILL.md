---
name: craft-engineering-ui-foundations
description: "Frame user-interface behavior, interaction states and accessibility independently of framework."
---

<!-- Generated from packages/; edit the portable source and rebuild. -->

Read the shared [package instructions](../../AGENTS.md) when using this capability.

# Frame an interface change

Identify the user's action and observable result, including relevant loading, empty, rejected and
recovery states. Read the consumer's rendering model, navigation and design-system conventions.
Locate the owner of local input, URL state and remote data before adding another state store.

Preserve input during recoverable failures and reject stale asynchronous results. Check keyboard
operation, focus, names and responsive content behavior where the change affects them. Trusted
permissions remain at the server boundary even when the UI hides an unavailable action.

Use [React engineering](../craft-engineering-react-engineering/SKILL.md) only for a React implementation and the
applicable [language page](../craft-engineering-foundations/SKILL.md) independently. Other frameworks use their own
lifecycle mechanics while keeping the same user outcome. Verify real rendering for layout/focus
claims and distinguish that evidence from mocked component behavior.

Select shared detail by the affected behavior:

- [Accessibility](references/accessibility.md): semantics, keyboard and focus.
- [Styling](references/styling-discipline.md): responsive layout and project tokens.
- [Visual evidence](references/visual-regression.md): comparable rendered states and interpretation.

For QA without source access, use supplied requirements and observed interaction as the
basis. Missing application access leaves rendering and focus checks unexecuted; a plan
or screenshot alone cannot establish those outcomes. No framework is implied.
