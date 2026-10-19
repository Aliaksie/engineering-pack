# Session efficiency

```sh
npm run session -- --root <project> --session <id> --task <task-id> --data <task.json>
npm run session -- --root <project> --session <id> --task <task-id> --data <task.json> --json
```

Installed bundles expose `node support/cli/main.mjs session`. The default is a small
status and recommendation; `--json` includes evidence, diagnostics and the selected guidance
report. `session:advice` retains JSON output. No command automatically compacts, resets,
switches models, starts workers or invokes arbitrary side effects.

## Three separate measurements

| Measure | Evidence | Unknowns and limits |
|---|---|---|
| Pack context | `context:report` and the current routed selection: paths, hashes, skills/references, bytes and estimated tokens | Selection does not prove the host loaded a file. Estimates are not occupancy. Missing resources make totals a lower bound. |
| Current occupancy | Fresh `current-context` observation with counts, capacity, source, timestamp and matching session identity | Cumulative/billing tokens and file reads cannot establish occupancy. Unsupported telemetry stays unknown. |
| Pack activity | Opt-in command receipts under `.capabilities/state/`, plus explicitly declared counters | Covers observed pack commands, not all host tools or provider usage. Missing counters stay unknown. |

A task record can begin small:

```json
{
  "sessionId": "work-1",
  "host": { "name": "consumer-host", "reliableCompaction": true },
  "task": {
    "id": "task-1",
    "revision": "actual-source-revision",
    "stage": "implement",
    "status": "working",
    "files": ["src/changed-file.mjs"],
    "safeBoundary": false
  }
}
```

`host.model` is optional and must be known. `usage` accepts `kind: "current-context"`,
integer `currentTokens` and `capacityTokens`, `source`, ISO `observedAt`, and matching
`sessionId`. Five-minute freshness is the default. Future, stale, cumulative, invalid and
other-session counts remain unknown. No private transcript or undocumented host API is read.

With an explicit `--session` and `--task`, `route`, `context-report`, `locate` and
`checkpoint-save` record their own bounded activity. Without both flags, these operations
retain their ordinary behavior without activity writes. `session` reads existing records;
it does not increment counters simply for displaying status. Recorded counts take precedence
over caller declarations. Read/lock failures never establish reuse or checkpoint freshness.
Ignore `.capabilities/state/` in the consumer repository. Neither activity nor lookup packets
belong in commits. Checkpoint state is excluded from repository-content comparison and lookup.

## Pressure and continuation policy

Default occupancy bands are healthy below 60%, elevated from 60%, high from 75%, saturated
from 85%. Tune ordered fractions with `policy.elevatedAt`, `compactAt`, `restartAt`; tune
observation freshness and notice cooldown with `maxAgeMs`, `cooldownMs`. These are advice
defaults, not universal provider limits. Session age alone never triggers a reset.

Healthy useful work continues. High pressure during a useful bounded step recommends
finishing it before reconsidering context. `safeBoundary: true` or a completed stage permits
context-change advice; `almostComplete: true` favors finishing. A stable task with reliable
host compaction favors checkpoint then compact. Saturation plus `phaseChanged`,
`staleAssumptions`, or `compacted` with persistent rediscovery favors checkpoint then a new
session. Without reliable compaction, the fallback is checkpoint plus new session. Unknown
occupancy yields no invented pressure recommendation. Recorded failures/blockers take priority.

Before recommending a context action, unified status verifies a saved continuation against
the task identity and complete current Git/index/working-tree fingerprints. Bare
`handoffSaved`/`checkpointFresh` declarations cannot bypass this check. Missing, changed,
wrong-task or incomplete checkpoints require a new handoff. Semantic accuracy still requires
the agent to keep the continuation current: unchanged files cannot prove a reasoning summary.
Native status updates without a matching continuation do not scan the whole tree just to
establish its absence. Automatic Git snapshots preserve the continuation's original source binding. They cannot
renew old semantic notes merely by updating a repository snapshot, infer conversation state,
or establish task completion.

### Continuation checkpoint

Save the small continuation with:

