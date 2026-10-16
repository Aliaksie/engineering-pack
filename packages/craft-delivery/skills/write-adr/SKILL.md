---
name: write-adr
description: Record the rationale for a consequential architecture choice in a concise repository-native ADR.
---

# Preserve the reason for a choice

Decide what the reader would lose without a record. Code and tests already expose many local
implementation details; comments can explain a nearby surprise. An ADR earns its place when
a durable contract, dependency, boundary or costly trade-off needs rationale beyond those
artifacts. Do not create one for each routine edit. Honor an explicit ADR request and identify
an overlapping record before drafting a competing decision.

Batch discovery of the local template, decision index, related records and relevant source
constraints. Reuse current architecture evidence. An authorized economical worker may return
cited excerpts, identifiers and statuses; keep the documentation decision, reasoning and
trade-off analysis with the main reasoner. Never manufacture consultation, benchmark results
or alternatives that nobody evaluated.

Follow repository conventions first. With no template, a compact record can use:

1. **Title and status:** the specific choice and whether it is proposed, accepted or superseded.
2. **Context:** the problem and constraints that make a decision necessary.
3. **Decision:** the selected approach, its scope and the reason it fits.
4. **Alternatives:** credible options and the relevant reason for declining or deferring them.
5. **Consequences:** benefits, costs, risks, follow-up obligations and reconsideration triggers.

Keep a record centered on a decision that can be understood and revisited as a unit. A simple
choice often fits roughly one page; that is a reader-oriented heuristic, not a quota. Use short
paragraphs, familiar terms and links to detailed evidence. Avoid copying code walkthroughs,
rollout plans or reference manuals into the rationale. Split independently revisable choices
when that improves comprehension, not merely to meet a length target.

Use the next available local identifier without renumbering existing records. Default a new
choice to proposed unless acceptance is established by the responsible user or project process;
editorial corrections do not change an existing status. Update an existing index when present.
For a changed accepted decision, follow local supersession rules and retain links explaining
both the earlier rationale and the new choice. Do not rewrite the earlier decision as though
it was never taken.

Before returning the record, check it as a reader: can a teammate identify the choice, why it
was needed, its main trade-off and its consequences without reconstructing the conversation?
Revise unexplained jargon, repeated sections and material that obscures that answer. Link
supporting detail rather than growing the record into a manual. Verify status and index links;
a readable draft still must not present an unaccepted choice as settled.

Return the record path, status and principal unresolved question. If no record is warranted,
point to the smallest durable home for the rationale instead of adding documentation ceremony.
