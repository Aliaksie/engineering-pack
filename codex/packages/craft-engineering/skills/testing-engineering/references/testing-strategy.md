# Match the check to the claim

Write an observable acceptance statement before choosing a test. Identify what could invalidate
it: a local rule, integration contract, persistence behavior, browser interaction or deployed
configuration. Choose the least expensive boundary that can actually detect that failure.

A unit test can establish a calculation or state transition. A stubbed client establishes how
the caller reacts to the stub, not whether authentication, serialization or the remote service
works. Add focused integration or end-to-end evidence when those boundaries are part of the claim.
Avoid repeating the same assertion at every layer without a distinct failure it detects.

Use assertions that distinguish correct behavior from a plausible defect. Verify results and
important effects, including rejected access, failure recovery or duplicate handling where
relevant. Do not test exact private call sequences when several implementations satisfy the
contract. Tests written from the same mistaken assumption as the implementation need review.

```text
Claim: repeating a completed redemption does not consume a second credit.
Check: submit twice with one operation identity; inspect durable balance and both outcomes.
A mocked "save called once" assertion alone misses a crash between save and acknowledgment.
```

Run affected checks and required project gates. Confirm test discovery and report skipped,
quarantined or unavailable coverage. Introduce TDD when it helps investigate or specify the
change; reversible prose edits and mechanical low-risk changes do not automatically need tests.
