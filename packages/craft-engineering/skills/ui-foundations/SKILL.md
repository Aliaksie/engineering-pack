---
name: ui-foundations
description: Frame user-interface behavior, interaction states and accessibility independently of framework.
---

# Frame an interface change

Identify the user's action and observable result, including relevant loading, empty, rejected and
recovery states. Read the consumer's rendering model, navigation and design-system conventions.
Locate the owner of local input, URL state and remote data before adding another state store.

Preserve input during recoverable failures and reject stale asynchronous results. Check keyboard
operation, focus, names and responsive content behavior where the change affects them. Trusted
permissions remain at the server boundary even when the UI hides an unavailable action.

Use [React engineering](../react-engineering/SKILL.md) only for a React implementation and the
applicable [language page](../foundations/SKILL.md) independently. Other frameworks use their own
lifecycle mechanics while keeping the same user outcome. Verify real rendering for layout/focus
claims and distinguish that evidence from mocked component behavior.
