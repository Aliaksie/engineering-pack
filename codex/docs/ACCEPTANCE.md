# Acceptance contracts

An opt-in engineering contract turns "done" into named criteria, each with declared checks
and an expected kind of evidence. `craft` computes per-criterion coverage from reported
evidence and the current source; required gaps block completion. Without a contract, tasks
work as before, with agent-declared completion and unknown coverage.

Examples: `tests/fixtures/acceptance/contract.json` (contract),
`examples/evals/manual-qa/` (QA charter) and `examples/evals/product-views/` (reader views).

## Shapes

Contexts and contracts carry `formatVersion: 1`; an evidence ledger carries 1 or 2. Unknown
fields fail and nothing is silently truncated.

- **Context**: relative `scope` paths, `sources` (kind
  `request|instruction|configuration|code|check|adr|external`, reference, content identity,
  observation time, provenance `declared|observed|inferred`, status
  `accepted|proposed|superseded|unknown`), `constraints` (authority, status, required flag,
  explicit conflicts), `checks`, `questions` and `coverage`. Source text stays outside.
- **Contract**: intent, exclusions, context, `criteria` (id, required flag, scope, source
  IDs, check IDs, expected evidence `check|manual`) and `decisions` (blocking/resolved).
- **Evidence ledger**: `records`, each naming criteria, scope, contract digest,
  `binding: {kind: repository|environment, identity}`, check or probe, artifact reference,
  reporter, support `verified|observed|inferred|unknown` and execution
  `passed|failed|not-run|not-available`. A format 2 record may add `tests: {runner, executed,
  failed, skipped, source: observed|declared}` (a count the runner did not report is left
  out), or `"tests": "observed"` to take the test run Craft observed on the current tree in
  this session (see [ROUTING.md](ROUTING.md#test-runs)): its counts, execution, `binding` and
  time replace what the record declared, and the request is refused when no run on the
  current tree was observed or its output had no recognized summary. `source: observed` is
  accepted only as Craft filled it, from a run it observed or the ledger already saved.

Contexts and contracts are limited to 32 KiB and 32 entries per collection; ledgers to 64
records. Errors use code `INVALID_INPUT` with an `acceptance:` reason as the message's first
word, followed by the entry and field that failed and what it takes:
`acceptance:invalid-value at context.sources[0].kind: one of request, instruction, …`.

## Coverage

A criterion is covered by current evidence that names it, one of its checks, its expected
kind and all its scopes, with `verified` support and `passed` execution. Evidence for a
different contract digest or source identity is stale; a missing identity is unknown. Any
current failure stays visible: a later pass never hides it. Completion also needs no open
required conflicts, blocking decisions or attempt blockers.

In a format 2 ledger, a record whose `tests` report no executed test (`reason: zero-tests`) or
counted failures fails its criteria whatever its execution says, a pass needs a named
`runner`, and a criterion with current evidence that decides nothing (not run, unverified,
unknown runner) is `unknown` instead of `gap`; `gap` means no current evidence. Format 1
ledgers read as before.

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
A passed step at the verify stage, named with `--stage verify` or carried over from the task,
with or without a contract, is refused while the test run that decides the current tree
failed (see [ROUTING.md](ROUTING.md#test-runs)).

Opting in upgrades that session's state to format 2; current readers accept 1 and 2. Stop
older writers before upgrading. `checkpoint-read` and session-start recovery return the
active task's recomputed coverage as `taskRecovery`.

## Criteria from a linked issue

When a session links an issue with `craft ticket --issue-data` (see
[ROUTING.md](ROUTING.md#the-linked-issue)), Craft keeps the list under the body's first
acceptance heading (`Acceptance:`, `## Acceptance criteria`, `**Acceptance criteria**`): one
criterion per bullet, numbered or checkbox item, with indented lines and sub-items joined to
their item. Up to 32 criteria of up to 512 characters are kept; others are counted in
`criteriaOmitted`, never cut short. A body with no such list states none.

A task tied to the issue takes these criteria as its contract when it starts without
`--contract` (or a plan task's contract) and without a contract of its own: a task named for
the issue with `--issue`, a plan task whose plan links it and that states no acceptance of its
own, or any task of a session whose ticket delivery (`ticket --accept`) is that issue. Other
tasks of the session carry the issue's identity only. The contract holds IDs `ac-1`, `ac-2`, … in the issue's
order, each required, checked by `issue-check` with `check` evidence, sourced from the issue
as an observed `external` source. Completion then needs evidence for each, as for any contract.
Criteria past the contract's 32 KiB are left out from the end; when any criterion the issue
lists is not held, the contract's context says how many in `questions` and its `coverage` is
`incomplete`, so every view shows it.
Starting the task again keeps its contract; an edited issue reaches a new task.

## Reader views

```sh
craft acceptance-view --view product --session <id>
craft acceptance-view --view qa --contract contract.json --acceptance-evidence ledger.json \
  --current-kind environment --current-identity BUILD_AND_ENVIRONMENT --probes probes.json
```

With `--session` (and `--task` for a task other than the bound one) a view reads that task's
contract and ledger and judges them against the task's checkout, or against the build
`--current-kind environment --current-identity <id>` names; `task-start` names this command
when the task has criteria. Otherwise it reads the files given. All three views read the same
contract and ledger, use the same coverage, write nothing and exit 2 on invalid input. With
`--current-kind` and no `--current-identity`, freshness is unknown.

| `--view` | Adds | Shows |
|---|---|---|
| `qa` | `--probes` (risk, flows, preconditions, actions, expected basis, linked evidence) | Probes `not-run` or `recorded` (not "passed"), unexplored criteria, and a `conflict` flag where current passes and failures disagree |
| `product` | `--focus` (constraint IDs as boundaries, dependencies, consequences) | Delivery gaps and unresolved decisions |
| `architecture` | Same as `product` | Evidence gaps and ADR sources with their status |

A probe links its evidence by `evidenceIds`: each names a ledger record whose `checkId` is the
probe's `id` and whose criteria are among the probe's `criterionIds`. Any other link, like
an unknown field, fails with `qa:invalid-probes at probes[<n>].<field>` and what it takes.

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
