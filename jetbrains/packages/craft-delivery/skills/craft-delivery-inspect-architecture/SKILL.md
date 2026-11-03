---
name: craft-delivery-inspect-architecture
description: "Locate the ownership, decisions and contracts needed to answer a specific architecture question."
---

<!-- Generated from packages/; edit the portable source and rebuild. -->

Read the shared [package instructions](../../AGENTS.md) when using this capability.

# Establish the relevant boundary

Frame the question before searching: which component owns the behavior, which contract must
remain compatible, or which decision constrains the proposed change? Use the repository
locations supplied by the user and explicitly linked project sources. Do not search unrelated
checkouts or assume an organization's repository layout.

Consult applicable instructions and the nearest maintained architecture entry point.
If `.craft/project.json` declares a relevant module, treat its owner, dependencies
and checks as leads to verify against the current tree. Follow the actual call, data or
ownership boundary only as far as the question requires.

Gather related decision records, contract declarations and test entry points in one bounded
search batch. Reuse a current evidence packet from earlier work. Deterministic lookup should
precede model interpretation; an authorized economical worker can extract paths, statuses and
short cited passages from scoped results. Give it the question, source revision and limits,
not the parent conversation. Keep architectural conclusions and conflicting evidence on the
main reasoning thread.

For decisions, start with the consumer's index or declared decision locations. Search relevant
identifiers and several domain terms together with the bounded `locate` command. If the idea uses
different terminology, an authorized economical worker may propose candidates from a bounded
set of titles, summaries or records, with citations and uncertainty. That is candidate retrieval,
not an architecture decision. Open the applicable records before concluding which constraints
bind the change; a title or search snippet is insufficient.

Carry the applicable decision's path, status, affected scope and constraints into planning or
implementation. Follow the consumer's status conventions and supersession links: a proposal is
not accepted policy, and a superseded choice is not the current constraint. Surface contradictions
between accepted decisions, instructions and code rather than silently choosing one. No matching
record within a limited search establishes only that search's coverage.

Return a small boundary brief: the answer, supporting paths, declared owner if known,
applicable decision status, and any contradiction or missing evidence. Mark inference as
inference. A proposed record describes a candidate choice; implementation describes what
exists. Neither establishes acceptance of the other. Missing documentation is a discovery
limit, not a reason to invent a decision or create an ADR automatically.

When carrying grounding into a structured task context, keep stable source IDs, source
identity, observed time and affected scope with each interpreted constraint. Preserve
explicit conflicts and unknown coverage. A source hash identifies inspected bytes;
it does not accept an ADR, verify a declared check or authorize its execution.
Reuse bounded lookup observations without promoting literal matches into requirements.

When the question needs a view of an existing contract, the optional runtime's
`bin/craft acceptance-view --view architecture` selects boundary, dependency and operational-consequence constraints by ID.
It preserves source provenance, ADR status, conflicts and evidence gaps without discovering
new facts. Keep declared design, observed coupling and inference distinct; cite mixed bases
individually. With no runtime or code access, give the same brief from supplied facts and
mark unobserved boundaries explicitly. Empty selections do not prove absence, and neither
this view nor passing checks accepts a proposed ADR or authorizes its creation.
