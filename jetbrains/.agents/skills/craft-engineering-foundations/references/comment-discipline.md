# Write the explanation readers need

Let readable code explain the sequence of operations. Use a comment for an obligation or
reason that is easy to violate: a protocol constraint, a compatibility exception, an ordering
requirement or a deliberate tradeoff. Keep the explanation next to the decision it governs.

```text
Pseudocode — assume the receiver atomically records the token with its effect:
operationToken = newOperationId()
# Reuse this identity after response loss; a new one could create another effect.
result = submit(payload, operationToken)
if result is Uncertain:
    result = lookup(operationToken)
```

The comment explains why the identity survives recovery; it does not narrate `submit`.
An uncertain lookup remains uncertain. Do not assume that a timeout undid the effect, or that
merely sending a token establishes receiver-side deduplication.

Check a comment against the full implementation and its callers. Remove misleading claims
and update examples when the contract changes. A TODO should identify useful unfinished work
and its condition or owner when known; avoid promises that no maintained process tracks.

Use canonical project documentation for cross-cutting decisions and link to it from local
code when needed. An ADR is useful for a consequential choice with durable alternatives or
constraints; ordinary code mechanics rarely need a separate record. Keep prose human-readable
and proportionate. Code and tests already document mechanics and observable contracts; avoid
prose that restates them. Internal planning paths, task IDs and execution checklists do not
belong in shipped comments or documentation. Explain the durable constraint in domain terms.
Update an existing canonical page before creating another, and add a document only when a
specific reader need remains unmet. Do not duplicate a large function or copy the same policy
across pages.
