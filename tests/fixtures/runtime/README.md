# Frozen runtime contracts

`engine-contracts.json` records requests and responses from the JavaScript engine
at `09d74ead129008575f8483f68bd5412d2627c839`. The source was verified unchanged
against that revision when capturing the fixtures. Expected responses are static;
do not regenerate them from a replacement implementation.

Investigate suspicious baseline behavior and fix confirmed defects with regression
coverage before migration. For an intentional contract correction, update the affected
fixture explicitly and explain the correction; frozen output does not justify a known bug.

Run `node --test tests/runtime-replay.test.mjs` to replay them through the public
engine boundary. Keep this same corpus when replacing that boundary with Rust.
Compare JSON results, error codes and caller immutability. Diagnostic error
messages and explicitly listed diagnostic result paths are excluded from wording
comparison; diagnostic fields must remain strings. Echoed user descriptions and
identity material remain exact. Arrays retain order and absent fields differ from null.

The 213 cases include an empty-input request for each of the 83 engine operations,
envelope failures, UTF-8 hashes, digest ordering, portable paths, literal discovery,
routing, continuation, acceptance projections, blocked attempts, checkpoint binding,
review publication plans, and worker/action lifecycle transitions. There are 123
successful responses across 62 operations. Context cases cover selection attribution,
byte budgets and lifecycle delivery decisions. Hint cases cover clock reversal and
the exact expiry boundary. Repository cases cover normalized scopes, ordered checkout
identities, sorted snapshots and incomplete observations. Worker cases preserve
reservation identities,
duplicate refusal, uncertain dispatch, fenced release and cancellation. Action cases
preserve scoped decisions, stale-source refusal, durable claims and completion receipts.
These use synthetic host observations; they do not execute workers or publish reviews.
Empty-input rejection is not successful operation coverage; each migrated use case
still needs its own success, state and installed checks.
