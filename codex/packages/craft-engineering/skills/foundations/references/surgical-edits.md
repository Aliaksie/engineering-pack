# Plan the smallest coherent edit

Inspect the affected source, its callers and accepted local conventions. Describe the
required behavior and distinguish it from nearby cleanup. Preserve unrelated user work,
formatting policy and compatibility constraints.

A small diff is useful when it makes review easier; it is not the sole goal. Follow a
contract change through all necessary callers, schemas and checks. Avoid opportunistic
renaming or reformatting that obscures the change. If a tool rewrites a broad file, inspect
that output and narrow avoidable churn before proceeding.

For a move, split or extraction, state what responsibility or dependency improves and which
behavior should stay stable. Check exported names, imports, discovery paths, ownership and
lifetime assumptions. An extraction that changes initialization order or exception behavior
needs explicit treatment even if the business expressions are unchanged.

Use the repository's available edit and formatting tools. Verify the affected boundary and
inspect the final diff for accidental omissions, generated output and unrelated files.
Report actual checks and any pre-existing failure that prevented completion.
