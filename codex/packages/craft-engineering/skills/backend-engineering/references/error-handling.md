# Define failure at the caller boundary

Write down successful, rejected and uncertain outcomes for the operation. Separate invalid
input, forbidden action, dependency failure and a lost response after a possible effect.
A generic exception type or status code cannot settle whether repeating an operation is safe.

Give calls a deadline consistent with the caller's remaining budget. Decide how cancellation
propagates and what work may already have committed. Retry only failures the dependency contract
allows, with bounded attempts, elapsed time and delayed/jittered scheduling where appropriate.
Avoid multiplying independent retry loops across client, service and queue layers.

```text
Timeline — a receipt is lost:
receiver commits reservation -> connection closes -> sender sees timeout
The sender must reconcile using a stable operation identity before creating another reservation.
```

Keep domain outcomes usable by callers and map them to safe external messages at the boundary.
Preserve diagnostic causes without leaking tokens, private data or internals. Log deliberately
at the owner of a failure; repeated logging at every layer creates noise rather than clarity.

Specify recovery for partial writes and unknown results. Test an effect occurring before an
error, exhaustion of the retry budget and cancellation during work. A fallback must preserve
the domain contract; silently returning an empty result can turn an outage into false success.
