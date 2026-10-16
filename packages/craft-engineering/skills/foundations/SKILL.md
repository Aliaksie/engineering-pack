---
name: foundations
description: Make cross-language design and change-scope decisions using the consumer’s actual constraints.
---

# Foundations

Identify the behavior to preserve, the intended improvement and the affected ownership boundary. Use the language reference only for the language actually changed; a file extension does not identify its runtime. Keep code as the authoritative explanation of mechanics, and document consequential rationale where readers need it.

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
verification. When a reference materially informs the result, cite `foundations/<reference>`
so a reader can inspect it. Citation counts are not a quality measure.
<!-- craft-bar:end -->

## Choose detail

- [design-principles](references/design-principles.md): Responsibility, contracts and justified structure.
- [surgical-edits](references/surgical-edits.md): Focused edits and evidence for structural change.
- [naming-conventions](references/naming-conventions.md): Vocabulary and public identifier compatibility.
- [comment-discipline](references/comment-discipline.md): Useful rationale and concise maintained documentation.
- [git-discipline](references/git-discipline.md): Coherent commits, message conventions, worktrees and integration.
- [context-discipline](references/context-discipline.md): Bounded evidence gathering, worker assignments and reusable handoffs.
- [infrastructure](references/infrastructure.md): Resource lifecycle, access and recovery.
- [execution-optimization](references/execution-optimization.md): Measured context, cache and model choices.

For language mechanics choose only the relevant page: [JavaScript](references/languages/javascript.md), [TypeScript](references/languages/typescript.md), [Python](references/languages/python.md), [JVM](references/languages/jvm.md), [Go](references/languages/go.md) or [C#](references/languages/csharp.md).

Return a proportionate decision or focused change, the evidence that supports it and any unresolved limitation. Opening this skill grants no authority to change external systems.