```sh
npm run checkpoint:save -- --root <project> --session <id> --task <task-id> --data <handoff.json>
npm run checkpoint:read -- --root <project> --session <id>
```

`handoff.json` contains a `continuation` object with ten string fields: `goal`, `currentState`,
`completed`, `approach`, `evidence`, `filesChanged`, `validation`, `remainingWork`, `blockers`,
`nextStep`. Use paths, hashes and concise summaries. All fields are required; goal and next
step must be nonempty. Each field is limited to 2,048 characters, the object to 8 KiB.
Unknown fields, including a transcript field, are rejected. Existing task/authorization/
nextStep/sources metadata remains supported; `--task` saves the continuation's `taskId`.

A new session reads the checkpoint, current repository diff/state and necessary routed
guidance. It does not replay the previous conversation. Recovery never renews authorization.
See [checkpoint recovery](ROUTING.md#checkpoint-and-recovery) for incomplete inspection limits.

## Reuse before bounded discovery

```sh
npm run locate -- --root <project> --session <id> --task <task-id> --path src --term Foo --term Bar --term Baz
```

The existing literal locator scans all fresh terms together. Optional session/task identity
adds a narrow cache of at most 32 compact symbol packets, capped at 64 KiB within bounded
session state. Resolve reusable items first; five valid packets out of eight requests leave
three fresh terms in one scan. Entirely reusable requests read no source-file contents.
Returned matches merge term identifiers and retain paths, line numbers and bounded snippets.

Reuse depends on session/task identity, locator implementation identity, HEAD, the scoped
file inventory and file version metadata (device/inode/size/mtime/ctime), plus ancestor
manifests and project configuration. Added, removed or edited files, revision changes,
manifest/config changes or task changes invalidate the affected scope. Versions are checked
again after scanning. Symlinks, failed inventory, incomplete/skipped coverage and unavailable
state never certify a fresh packet. This is local navigation evidence, not a semantic cache
or a defense against malicious concurrent filesystem changes. Accepted decisions or runtime
facts outside these literal packets must retain their own source/revision dependencies.

For staged discovery, `locate --data <request.json>` accepts:

```json
{
  "paths": ["src"],
  "requests": [
    { "id": "implementation", "term": "Foo" },
    { "id": "framework-guidance", "term": "Bar", "dependsOn": ["implementation"] }
  ],
  "maxOutputBytes": 32768
}
```

Only independent known literal requests execute. Dependent requests are returned as deferred
for the next reasoning stage, even if their prerequisite was in the same call. Arbitrary
commands, writes and mutations are unsupported. The default ceilings remain 500 files,
80 matches, 2 MiB scanned bytes and 240 characters per snippet; up to 32 terms are accepted.
`maxOutputBytes` additionally caps serialized result evidence (default 32 KiB, maximum
64 KiB); truncation is explicit. Metadata too large for the requested cap requires a smaller
request. Do not interpret no match as proof of absence. No saved tokens/cost or hypothetical
avoided round-trips are inferred from reuse counters.

Batch independent known reads, metadata and checks at a reasoning boundary. Keep adaptive
framework discovery, approvals, mutations and dependent checks staged. Batching does not
change their authorization, failure isolation or recovery semantics.

Existing routing-hint dedupe remains separate: it caches only emitted text identity for
15 minutes, keyed by current project config, rendered guidance and runtime identity. Routing
still recomputes. Session start/compaction invalidates it; unsupported delivery or unavailable
state emits the hint. Suppressed text is not evidence that files were freshly read.

## Meaningful attempts and capability classes

Record an acceptance attempt, not every message or successful mechanical tool call:

```sh
npm run session:attempt -- --root <project> --session <id> --task <task-id> --data <attempt.json>
```

The receipt requires `operation`, `target`, `approach`, `evidence` and `outcome`. Outcomes are
`passed`, `quality-failure`, `permission-denied`, `tool-unavailable`, `blocked`. Use bounded
identifiers/paths, never raw output. Optional `profile`, `maxAttempts` (1–5) and
`escalationReason` preserve actual model-profile changes and their explanation.

Complete repository fingerprints plus operation/target/approach identify equivalent failures.
Incomplete inspection still permits an explicit attempt receipt and budget, but equivalence
remains unknown. Escalation uses only receipts matching the current complete source fingerprint.
Changed approach/evidence target or repository contents distinguishes a retry; reverted
contents can match an earlier failure. A pass ends the current operation's failure series. Profile changes begin a new tier budget;
configured per-profile limits and the end of the escalation chain still stop further attempts.
Keep at most 24 receipts, evicting closed history before active failure series. Unrelated
successful operations cannot erase unresolved blockers or retry budgets. If all 24 slots
contain active failures, additional failure receipts are rejected explicitly; resolve an
active operation before recording more. A success can close a series even at capacity.
Model advice considers only the current series (or its latest closing success), so resolved
blockers and earlier successes cannot override a later retry series.
The default failure budget is three, configurable per receipt.
Repeated identical discovery requests with no new evidence also expose a loop signal.
A first no-progress failure recommends changing approach. At the budget/equivalence limit,
return control, or recommend a justified configured escalation. Permission/tool/prerequisite
failures stop as blocked. Receipts report outcomes supplied by the caller; they do not judge
acceptance on their own. Unobserved host operations cannot be counted or stopped by the pack.

Routing and unified status expose portable capability suggestions:

| Class | Typical work |
|---|---|
| economical | Literal lookup, known-file extraction, formatting, metadata and deterministic checks; prefer a deterministic tool first |
| standard | Routine implementation, focused debugging, tests and bounded review |
| strong | Architecture, ambiguous debugging, distributed consistency, security-sensitive or complex cross-module reasoning |

Task risks can justify direct strong reasoning; there is no forced traversal through cheap
tiers. Configure optional `execution.profiles.<name>.class` alongside existing capabilities,
quality budgets and `escalateTo` chains. Host model names remain in consumer mappings.
`model:advice` accepts an optional required `capabilityClass` and `currentProfile`. Unified
status connects current-task/current-revision failed-attempt evidence to those configured
profiles. Undeclared classes/mappings remain unavailable, not guessed. Capability classes
are recommendations, not measured price rankings or proof that a named model is cheaper.
No actual model switch or delegation occurs. A recorded escalation receipt counts the event;
a recommendation alone does not.

## Host support

| Host | Automatic occupancy input | Native controls exposed by this adapter | Other evidence |
|---|---|---|---|
| Claude Code | Explicit native statusline payload: current input/cache counts, excluding output; session-bound | `/compact`, `/clear` as recommendations | Known payload model id; otherwise unknown |
| Codex | Native UI only; no stable mapped-hook usage field | `/statusline`, `/compact`, `/new` as recommendations | Explicit normalized observations; unknown model unless supplied |
| Gemini, Cursor, Copilot, AGENTS, Gemini extension, JetBrains | Unavailable in current adapters | No mapped compact/reset command | Same portable CLI/status policy and explicit normalized observations |

Use native host indicators first. The pack does not replace the host runtime or configure
statusline settings automatically. Builds copy the same runtime into every standalone package.
Unsupported lifecycle enforcement remains visible in adapter reports. Default `mode` is
`recommend`; deterministic read-only lookup and explicitly requested local receipts/checkpoints
are the only actions introduced here. No provider/account changes, cost increases, context
destruction or unsafe side effects are silently executed.

## Evaluation and limits

The source-checkout fixture at `examples/evals/session-efficiency/` exercises a
resumed task with reusable evidence, two failed attempts and a small checkpoint. Use the
existing collector and retain local configs, outputs and judgments under ignored
`.eval-results/`. Deterministic policy tests establish behavior; actual paired model runs are
needed before reporting turn, latency, cost or correctness improvements. Unknown measurements
stay unknown. A compact continuation fixture does not establish behavior in a genuinely
saturated long conversation.

The operational view includes the canonical pack version. Supporting local telemetry is
enabled by default and independent of session counters and host occupancy; see
[observability and privacy](OBSERVABILITY.md) for content, limits, disable configuration
and explicit remote opt-in. Telemetry volume never estimates host context usage.
