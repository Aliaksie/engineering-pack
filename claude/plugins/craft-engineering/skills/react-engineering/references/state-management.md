# Give each fact one authoritative owner

Classify a value as local interaction state, URL/navigation state, shared client state or remote
server state. Keep it where the lifetime and consumers require it. Derive totals and selections
when possible instead of synchronizing duplicate copies with effects.

For asynchronous work identify which request still owns the result. Abort superseded work where
supported and reject stale responses before applying them. Cancellation alone may not prevent a
previous completion. Define optimistic update rollback/reconciliation and preserve independent
user edits while data refreshes.

```text
Timeline: select parcel A -> request A starts -> select parcel B -> request B completes
-> request A completes. The rendered parcel remains B, including errors and loading state.
```

Keep credentials and tenant-sensitive cached data inside the consumer's approved security model.
A global store must not accidentally share server-rendered request data between users. Verify
navigation, rapid input changes, retries and unmounting, rather than only the first successful load.
