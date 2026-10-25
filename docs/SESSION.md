# Session efficiency

```sh
npm run session -- --root <project> --session <id> --task <task-id> --data <task.json>
npm run session -- --root <project> --session <id> --task <task-id> --data <task.json> --json
```

Installed bundles expose `node support/cli/main.mjs session`. The default is a small
status and recommendation; `--json` includes evidence, diagnostics and the selected guidance
report. `session:advice` uses the same display. Its `--data` declarations must be a JSON
object. No command automatically compacts, resets, switches models, starts workers or invokes
arbitrary side effects.

Routine `task-start`, `task-step`, `session-attempt`, `checkpoint-save`, `task-offer`,
`task-decision` and `task-action-check` commands print compact receipts, next actions and
diagnostics. Use `--json` or `--verbose` for the full structured result, including checkpoint
state and guidance reports. Programmatic callers still receive the complete object.

## Normal task lifecycle

For multi-step work, establish a task once using the host-provided session identity:

```sh
node cli/main.mjs task-start --root <project> --session <host-id> --task fix-value --goal "Correct the value" --file src/value.mjs
node cli/main.mjs locate --root <project> --session <host-id> --term SymbolValue
node cli/main.mjs task-step --root <project> --session <host-id> --stage implement --target acceptance:value --approach bounded-fix --evidence checks/value-1 --outcome passed --result "Value corrected" --next "Verify acceptance"
node cli/main.mjs session --root <project> --session <host-id>
```

Installed bundles use `support/cli/main.mjs`. `task-start` routes relevant guidance and binds
one active task to the session using private runtime state. The binding includes the Git
symbolic ref or detached state; after changing it, rerun task-start explicitly. Older bindings
without that checkout identity also require task-start. No branch name is prohibited. Read the selected resources;
routing is a selection receipt, not proof they were loaded. Repeating the same id and goal
preserves attempts and lookup evidence; use a new task id for a new outcome. A material stage
or file-scope change can rerun `task-start` with the same goal and updated `--stage`/`--file`.
Explicit task flags on existing commands retain their meaning. `task-step` must target the
active binding; switch with `task-start` before recording a step for a different task.

The existing `route`, `locate`, `context-report`, `session`, `session-attempt` and
`checkpoint-save` commands can adopt this binding when `--session` is supplied without
`--task`. Missing identity, invalid binding or unavailable storage never establishes a cache
hit. Direct commands and guidance-only hosts remain usable without the lifecycle. A tiny
one-off task need not create a record. No undocumented host session discovery is attempted.

### Workspace and repository scope

The coordination root may be a Git repository or a plain folder containing several
repositories. Existing single-repository calls need no new flags. From a parent workspace,
select the task's Git worktree roots explicitly; selection never scans or enrolls siblings:

```sh
# One repository within a non-Git workspace.
node cli/main.mjs task-start --root <workspace> --session <host-id> --task update-kit --goal "Update the kit" --repository career-kit --file career-kit/src/main.mjs
# One coordinated task across two repositories.
node cli/main.mjs task-start --root <workspace> --session <host-id> --task update-contract --goal "Update producer and consumer" --repository career-kit --repository career-profile
```

Repeat `--repository` for at most eight workspace-relative worktree roots, including `.`
when applicable. Traversal, absolute paths, symlink or case aliases and subdirectories
masquerading as repository roots are rejected. Linked Git worktrees are supported. Keep the coordination
root/session stable across task steps. Repeating task-start retains its repository selection
unless a new selection is supplied; a changed selection invalidates prior acceptance.
Each selected repository retains its own revision, checkout, status and fingerprints in
`repositorySnapshots`. The combined `workspace:…` revision is a collection identity, never
a Git commit to pass to Git. Changes or checkout transitions in any selected repository
invalidate evidence; an unselected sibling does not. Missing or inaccessible selected
repositories remain unavailable rather than silently shrinking the scope.

