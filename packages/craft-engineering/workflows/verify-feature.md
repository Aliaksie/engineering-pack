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
check is `not-run`; unavailable execution is `blocked`, not passed. Evidence for another build
does not verify the target. Report findings, coverage and limits, and suggest the next useful
action without creating product work, publishing defects or claiming exhaustive correctness.
