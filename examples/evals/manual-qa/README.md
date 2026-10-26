# Manual QA from shared acceptance evidence

This fixture exercises source plus application access and requirements-only QA using
the same criterion `single-use`. It is a local synthetic example, not evidence about a
deployed password-reset service or model performance.

`craft acceptance-view --view qa` consumes [contract.json](contract.json),
[probes.json](probes.json) and an existing versioned acceptance ledger. No hook, companion package or configured
model is needed. Runtime-free QA can use the charter below directly.

## Source and application access

[The local application](fixture/reset.mjs) intentionally fails to consume a reset token.
Each instance owns isolated synthetic state. Redeeming `synthetic-token` once yields
`{accepted: true, changes: 1}`; replay yields `{accepted: true, changes: 2}`. Replay
violates the supplied single-use requirement. The fixture test performs both actions;
it makes no product repair, email request or remote account mutation.

The test binds QA evidence to `reset-fixture-v1:isolated-memory` and retains a failed
manual probe alongside an implementation check that passed first redemption only.
The view exposes the disagreement and failed manual acceptance. That green check does
not cover replay. This binding is an explicit fixture identity; real QA must identify
the actual observed application build and environment rather than copy this string.

## Requirements only; execution unavailable

Charter: spend at most 20 minutes investigating single-use reset links once an authorized
synthetic account, isolated environment, application access and build identity are ready.
Stop before sending email, changing customer accounts or exceeding the authorized data scope.

| Probe | Criterion/risk | Actions and expected basis | Current result |
|---|---|---|---|
| `replay` | `single-use`: a used link changes a password again | With a synthetic link and first-use success established, replay the same link; the agreed requirement prohibits reuse. Record both outcomes and account change count if observable. | NOT RUN; execution NOT AVAILABLE: no application access, test account or build identity |

Expiry, concurrent redemption, other sessions, delivery and keyboard/focus behavior are
unexplored. Their requirements and tools are unknown; do not invent expected outcomes.
On execution, retain environment/configuration, data assumptions, actions, observed results,
reporter and a bounded artifact. Missing build identity leaves freshness unknown, even
if an observation is available. A new build requires new observations.

## Assessment boundary

The automated tests cover serialization, incomplete execution, build/contract invalidation,
conflicting results and invalid probe links. They do not judge whether a model chooses
useful regression scope. A human reviewer should assess whether replay targets the changed
behavior, the green first-use check is interpreted accurately, omissions are explicit and
the charter is usable without unsafe effects. Record that judgment separately from tests;
no baseline/candidate model comparison or human acceptance is implied by this example.