Bound lookup defaults to the selected paths and rejects paths outside them. Parent-folder
lookup combines the selected repositories' Git inventories, honoring their ignores and
global scan bounds, and currently does not claim lookup-cache reuse for explicit selections.
Each repository's instructions, permissions, branch rules and build commands still
apply independently; this binding does not merge child configuration into parent policy.
Revision-bound Git operations and reviews still require an actual repository and its refs.
Without explicit selection, a non-Git root remains limited and compact checkpoint output
reports incomplete repository inspection, as does a working-tree name that is not UTF-8.
Standalone checkpoint-save also accepts repeated `--repository`; recovery reuses that saved
selection. Its `--data` handoff must be a JSON object.

### Private scratch and retained evidence

Use `workspace-open` only when a task needs filesystem-backed intermediate material.
The result supplies a private scratch directory for the session, task and optional
`--operation` scope. No scratch directory is created by ordinary task binding or inspection.
The workspace sits beneath `.craft/state/<worktree-id>/shared/` in the selected
coordination root, for both Git and non-Git folders. Raw
session/task names are hashed in directory names. Existing checkpoints, receipts, caches
and telemetry keep their existing stores; there is no duplicate state tree or new `.pack` root.
Repository selection also participates in workspace identity, so rebinding a task to other
repositories cannot consume the prior selection's scratch. Prior scoped evidence remains private.

```sh
node cli/main.mjs workspace-open --root <workspace> --session <host-id> --operation investigation
node cli/main.mjs workspace-write --root <workspace> --session <host-id> --operation investigation --data <artifact-input.json>
node cli/main.mjs workspace-promote --root <workspace> --session <host-id> --operation investigation --data <promotion-input.json>
node cli/main.mjs workspace-release --root <workspace> --session <host-id> --operation investigation
```

The write input is the JSON object `{ "name": "result.json", "content": "small structured result" }`;
promotion is `{ "name": "result.json", "purpose": "acceptance" }`. Managed writes enforce
file/byte budgets and refuse overwrites. Direct tool writes to the supplied directory are
cooperative: they must respect the same bounds; the pack cannot intercept arbitrary tools.
Promotion copies only the named bounded regular file and records operation, time, source
fingerprint and purpose. Never promote credentials, authorization material, full transcripts
or complete prompts. Routine receipts often suffice; retaining an artifact is optional.
The fingerprint is explicitly `sourceAtPromotion`; creation-time source identity is unknown
for files produced by external tools and is never inferred from a later promotion.

Task completion closes the coordinator's scopes and removes scratch when no other live owner
remains. Workers release their own scopes after use. Startup/task boundaries and
`workspace-clean --root <workspace>` reclaim verified closed or stale abandoned workspaces.
Live process owners are retained regardless of age; uncertain ownership, symlinks, unsafe
metadata, exhausted bounds or busy locks retain data and expose a reason. A dead managed lock
can be reclaimed; unknown locks are preserved. On Windows the native runtime observes other
processes through one bounded `tasklist` snapshot per operation; when that listing is
unavailable, a recorded owner holds its workspace until the scratch stale age, and its
workspace lock can be reclaimed after ten minutes. Cleanup failure never blocks task acceptance.
The lock serializes cooperating pack processes; these path checks are not an OS sandbox
against a hostile same-user process swapping paths concurrently.

Defaults need no configuration: workspace enabled, 20 session/task directories, 8 MiB/64
scratch files per directory, stale scratch after 24 hours with no live owner, and 1 MiB/16
intentionally retained evidence files for seven days after inactivity. A new workspace at
capacity evicts the oldest inactive one; active owners and unverified entries are never
evicted, so optional allocation fails when only those remain. Effective user/project
settings may override `workspace.enabled`, `maxSessions`, `scratch.staleAfterHours`,
`scratch.maxBytes`, `scratch.maxFiles`, `evidence.enabled`, `evidence.retentionDays`,
`evidence.maxArtifacts` and `evidence.maxBytes`. Disabling persistence does not delete prior
data. Settings preview creates no workspace. Session JSON exposes paths, actual disk usage,
cleanup outcomes and retention reasons; compact output shows workspace details only when
present or exceptional.

