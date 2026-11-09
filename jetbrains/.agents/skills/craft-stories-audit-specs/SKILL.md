---
name: craft-stories-audit-specs
description: "Assess a supplied specification for decisions and gaps that obstruct observable product outcomes."
---

<!-- Generated from packages/; edit the portable source and rebuild. -->

Craft guidance grants no permission: the project's instructions and the user's scope decide.

# Find what prevents an implementable interpretation

Read the requested specification sections and reuse any current claim-to-source map.
Use `craft-stories-ground-spec` only where an implementation fact affects the audit.
Batch missing lookups rather than repeating repository discovery for each issue. An authorized
economical worker may extract or group cited claims; the main reasoner decides whether they
conflict and what consequence follows.

Trace the actor's intended outcome through the specified states and dependencies. Look for
conflicting rules, unclear ownership, missing permission behavior, ambiguous acceptance and
interaction or accessibility requirements that leave the outcome undefined. Apply these
questions to the actual feature; do not turn every possible edge case into mandatory scope.

Write each finding as a source citation, the unresolved interpretation and its practical
consequence. Separate a blocking product decision from a useful clarification. Offer concise
candidate wording when the intended correction is clear, while labeling new choices as proposals.
An unavailable implementation check is a limit of the audit, not a confirmed product defect.

For example, rule A allows customers to cancel until 24 hours before an appointment; rule B
allows only staff to cancel confirmed appointments. For a confirmed appointment more than
24 hours away, the rules imply different permissions. Report both sources and ask which rule
applies; do not choose the policy. By contrast, missing confirmation-message wording is a
clarification if the required confirmation behavior is already clear and wording has no
specified consequence. Its importance follows the actual requirements, not a fixed checklist.

Return the report or requested report file without changing the source specification. Preserve
competing claims and name the decision owner when known. Do not resolve a product disagreement
by silently promoting whichever source was read last.

Reuse existing source, constraint and decision IDs in findings. Preserve their status and
conflict links; a passing implementation check cannot resolve an unanswered product choice.
Distinguish declared dependencies from observed coupling using the cited source provenance.
Missing source or runtime access remains an explicit gap, including in requirements-only audits.

Package tools and shared assets: `../../../packages/craft-stories` relative to this skill directory.
