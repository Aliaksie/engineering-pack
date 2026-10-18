# Choose structure from obligations

Start with the rule or outcome the code must uphold. List who supplies inputs, who owns
state and which callers depend on the result. A useful boundary makes those obligations
clear and gives independently changing work a place to live.

## Evaluate a proposed boundary

A module should have a coherent responsibility. Use SOLID as questions: can an implementation
replace another without surprising callers? Does an interface require irrelevant methods?
Can a volatile external dependency be substituted at a real seam? Can needed variation be
introduced without editing unrelated behavior? These questions do not require an interface,
container or inheritance hierarchy in every implementation.

Compare the proposed structure with a direct implementation. Name a present benefit such as
policy ownership, isolation of I/O, a test seam or an existing variation. Avoid speculative
extension machinery. Large files and deep nesting justify inspection; split when different
responsibilities or lifecycles can change independently. A cohesive parser may remain one
module even if it performs several closely related operations.

## Protect meaning

Matching fields do not establish a shared concept. Transport payloads, stored records and
public views can evolve under different owners. Map deliberately where semantics differ;
share a definition when its consumers must change together. Apply the same test to duplicated
logic: accidental similarity may diverge, but a shared authorization rule needs one policy.

```text
Pseudocode — define the public fields instead of exporting the storage record:
stored = { reference: "item-42", caption: "Draft", internalReviewNote: "needs follow-up" }
publicEntry = { reference: stored.reference, caption: stored.caption }
expect publicEntry == { reference: "item-42", caption: "Draft" }
```

The explicit projection keeps a new storage-only field out of the response unless the public
contract is deliberately changed. It does not authorize access to the record; that decision
belongs at the protected boundary. Check both allowed output fields and access separately.

Check installed dependencies and platform facilities before adding machinery. Explain an
actual semantic or operational mismatch when custom implementation is warranted. Verify
contracts and callers after restructuring, including failure behavior and resource ownership.