Private state is excluded at source boundaries, including nested `.craft/state/`
paths: lookup, routing, content/index snapshots, review diffs, guidance roots and package
copying. No `.gitignore`, host `TMPDIR`, host storage, IDE cache or third-party cache is
rewritten. Standalone build/release/test staging keeps its already-owned command lifecycle;
this runtime does not migrate unrelated stores for directory consistency.

Runtime state stays beneath the selected root’s `.craft/state/`; Craft storage never
writes Git metadata. This also applies to linked worktrees and non-Git coordination roots.

At an acceptance boundary, `task-step` requires an agent-declared target, approach, evidence
identifier, outcome, result and next step. Outcomes are `passed`, `quality-failure`,
`permission-denied`, `tool-unavailable`, `blocked` and `prerequisite-resolved`. It records
the receipt and updates a compact continuation through the existing checkpoint mechanism when enabled. It does not
read or judge the evidence artifact. Goal/result/next-step text is bounded to 512 characters.
No prompt, transcript or tool output is automatically captured. Shell success never creates
an acceptance receipt. Unchanged repository snapshots cannot renew older semantic evidence.

Use `--complete` only for accepted overall completion. A failed receipt or unresolved
acceptance target prevents that declaration. Success closes only the matching operation and
target; unrelated success cannot erase a failure series. A blocker outcome on a verify step
leaves verification unrecorded: the check did not run. Source edits invalidate stored
completion and verification when status is read, even if HEAD did not change. Context
occupancy remains unknown unless a supported observation supplies it.

After actually correcting a permission, tool or prerequisite issue, record
`prerequisite-resolved` against the same stage/operation and target, with evidence of
the correction. This closes only matching blocker receipts; quality failures and other
targets stay open. It returns the task to working state without marking acceptance or
verification passed, and cannot be combined with `--complete`. It grants no permission,
dispatches no retry and does not bypass fresh host/action checks. Run acceptance afterward.

The step response separates recorded acceptance, checkpoint result, next-step suggestion
and status. A checkpoint failure does not undo a recorded receipt or invent a saved handoff.
Explicitly disabled checkpoints leave receipts usable. Advice is a recommendation; no test,
review, worker, model switch, compaction or remote operation is dispatched. A stored notice
suppresses unchanged repeated step offers during its cooldown; an explicit status query still
shows the underlying recommendation for inspection. Continue already-authorized work;
request a new decision only for an additional action that lacks authorization.

## Scoped action recommendations

`task-start` returns logical model advice with the route. It keeps the current execution
when exact mappings or native switch controls are unavailable. Economical/standard/strong
profiles are bundled when no profile catalog is supplied; a supplied catalog remains
consumer-owned. Named capabilities are never invented. Default quality budget is three;
explicit profile limits override it. Main-model advice works without worker configuration.
`mainModelSwitch: true` in an execution host is a support declaration, not permission or
proof of a native switch. Inspect `mainModel` limitations separately from worker mappings.

A verified risk-sensitive step can create an additional review offer automatically.
Declare applicable risks with `task-start --risk security`. If the exact review scope is
already authorized, `--review-authorization <user-or-standing-scope-reference>` records that
fact and avoids a new approval request. A recommendation and an accepted check are separate
records. Manual review settings suppress optional offers while explicit review work remains usable.

Use the same local task record for optional review, delegation and compaction:

```sh
node cli/main.mjs task-offer --session <id> --root <project> --action review --scope review:current-source --benefit "Check the verified change risks" --evidence checks/current --data facts.json
node cli/main.mjs task-decision --session <id> --root <project> --offer <proposal-id> --decision approved --evidence user-message:12
node cli/main.mjs task-action-check --session <id> --root <project> --offer <proposal-id> --data facts.json
```

