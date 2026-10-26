# Change resources through a recoverable lifecycle

Identify the consumer environment, desired configuration and observed resources separately.
Read the actual source of configuration and ownership boundaries before proposing a change.
Include stateful data, identities, network reachability and dependencies in the affected scope.

Review a concrete plan or diff for creation, replacement and deletion. Check who may apply it,
which account and region are targeted and whether secrets are referenced without being embedded.
Defaults suitable for a sample are not a production access policy. Detect drift rather than
silently overwriting a resource owned elsewhere.

Plan compatibility across rollout stages: old/new application versions, schema changes, capacity
and readiness. Bound disruption and establish an observable stopping condition. A rollback of
configuration may not undo a destructive data migration; identify restore or forward recovery
and verify backups against the required recovery point and time.

```text
Rollout example — move a stored value from oldField to newField:
1. Expand the schema; keep old readers and writers working.
2. Deploy compatible readers and a supported synchronization/dual-write path.
3. Backfill newField without overwriting newer values; record failures for retry.
4. Retire old-only writers and catch up changes made during the backfill.
5. Verify convergence and switch readers; observe the agreed recovery window.
6. Remove oldField only after old consumers are gone and recovery is settled.
```

This is a rollout sequence, not executable pseudocode. Two independent writes are not
inherently atomic: choose a transaction, change capture or durable reconciliation according
to the store. Backfill needs a conflict/version rule, and cleanup must wait for convergence.
The same ordering does not fit every schema change; identify the invariant at each stage.

Run appropriate static/plan checks locally. Apply or deploy only within the user's authorized
targets, then inspect actual state and service health. Report what was planned versus executed.
