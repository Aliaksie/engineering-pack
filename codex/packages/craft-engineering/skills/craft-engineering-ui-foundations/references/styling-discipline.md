# Preserve layout intent across conditions

Read the consumer's design tokens, spacing and component conventions. Make changes at their
owned source instead of stacking increasingly specific overrides. Scope rules so a local fix
does not silently alter another screen.

Choose layout from content behavior: wrapping, available space, intrinsic sizes and overflow.
Test narrow/wide viewports, long labels, translated text, zoom and empty or dense data. Avoid
fixed heights that clip valid content or position-only solutions that break reading order.

Example: a toolbar can wrap secondary actions while preserving the primary action's name and
keyboard order. At high zoom its controls should remain reachable without overlapping the title.
Use the project's breakpoints and responsive patterns rather than inventing a new scale.

Check focus/hover/disabled/error states and motion preferences. Keep styling changes separate
from unrelated formatting. A screenshot can reveal visual consequences, but verify real computed
layout and interaction when overlap or hidden content affects task completion.
