# Make failures reproducible

Choose fixtures that exercise meaningful states and boundaries without private data. Keep each
test's state owned by that test: temporary directories, database rows, ports and clock control
need isolation. A shared mutable fixture can create a failure dependent on execution order.

Prefer explicit synchronization to arbitrary sleeps. Wait for the event or condition the test
needs and give the wait a bounded timeout with useful diagnostics. Use a controlled clock for
calendar and expiration rules; use the real relevant scheduler when investigating a race.
Seed generated data and retain the seed when a case fails.

```text
Example fixture set for a reservation: zero capacity, one free place, already committed token,
and a record expiring exactly at the supplied clock value. Each fixture owns its identifiers.
```

When a test flakes, preserve the first failure and its environment. Compare order, parallelism,
time, external dependencies and cleanup behavior. A rerun can supply evidence; it does not erase
a failed attempt. Quarantine or deletion is a visible project decision with a reason and scope,
not a way to report a clean result.

Cleanup should target only resources created by the test. Protect temporary-root ownership before
recursive deletion and report cleanup failures without hiding the primary error. Verify that
fixtures remain representative when the production contract changes.
