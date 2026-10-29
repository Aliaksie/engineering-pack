---
name: locate-code
description: Find scoped repository evidence with batched deterministic search and optional authorized extraction.
---

# Locate evidence efficiently

State the question and likely paths first. Search multiple related literal names in one bounded
pass rather than repeatedly loading files into the strongest model's context. Where available:

```text
<package-root>/bin/craft locate --root <project> --path src --term Dispatch --term receipt
```

From the source checkout, use `npm run locate -- ...`. On Windows the launcher is `bin\craft.cmd`.

For a structural question, ask it instead of a term: `--declaration <name>`, `--callers <name>`,
`--imports <target>`, `--section <heading>` or `--outline <document>`, with `--path` scopes as
usual. When the project enables evidence, Craft answers from verified evidence and searches the
sources it does not cover literally; otherwise it searches literally. The first output line names
the route and any gaps. Treat "absent within this coverage" as absence; any other empty answer is
not. Names are matched exactly as spelled; text inside bodies or prose stays a `--term` search.

With a task already bound by `task-start`, pass the verified `--session` id to reuse
current packets; no per-lookup task JSON is needed. Without a binding, use explicit session
and task identities when known, or perform a bounded fresh lookup and report reuse unavailable.
Do not fabricate session identity or create a new task for each search.
Configure file, match and byte budgets for the
question. Inspect truncation, skipped files and searched scope before treating no match as absence.
Literal evidence does not by itself identify the authoritative implementation.

Follow the most relevant definition to its caller, boundary test or configuration as needed.
Reuse current repository maps and earlier cited results. An economical worker may classify bounded
matches when delegation is supported and authorized; give it the question, source revision and
small evidence set. Return locations, concise relevant excerpts, coverage and ambiguity. Keep
architecture, correctness and product decisions with the main reasoner; do not copy the full parent
conversation into a lookup task. Escalate only when evidence or interpretation requires it.

Keep lookup workers read-only: they do not need a worktree merely to search. Do not switch
branches, stage files or edit the shared checkout as part of retrieval. If implementation is
needed, return that finding to the task owner for a separately scoped writing assignment;
parallel writers use deliberate workspace isolation.

Apply the same approach to ADRs and architecture notes. Find the consumer's decision index
and locations first, batch-search identifiers and domain terms, then read relevant records.
For an idea with unfamiliar terminology, let an authorized economical worker rank a bounded
candidate set with citations and uncertainty. The main task reads the candidates, checks status
and supersession, and decides applicability. Neither a literal miss nor a model ranking proves
that no decision exists; preserve coverage limits in the handoff.