An offer explains trigger, benefit and scope. Present its question through the actual host's
supported question mechanism. Record `approved` or `declined` only from an actual scoped
answer; pending, silence and timeout never become approval. `--prior-authorization <reference>`
can declare existing authorization for an exact offer scope. Existing decisions are preserved;
use `task-decision` for a later explicit change of decision. Unchanged pending/declined offers
are suppressed. Source, policy, scope, host/model or material risk changes require re-evaluation.

Facts are **caller-supplied observations**, never preference grants. A review preflight can
use `{"allowed":true,"supported":true}` after verifying current session/consumer policy and
actual host capability. `observations` accepts the existing session input shape. Requested
review or current verified source plus declared risks supports a review recommendation.
Compaction additionally needs a safe boundary, fresh matching occupancy, a fresh continuation
and declared reliable compaction support. Unknown/cumulative usage, stale handoffs,
almost-complete/completed work and unsupported controls cannot justify compaction.

Worker offers require an exact configured role/profile/model mapping, `independentTasks`
greater than one, and a fresh `workers` observation:

```json
{
  "observations": { "host": { "name": "consumer-host" } },
  "independentTasks": 3,
  "allowed": true,
  "supported": true,
  "workers": {
    "capacity": 4,
    "occupiedSlots": 1,
    "taskWorkers": 0,
    "allowed": true,
    "sessionId": "actual-session-id",
    "observedAt": "actual-current-ISO-time",
    "source": "verified-host-slot-observation"
  }
}
```

Replace observations with actual values; this example is not live evidence. Capacity is total
host slots. Occupied slots include main, other tasks and every current task worker that still
occupies a slot, including idle/reusable/nested workers. All roles/models share one task-tree
ceiling. Do not subtract failed/cancelled/completed workers until the actual host releases
their slots. Observations expire after 30 seconds and must match the session. A preflight is not a slot reservation; a native controller must serialize/reobserve admission
before dispatch, with nested workers sharing the same controller. The ceiling is
the lesser of configured workers and host capacity after main/other occupied slots. A reduction
stops new assignments without cancelling existing work. Explicit zero suppresses worker plans
and offers; deterministic tool batching remains independent. `settings --session <id> --facts
<workers.json>` shows requested/effective/available limits and missing prerequisites.

Every action response distinguishes the offer, decision, current prerequisites and execution.
`ready: true` means the **declared** decision and prerequisites passed revalidation; it is not a
permission grant or evidence of native execution. Verify decision references against the real
conversation/standing policy, then let the supported host perform and observe the action.
The CLI does not dispatch workers, switch models or invoke compaction. Explicit application
controllers accept trusted host callbacks for workers, review and compaction. The explicit
native bindings connect those controls through Codex App Server, Claude SDK or ACP for an
owning client; JetBrains/AGENTS require an explicit underlying host. Project settings do
not activate clients. See the [host controller contract](HOST-CONTROL.md).
Native lifecycle, actual concurrency and model round trips still require isolated integration evaluation.
Worker success does not close main acceptance. Keep independent authorized work moving while
an optional action is pending, and never repeat a permission/tool failure through a stronger model.

## Three separate measurements

Runtime receipts and host controllers record observable feature use without model
classification. Normal delivery requires no feature inspection or bookkeeping; missing
evidence and semantic judgments stay unknown. For an explicitly requested evaluation,
`task-features --session <id>` shows outcomes, provenance and source freshness. Use
`task-feature --session <id> --feature review --feature-status used --reason <reason>
--evidence <reference>` to retain a bounded declaration. Supported statuses are `used`,
`eligible-but-skipped`, `disabled`, `unsupported`, `not-applicable` and `failed`.
CLI declarations are marked `agent-declared`; observed controller results and historical
command counters remain separate. A complete classification is no semantic success verdict.
`worker-status --session <id>` reads the shared task-tree reservations without dispatching.

