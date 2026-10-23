# Frozen runtime contracts

`engine-contracts.json` records requests and responses from the JavaScript engine
at `09d74ead129008575f8483f68bd5412d2627c839`. The source was verified unchanged
against that revision when capturing the fixtures. Expected responses are static;
do not regenerate them from a replacement implementation.

Run `node --test tests/runtime-replay.test.mjs` to replay them through the public
engine boundary. Keep this same corpus when replacing that boundary with Rust.
Compare JSON results, error codes and caller immutability. Diagnostic error
messages and explicitly listed diagnostic result paths are excluded from wording
comparison; diagnostic fields must remain strings. Echoed user descriptions and
identity material remain exact. Arrays retain order and absent fields differ from null.

The 122 cases include an empty-input request for each of the 83 engine operations,
envelope failures, UTF-8 hashes, digest ordering, portable paths, literal discovery,
routing, continuation, acceptance projections and blocked attempts. There are 48
successful responses. Empty-input rejection is not successful operation coverage;
each migrated use case still needs its own success, state and installed checks.
