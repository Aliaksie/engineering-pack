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

The 264 cases include an empty-input request for each of the 83 engine operations,
envelope failures, UTF-8 hashes, digest ordering, portable paths, literal discovery,
routing, continuation, acceptance projections, blocked attempts, checkpoint binding,
review publication plans, and worker/action lifecycle transitions. There are 160
successful responses across all 83 operations. Task cases preserve completion,
stale-source detection, scope reset and binding rejection. Grounding cases preserve
incomplete coverage and source provenance; session cases use explicit observations
and a fixed clock. Settings, model advice and service cases preserve precedence,
blockers and typed invocation plans without executing them. Context cases cover
selection attribution,
byte budgets and lifecycle delivery decisions. Hint cases cover clock reversal and
the exact expiry boundary. Repository cases cover normalized scopes, ordered checkout
identities, sorted snapshots and incomplete observations. Worker cases preserve
reservation identities,
duplicate refusal, uncertain dispatch, fenced release and cancellation. Action cases
preserve scoped decisions, stale-source refusal, durable claims and completion receipts.
These use synthetic host observations; they do not execute workers or publish reviews.
Empty-input rejection is not successful operation coverage; each migrated use case
still needs its own success, state and installed checks.

`cli-contracts.json` freezes 28 executable cases after verifying CLI, application,
platform, engine and review executable sources unchanged against the same baseline.
Run `npm run build` then `node --test tests/runtime-cli-replay.test.mjs` to replay
source and relocated AGENTS exports from disposable working directories with isolated
user configuration. These cover the command catalog, help precedence, repeated Unicode
file arguments, JSON/verbose routing, parser and required-input failures, review argument
rejection, and disabled Project Intelligence with an unavailable binary. Compare exit
status, stdout/stderr separation, structured results and native error codes; recorded
diagnostic prose is evidence only. These checks do not establish signal cancellation,
enabled native execution, automatic host hook dispatch or other platform support.

`host-contracts.json` freezes 120 host translation cases from the same baseline,
after verifying hosts and their runtime dependencies unchanged. Run `npm run build`
then `node --test tests/runtime-host-replay.test.mjs` for source and relocated
AGENTS bundle replay. Cases cover five hook-capable hosts and the guidance-only
AGENTS host, all seven lifecycle events, deny/context support, native response
schemas, Unicode, string tool arguments, alias precedence, invalid arguments and
session bounds. Optional undefined event fields compare as absent JSON fields.
These fixtures protect the thin adapter boundary; they do not establish automatic
hook dispatch, full application lifecycle, cancellation or native platform support.
