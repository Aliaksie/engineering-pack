# Acceptance contracts

An opt-in engineering contract turns "done" into named criteria, each with declared checks
and an expected kind of evidence. `craft` computes per-criterion coverage from reported
evidence and the current source; required gaps block completion. Without a contract, tasks
work as before, with agent-declared completion and unknown coverage.

Examples: `tests/fixtures/acceptance/contract.json` (contract),
`examples/evals/manual-qa/` (QA charter) and `examples/evals/product-views/` (reader views).

## Shapes

All payloads carry `formatVersion: 1`; unknown fields fail and nothing is silently truncated.

- **Context**: relative `scope` paths, `sources` (reference, content identity, observation
  time, provenance `declared|observed|inferred`, status
  `accepted|proposed|superseded|unknown`), `constraints` (authority, status, required flag,
  explicit conflicts), `checks`, `questions` and `coverage`. Source text stays outside.
- **Contract**: intent, exclusions, context, `criteria` (id, required flag, scope, source
  IDs, check IDs, expected evidence `check|manual`) and `decisions` (blocking/resolved).
- **Evidence ledger**: `records`, each naming criteria, scope, contract digest,
  `binding: {kind: repository|environment, identity}`, check or probe, artifact reference,
  reporter, support `verified|observed|inferred|unknown` and execution
  `passed|failed|not-run|not-available`.

Contexts and contracts are limited to 32 KiB and 32 entries per collection; ledgers to 64
records. Errors use code `INVALID_INPUT` with an `acceptance:` reason.

## Coverage

A criterion is covered by current evidence that names it, one of its checks, its expected
kind and all its scopes, with `verified` support and `passed` execution. Evidence for a
different contract digest or source identity is stale; a missing identity is unknown. Any
current failure stays visible: a later pass never hides it. Completion also needs no open
required conflicts, blocking decisions or attempt blockers.

Coverage is `assurance: attributed`: it follows the reporter's assessments and does not
check artifacts or truth itself. Reporters establish artifact availability before claiming
`verified`; unexecuted work is `not-run`. None of this grants authorization or runs a check.

## With task commands

```sh
craft task-start --root P --session S --task T --goal "Outcome" --contract contract.json --json
craft session    --root P --session S --json
craft task-step  --root P --session S --stage verify --target acceptance:criterion --approach assessed-check \
  --evidence retained-output --outcome passed --result "Observed behavior" --next "Report gaps" \
  --acceptance-evidence ledger.json --expected-task-contract TASK_DIGEST --expected-evidence-digest LEDGER_DIGEST --complete --json
```

Use the `taskContract`, `evidenceDigest` and `binding` that `task-start` and `session`
return. A ledger is replaced only when both expected digests still match; after a conflict,
reload and reconcile rather than retry. Contract, scope, risk, repository or checkout changes
clear coverage; source edits make evidence stale. `--complete` is refused while a required
gap, failure, conflict or blocker remains, and a refused completion persists nothing.
Non-Git roots have no repository identity and cannot reach verified completion this way.

Opting in upgrades that session's state to format 2; current readers accept 1 and 2. Stop
older writers before upgrading. `checkpoint-read` and session-start recovery return the
active task's recomputed coverage as `taskRecovery`.

## Reader views

```sh
craft acceptance-view --view qa --contract contract.json --acceptance-evidence ledger.json \
  --current-kind environment --current-identity BUILD_AND_ENVIRONMENT --probes probes.json
```

All three views read the same contract and ledger, use the same coverage, write nothing and
exit 2 on invalid input. Without `--current-identity`, freshness is unknown.

| `--view` | Adds | Shows |
|---|---|---|
| `qa` | `--probes` (risk, flows, preconditions, actions, expected basis, linked evidence) | Probes `not-run` or `recorded` (not "passed"), unexplored criteria, and a `conflict` flag where current passes and failures disagree |
| `product` | `--focus` (constraint IDs as boundaries, dependencies, consequences) | Delivery gaps and unresolved decisions |
| `architecture` | Same as `product` | Evidence gaps and ADR sources with their status |

Selections never add facts or change coverage. Empty dependencies mean none were supplied,
not none exist.

## Grounded context

```sh
craft grounded-context --root <project> --data input.json
```

Composes a read-only context packet for 1–32 project-relative paths. It adds hashes of
`AGENTS.md` files, project configuration and cited local sources, plus module ownership,
dependencies and declared checks from routing. Supplied constraints keep their citations and
conflicts. An existing `locate` result passes through unchanged. Coverage is always
`unknown` or `incomplete`; recompose after sources change. Nothing is executed and no task
state changes.