| Measure | Evidence | Unknowns and limits |
|---|---|---|
| Pack context | `context:report` and the current routed selection: paths, hashes, skills/references, bytes and estimated tokens | Selection does not prove the host loaded a file. Estimates are not occupancy. Missing resources make totals a lower bound. |
| Current occupancy | Fresh `current-context` observation with counts, capacity, source, timestamp and matching session identity | Cumulative/billing tokens and file reads cannot establish occupancy. Unsupported telemetry stays unknown. |
| Pack activity | Identity-bound command receipts in private runtime state, plus explicitly declared counters | Covers observed pack commands, not all host tools or provider usage. Missing counters stay unknown. |

For separately installed packages, repeat `--bundle <verified-package-root>` on
`context-report`, `session`, `session-advice`, `task-step` and action advice commands.
Agents derive these roots from the loaded skill resources; users need not know cache layouts.
The API also accepts an array of roots. A single root still works for source trees, complete
exports and standalone packages. At most 16 explicitly supplied roots are inspected; no host
cache discovery or sibling imports occur. Manifests identify each package. Multiple distinct
installations of the same selected package are reported as ambiguous rather than choosing a
version. Multi-root file entries include `bundleRoot` alongside their relative `path` and hash;
repeated physical roots and files are counted once. Missing packages keep totals incomplete.
The CLI supplies its own installed root when none is given; running `craft session`,
`session-advice`, `task-step`, `task-offer` or `task-action-check` directly requires at least
one `--bundle`.
This measures selected resources, not observed host loads or current context occupancy.

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
`checkpoint-save` record their own bounded activity. Without a complete explicit or bound identity, these operations
retain their ordinary behavior without task activity writes. `session` reads existing records;
it does not increment counters simply for displaying status. Recorded counts take precedence
over caller declarations. Likewise, a saved task record owns its stage, status, completion,
files, revision, verification, safe boundary, next step, risks and review request, even when a
field is absent or reset as stale; `--data` task declarations fill only the remaining fields.
Read/lock failures never establish reuse or checkpoint freshness.
Git and non-Git roots use the same version 2 storage layout:

```text
.craft/
  project.json
  telemetry.json
  state/
    <worktree-id>/
      <session-id>/       # checkpoints, bindings, activity, workers, hint/control journals
      shared/             # telemetry/key/captures and workspace maintenance
```

The platform resolver hashes the canonical selected root into a 24-character worktree ID
and an explicit session identity into a 24-character session ID. IDs, rather than Git
metadata location, isolate roots and sessions. Callers resolve invocation subdirectories
through the normal project-root resolver before accessing state; explicitly selected nested
or coordination roots keep their own identities. Linked worktrees have distinct root IDs.
Project-wide records use the reserved `shared` scope; scratch/evidence beneath it retain
explicit hashed session/task/operation ownership and their existing cleanup policies.
`settings` shows this shared directory and layout version. Raw session names never become
path components. File schemas and retention limits remain unchanged. Directories/files use
0700/0600 where supported; symlinks, unsafe paths and bounded-file violations are refused.

Add `.craft/state/` to the consumer's `.gitignore` before use (this repository already
ignores it at every depth). Craft does not edit consumer ignores or Git metadata. Source
lookup, snapshots, review and package export exclude state independently of Git ignores;
consumer-authored configuration remains visible. Runtime cleanup owns only its existing
bounded scratch, evidence and telemetry lifecycles, never project/telemetry configuration.

