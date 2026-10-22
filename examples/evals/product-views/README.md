# Read one packet for two decisions

This synthetic requirements-only reset-link example extends the manual QA scenario.
All design and implementation observations in `input.json` are fixture declarations,
not claims about an inspected production system. No application or model is invoked.

The product reader asks: can we draft an implementable single-use reset story?
`product-projection` retains criterion `single-use` as a gap, the unanswered expiry and
concurrency question, and unresolved decision `redemption-owner`. Its dependencies cite
both the proposed shared store (`design`, declared) and process-local state
(`implementation`, observed). A story can preserve the agreed single-use outcome while
keeping cross-instance ownership and outage behavior as open choices. It cannot promise
expiry, concurrency or delivery readiness from these inputs.

The architect asks: does the proposed boundary match the supplied implementation observation?
`architecture-projection` exposes `shared-store` and `local-state` as conflicting boundary
interpretations, the proposed ADR source and the proposed store-availability consequence
`outage`. An observed process-local implementation does not accept the proposed design.
The missing runtime/build identity and empty ledger provide no executed verification.

Run either view from the source checkout:

```js
import fs from 'node:fs';
import { evaluate } from '../../../engine/index.mjs';
const input = JSON.parse(fs.readFileSync(new URL('./input.json', import.meta.url), 'utf8'));
for (const operation of ['product-projection', 'architecture-projection']) {
  console.log(JSON.stringify(evaluate({ formatVersion: 1, operation, input }), null, 2));
}
```

Run `node --test tests/product-projection.test.mjs tests/engine-contract.test.mjs` for
ID/provenance parity, requirements-only behavior, invalid selections, stale/unknown
evidence and portable-engine replay. Those checks establish deterministic mechanics;
they do not establish that a model writes useful stories or architecture advice.
