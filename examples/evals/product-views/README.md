# Read one packet for two decisions

This synthetic requirements-only reset-link example extends the manual QA scenario.
All design and implementation observations in `input.json` are fixture declarations,
not claims about an inspected production system. No application or model is invoked.

The product reader asks: can we draft an implementable single-use reset story?
The product view retains criterion `single-use` as a gap, the unanswered expiry and
concurrency question, and unresolved decision `redemption-owner`. Its dependencies cite
both the proposed shared store (`design`, declared) and process-local state
(`implementation`, observed). A story can preserve the agreed single-use outcome while
keeping cross-instance ownership and outage behavior as open choices. It cannot promise
expiry, concurrency or delivery readiness from these inputs.

The architect asks: does the proposed boundary match the supplied implementation observation?
The architecture view exposes `shared-store` and `local-state` as conflicting boundary
interpretations, the proposed ADR source and the proposed store-availability consequence
`outage`. An observed process-local implementation does not accept the proposed design.
The missing runtime/build identity and empty ledger provide no executed verification.

`input.json` holds the contract, the empty ledger and the focus together. Write each to
its own file, then run either view from the source checkout:

```sh
npm run acceptance:view -- --view product --contract contract.json --acceptance-evidence ledger.json --current-kind environment --focus focus.json
npm run acceptance:view -- --view architecture --contract contract.json --acceptance-evidence ledger.json --current-kind environment --focus focus.json
```

`cargo test -p craft-engine --test view_contracts` replays both views over this input and
its variants: ID/provenance parity, requirements-only behavior, invalid selections and
stale/unknown evidence. Those checks establish deterministic mechanics;
they do not establish that a model writes useful stories or architecture advice.