On access, the resolver copies matching legacy records from `.git/craft-state/<root-id>/`
(or a linked worktree's Git directory), flat `.craft/state/`, and `.capabilities/state/`.
Session records go to the explicit session scope; project-wide records go to `shared`.
Workspace scratch/evidence ownership trees, telemetry backups and hint generations are
preserved. Sources are never deleted or written, and configuration is never migrated.
Copies are bounded (4 MiB/file, 16 MiB and 4096 entries per record tree, depth 16), reject
symlinks and hard-linked files, and retain existing record validators. A source fingerprint
receipt is published after copying; identical interrupted copies can be retried, differing
destination files are refused. Unused legacy records remain available for later access.

Stop older installed plugins before upgrading. Legacy locks block migration; source
fingerprints are checked again on access and changed sources cause an explicit reconciliation
error. Old plugins cannot honor new locks, so simultaneous old/new writers are unsupported.
Do not clear a lock or conflict without verifying the old writer has stopped and reconciling
records. Retained legacy sources may be removed only through separately authorized cleanup.

Changing a task's stage, files, risks or checkout invalidates completed acceptance and verification.
Continuation freshness also requires the saved task contract to match; an automatic snapshot
cannot renew a continuation written for earlier scope.

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
`passed`, `quality-failure`, `permission-denied`, `tool-unavailable`, `blocked`, `prerequisite-resolved`. Use bounded
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
profiles. Undeclared capability classes remain unavailable; missing exact model mappings retain
main execution with an explicit limitation rather than blocking logical advice. Capability classes
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
`recommend`; deterministic lookup, evidence reuse and local milestone snapshots can run
under the effective settings. Acceptance receipts still require explicitly supplied evidence. No provider/account changes, cost increases, context
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

## Effective default policy

`node cli/main.mjs settings --root <project>` shows the effective policy,
with the origin of each leaf value and the private state directory. CLI and hooks share
the same resolver. Routing hints, hint deduplication, milestone snapshots and lookup reuse
are enabled by default; explicit false disables the respective automatic behavior.
`runtimeApplied` lists connected controls and `prerequisites` identifies integration limits.
Semantic acceptance and continuation still require agent-supplied evidence; settings never
start workers, review or native compaction.

Precedence is bundled defaults, personal preferences, project settings, then task overrides.
Personal preferences live at `~/.config/craft/preferences.json`; `--preferences <file>`
selects an explicit file for reproducible inspection. Missing files contribute no values.
Preference paths reject symlinks; on POSIX, the file and its parent must also be user-owned
and not writable by others. Windows relies on consumer-managed ACLs. Files are limited to 64 KiB. Preferences and task overrides accept
only `routing`, `checkpoint` and `efficiency`; they cannot declare permissions, services
or host model mappings. No configuration file is generated in the project.

For a personal override, save `{"efficiency":{"delegation":{"maxWorkers":0}}}` in
the preference file. For a project override, add
`"efficiency":{"batching":{"maxConcurrency":2}}` to `.craft/project.json`.
Use `--data <file>` for a task override with the same restricted preference shape.
Explicit `false` and zero survive merging; omitted nested siblings retain defaults.
Worker ceilings apply to the shared reservation ledger when a trusted controller adapter is
connected. They accept nonnegative safe integers and do not establish host capacity
or authorization. Review, delegation and context actions default to recommendation.

A manual policy can use:

```json
{
  "routing": { "editHints": false, "dedupeHints": false },
  "checkpoint": { "enabled": false },
  "efficiency": {
    "lookup": { "enabled": false, "reuse": false },
    "batching": { "enabled": false },
    "attempts": { "enabled": false },
    "advice": { "enabled": false, "nextStep": false },
    "review": { "mode": "manual" },
    "context": { "action": "manual" },
    "delegation": { "mode": "manual", "maxWorkers": 0 }
  }
}
```

Explicit commands remain available. A preference is never execution permission, a
model mapping, a hook trust decision or evidence of observed feature use.

Structured task contracts and criterion evidence are opt-in through the existing
`task-start --contract` and `task-step --acceptance-evidence` commands. See
[acceptance contracts](ACCEPTANCE.md#task-command-persistence) for input shapes,
conditional ledger replacement, completion gates and version-2 session compatibility.

The developer acceptance pilot in source-checkout `examples/evals/developer-acceptance/README.md`
provides a reproducible two-criterion bug fix and adversarial reporting variants. Its
source test composes cited context, persists a contract, executes independent checks
and rejects missing or stale coverage through the task CLI. The direct tiny-fix variant
uses no session runtime. Deterministic results establish mechanics; usefulness and
shorter-guidance decisions require separate retained paired model runs.
