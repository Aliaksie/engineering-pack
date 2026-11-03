# State the distributed invariant

Name the fact that must remain true and the actors that can change it. Determine whether reads
may be stale, whether updates can conflict and which authority decides order. A deployment across
processes does not supply a consistency guarantee.

Choose consistency by the business contract: a cached availability hint may tolerate staleness,
while allocating the final unit may require an authoritative conditional write. Record the
observable consequence of a partition or unavailable owner instead of promising both immediate
availability and a globally current answer without supporting mechanisms.

Use stable identifiers, version checks and explicit conflict handling where updates can race.
Wall-clock timestamps may not establish causality; clocks skew and events arrive late. Define
per-key ordering needs and handling for missing, duplicated or reordered messages.

```text
Failure timeline — the same transfer is delivered again after a crash:
A. In the debit store's transaction, claim transferId, debit once and append an outbox event.
B. Commit. A crash here leaves the event available for a later publisher.
C. Publish; the publisher may send the event more than once.
D. In the credit store's transaction, claim transferId and credit only on its first claim.
E. If either side remains pending, reconcile from durable records rather than guessing success.
```

The claims require durable uniqueness and atomic association with the local effect. Reusing
an ID with a different transfer payload must be rejected. This sketch illustrates recovery;
it does not make the two stores one transaction or guarantee eventual completion without a
working retry/reconciliation path.

A transaction covering one store does not atomically commit a second store or remote service.
For multi-resource work choose a supported transaction, durable outbox/inbox, reconciliation or
compensating action. Compensation is a new domain action and may fail; record progress and an
operator path. Test interruption at each durable boundary and report unresolved uncertainty.
