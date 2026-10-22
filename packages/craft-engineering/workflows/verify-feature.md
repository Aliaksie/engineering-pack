---
name: verify-feature
description: Verify existing behavior through manual checks, existing automation or explicitly requested test-code changes, without assuming a product-code fix.
---

# Verify the requested behavior

Establish the acceptance claim, target build/revision, environment and permitted effects.
Choose the mode from the request and available evidence; if unclear, prepare a bounded
verification plan and resolve only the choice that blocks execution. A feature description
is a test basis, not an instruction to implement it. A local request is sufficient; a PR/MR
is optional context and must identify the revision it contributes.

- **Manual:** plan or perform authorized observations against the application. Source code
  and a runner are optional. Use `craft-engineering:workflow-explore-testing` for exploratory
  work; criteria-based checks need not become an exhaustive exploration campaign. Test data,
  account changes and remote actions still require the existing scope.
- **Existing automation:** inspect the actual configured checks and run only the relevant
  authorized commands. The `run-check` route returns declared argv, not execution evidence.
  Preserve failures and skipped/missing coverage; distinguish an assertion failure from an
  unavailable environment. Do not silently alter tests or weaken assertions to get a pass.
- **Test code:** when explicitly requested, change tests and necessary scoped fixtures/helpers
  or runner configuration. Use project conventions and the relevant testing/language detail;
  do not load unrelated implementation guidance. Check that the test distinguishes the
  accepted behavior from a plausible defect, then run the affected verification.

Keep product-code fixes outside this testing-only scope. A discovered defect becomes an
evidence-backed finding with reproduction and impact. If product changes are separately
authorized, make that expanded boundary explicit before using the coding delivery flow.

Use `craft-engineering:testing-engineering` for affected boundary, flakiness or debugging
decisions. Retain one compact evidence record per acceptance claim: criterion, build/revision,
environment, check or observation, observed result, artifact and coverage limit. A proposed
check is `not-run`; unavailable execution is `not-available` (the task may be blocked). Evidence for another build
does not verify the target. Report findings, coverage and limits, and suggest the next useful
action without creating product work, publishing defects or claiming exhaustive correctness.

Carry supplied criterion IDs through the charter, each probe and the final findings.
Map a probe to its affected flow/risk, expected requirement or explicit question,
preconditions and test-data assumptions, actions, observations and retained artifact.
Inspect configured automation and what it asserts; filenames and a green suite do not
prove affected-flow coverage. Prioritize changed transitions and adjacent recovery or
permission flows, and state what remains unexplored.

Keep implementation and QA evidence together under the same criterion IDs. Preserve a
contradictory result with both reporters and artifacts; do not silently select a pass.
Bind observations to the actual build and environment, including relevant configuration.
For a non-Git application, record an explicit build/version-and-environment identity;
missing identity leaves freshness unknown. A changed build invalidates earlier evidence.
An unexecuted probe is `not-run`; missing execution access is `not-available` with its
prerequisite (the task may be blocked). Neither is passed. Do not copy a current identity
onto an old observation. Keep observations in artifacts, not invented execution receipts.

The optional runtime's `qa-projection` operation reads the existing contract and evidence
ledger plus bounded probe detail; it neither executes probes nor persists a separate QA
state. Without runtime access, retain the same facts in the charter/report. Required
manual acceptance uses declared manual checks; automation does not certify a different
manual observation. Testing-only scope grants no product edits or remote mutations.
