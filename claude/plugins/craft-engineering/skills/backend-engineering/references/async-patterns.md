# Give concurrent work an owner

Identify which task waits for completion, owns failure reporting and cancels child work.
Await owned tasks or use an explicit durable background mechanism; detached work can outlive
its request, lose errors and hold disposed resources. Completion must mean what callers expect.

Bound active operations and queued demand separately. A concurrency semaphore does not bound
an unbounded backlog. Choose rejection, backpressure or durable queuing according to the caller
contract. Release locks, permits and connections on every exit path, including cancellation.

Protect shared invariants across the complete read/decide/write operation. A thread-safe map
does not make a multi-step update atomic. Prefer suitable transactions, atomic conditional
updates or narrowly owned synchronization; document lock ordering when several locks are needed.

```text
Pseudocode — reserve a positive number of whole units:
require requested is an integer within the permitted positive range
result = store.decrementIfEnough(accountId, requested)
if result.updatedRows == 0: return NotReserved
return Reserved
```

Here `decrementIfEnough` means one datastore operation that checks the balance and decrements
it atomically. A client-side read followed by a write is not equivalent. Zero updated rows
means no reservation was made; it does not distinguish a missing account from insufficient
balance. This operation alone does not deduplicate requests. See a runnable
[SQLite example in Python](../../foundations/references/languages/python.md#atomic-reservation-with-sqlite).

Model duplicate and out-of-order delivery. Persist an operation identity with the protected effect
where possible, and define replay results. Check crash windows between reservation, effect and
acknowledgment. A lease expiry does not prove an old worker stopped; use fencing or a contract
that rejects obsolete ownership where concurrent effects would violate safety.

Verify contention, overload, cancellation and restarts, not just a successful parallel run.
Language references supply concrete task, synchronization and cleanup mechanics.
