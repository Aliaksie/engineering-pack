# Validate meaning and control interpretation

For each external value identify its source, intended domain, size limits and eventual effect.
Parsing a valid shape does not establish authorization or business validity. Check the caller's
right to act on the selected resource, including tenant and ownership constraints.

Use the receiving interpreter's safe interface. Bind database values, construct structured
process arguments without shell interpolation and encode output for its actual HTML/URL/script
context. One generic escaping function cannot secure all sinks. Shell-free argv still permits
option injection; use an option terminator or explicit option/value contract where supported.

```text
Pseudocode — use the documented argument contract of the selected search tool:
require searchTerm fits the permitted input budget
result = execute(trustedSearchExecutable,
    argv = [literalPatternOption, optionTerminator, searchTerm, approvedDirectory],
    shell = false, timeout = searchBudget, outputLimit = resultBudget)
if result.meansNoMatches: return NoMatches
if result.failed: return SearchFailed
return Matches(result.stdout)
```

The executable and directory come from trusted configuration. The literal-pattern option,
terminator position and no-match exit status must match that tool; none is a universal process
API convention. Do not convert failed or truncated execution into an empty successful search.
The [Node/ripgrep example](../../foundations/references/languages/javascript.md#bounded-literal-search-in-node)
shows a concrete argument and exit-status contract.

For paths, define allowed roots and behavior for traversal, symbolic links and concurrent changes.
Lexical normalization alone does not establish containment on disk. Prefer APIs that preserve
validated ownership through the actual open/use operation; disclose remaining race assumptions.
For URLs, validate permitted schemes and destinations at connection time, including redirects and
private-address policies when server-side requests are allowed.

Enforce resource budgets before expensive parsing or expansion. Verify rejected values at the
real boundary and test interpreter-specific edge cases. Avoid claiming a reachable exploit solely
from a suspicious string when no protected sink or trust change exists.
