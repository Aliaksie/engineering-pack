# Define a component contract

Describe inputs, visible states, events and ownership before adding hooks. Render from props and
state without performing external effects during rendering. Keep a controlled input's value and
change contract consistent; switching ownership midway can discard user edits.

Use effects to synchronize with an external system, not to compute values already derivable during
render. Include real dependencies and cleanup. Development-time repeated setup/cleanup can expose
an invalid lifetime; do not silence it by concealing dependencies. Memoization is a measured
performance choice, not a substitute for correct state flow.

Example: a subscription effect acquires a listener for the selected channel and returns the
matching unsubscribe operation. When the channel changes, verify that only the new channel updates
the view and no stale callback mutates state.

Use stable identity when rendering collections. Array positions can attach input state to the wrong
item after reordering. Preserve semantic element behavior when composing abstractions, including
refs, accessible names, disabled semantics and event propagation.
