# Sessions

`craft` keeps a small private record per host session: the active task, its acceptance
receipts, a continuation checkpoint and activity counters. From these and fresh host
observations it recommends the next step. It never compacts, resets, switches models,
starts workers or runs anything on its own. Unknown observations stay unknown.

Commands print compact receipts; `--json` or `--verbose` returns the full result. Every
command and its options are listed by `craft <command> --help`.

## Task lifecycle

```sh
craft task-start --root <project> --session <host-id> --task fix-value --goal "Correct the value" --file src/value.mjs
craft locate     --root <project> --session <host-id> --term SymbolValue
craft task-step  --root <project> --session <host-id> --stage implement --target acceptance:value \
  --approach bounded-fix --evidence checks/value-1 --outcome passed --result "Value corrected" --next "Verify acceptance"
craft session    --root <project> --session <host-id>
```

- `task-start` binds one active task to the session, routes guidance and snapshots the
  checkout. Repeat it with the same id to change stage or files; use a new id for a new
  outcome. Changing branch, stage, files or risks invalidates earlier acceptance.
- With `--session`, `route`, `locate`, `context-report`, `session`, `session-attempt` and
  `checkpoint-save` adopt the bound task. One-off work needs no task.
- `task-step` records an agent-declared receipt: target, approach, evidence reference,
  outcome, result and next step. Outcomes are `passed`, `quality-failure`,
  `permission-denied`, `tool-unavailable`, `blocked` and `prerequisite-resolved`. `craft`
  does not read or judge the evidence; shell success never creates a receipt.
- `--complete` declares accepted completion and is refused while a failure or blocker is
  open. Source edits invalidate stored completion and verification.
- After really fixing a permission or tool problem, record `prerequisite-resolved` for the
  same target. It reopens work; it does not pass acceptance.

### Session identity and handoff

When a session starts or its context is restored, hooks that can add context begin with the
session line: `{"id": <host session id>, "task": <bound task or null>}`. Use that id as
`--session`; without it, work without a task rather than inventing one.

All task records belong to one session. A new session (a new conversation, `/clear`, or after
"checkpoint, then new session") starts without a task. If it starts without one and
checkpoints are enabled, the session line also lists up to three unfinished tasks of earlier
sessions in the same worktree and checkout, newest first: those changed within 14 days and
not completed. Only the most recent record of a task counts, so a task completed in a later
session is not offered again. Adopt one only when the user's request continues it:

```sh
craft task-adopt --root <project> --session <new-id> --from <from>
```

Adoption copies the task record and its checkpoint into the new session and binds the task
there; the earlier session's records are not changed. The result shows the task as current
source sees it and compares the checkpoint with the current tree: evidence for changed
source must be rechecked. Worker and host-control records stay with the earlier session and
the native work they describe (`notCarried`). Adoption never renews authorization and never
replaces another task bound to the session or its checkpoint. Repeating it is harmless.
Directories of earlier layouts are not offered.

### Work plans

A plan belongs to a work item (a feature, a tracker issue, a backlog item), not to a
session: one goal and up to 64 tasks with dependencies, worked on across sessions. Simple
work needs none; create one when the work is multi-step, likely to span sessions, delegated,
or the user asks for it.

```sh
craft plan-create --root <project> --plan checkout-retry --goal "Retry failed checkouts"   --reference tracker:PROJ-123 --data tasks.json      # {"tasks": [{"id": "a", "goal": "...", "dependsOn": []}]}
craft task-start  --root <project> --session <host-id> --plan checkout-retry --task a
craft plan        --root <project> --plan checkout-retry
craft plan-update --root <project> --plan checkout-retry --expected-revision 3 --data changes.json
```

- Task states are fixed: `pending`, `running`, `verifying`, `completed`, `blocked`,
  `failed`, `abandoned`. A pending task whose dependencies are all completed is shown as
  `ready`; readiness is derived, never stored.
- `task-start --plan P --task T` takes the goal, files and repositories from the plan task
  and claims it: a ready task starts `running` in this session. A task that is not ready, or
  that another session holds, is refused.
- `task-step` advances the plan task held by its session: the `verify` stage makes it
  `verifying`, a blocker `blocked`, `--complete` `completed`; a failed check or a resolved
  prerequisite keeps it `running` until the session's attempt budget is spent, which makes
  it `failed` and releases it. Result, next step and evidence reference are copied; a
  completion also records `verified: {revision, contract, at}`, the source revision and
  engineering contract digest it was accepted against. Only such a completion readies
  dependent tasks. A session whose task was taken over can no longer step it.
- `failed`, `abandoned` and a return to `pending` are set explicitly with `plan-update`,
  which also adds tasks, dependencies and decisions. It is refused when the plan changed
  since `--expected-revision`, when dependencies are unknown or cyclic, and for a task held
  in another session unless `--take-over` is given. A plan is `completed` only when every
  task is completed or abandoned.
- When a session starts, the session line lists up to three open plans, newest first, with
  ready tasks and held tasks (`here` when this session holds one, and minutes since its last
  step). To continue a task held in another session, repeat `task-start` with `--take-over`:
  that session may still be working, so take over only when the user's request continues
  the work here. Its task record and checkpoint are copied into this session; the earlier
  session's records are not changed and no authorization is renewed.
- References are opaque labels; reading a tracker issue is a separate, authorized
  [service operation](COMMANDS.md#services). `plan --reference tracker:PROJ-123` finds the
  plans that link it.

#### Coordination

```bash
craft plan-next    --root <project> --plan checkout-retry --session <host-id>
craft task-start   --root <project> --session <host-id> --plan checkout-retry --ready
craft plan-release --root <project> --plan checkout-retry --task b --session <host-id> --reason "Needs a decision"
```

- `plan-next` only reads. It names the `next` task and groups the rest in plan order:
  `ready` (children of a parent this session holds first), `held` here, held `elsewhere`
  (with `idleMinutes`, `stale` after 120 minutes without a step, `orphaned` when the holding
  session's folder is gone, `worker` when this session assigned it), `waiting` with the
  unfinished tasks they wait on (`stuck` behind a failed or abandoned one, which only
  `plan-update` resolves), `retry` (failed with attempts left), `exhausted`, `parents` whose
  children are all done, `integrate` and `needsUser` (see parallel workers); `workers` counts
  this session's running workers.
- `task-start --plan P --ready` claims that next task. The claim under the plans lock
  decides, so two sessions asking at once get different tasks. It never takes over a task
  held elsewhere, stale or not; nothing is released by age.
- Every start of a task counts an attempt; continuing it with `--take-over` does not. A task
  allows three starts unless it sets `maxAttempts` (1..10); then the claim is refused until
  `plan-update` raises `maxAttempts` or abandons the task.
- `plan-release` returns a held task to `pending` for any session to claim and removes this
  session's binding to it; `--reason` becomes its next step. Another session's task needs
  `--take-over`; a parent is released after its held children.
- A take-over of an orphaned task carries nothing (`carried: false`); the task continues
  from the plan.

#### Child tasks

A plan task may name a `parent` (one level deep), the paths it owns, an engineering contract
and the shape of the result it returns:

```json
{"id": "api", "goal": "Add the API", "parent": "split", "dependsOn": ["model"],
 "acceptance": ["API exported"], "scope": {"paths": ["src/api/**"], "worktree": "wt/api"},
 "contract": "contracts/api.json", "returns": "The exported API"}
```

- A parent completes only after every child is completed or abandoned; a child cannot wait
  for its own parent. The session holding the parent can start a child and return to the
  parent with `task-start --plan P --task <id>`; the parent stays `running` meanwhile.
- `scope.paths` are project-relative patterns: `*` within a name, `**` across folders, and a
  pattern naming a folder covers everything below it. When the task starts, its claim
  records the source revision and the files already changed. `task-step --complete` is then
  refused with `scope exceeded` when a file committed or changed since the claim lies
  outside the task's paths (a parent also accepts its children's); widen them with
  `plan-update`. When the changed files cannot be told completely (no Git, more than 64
  files already changed at the claim), the step reports `scope: {checked: false, reason}`.
- `contract` is loaded as the engineering contract of `task-start --plan` unless
  `--contract` is given; [acceptance contracts](ACCEPTANCE.md) then gate completion.
- `returns` describes the expected result; the result given at completion is what dependent
  tasks see.
- `scope.worktree` is declared for the task and shown to it; it is not created or enforced
  (worker worktrees come from `plan-assign`, below).
- `plan` and `plan.md` list unfinished tasks that may work at the same time on intersecting
  paths under `overlaps`; this is reported, and refused only when a worker is assigned.

`task-start --plan` returns the task's envelope, also shown by `craft plan-envelope --plan P
--task T`: the task, its dependencies with their results and latest evidence, its parent
with a reference to the holder's last checkpoint (session, revision, time, next step), its
children, the latest eight decisions and the plan's references, at most 16 KiB. It carries
no conversation, other tasks' work or secrets. `--brief` emits it as a worker `brief`
([host control](HOST-CONTROL.md)): sources are the references and dependency results,
restrictions the owned paths.

#### Parallel workers

A session may delegate ready tasks that own paths to workers, which the host's agent tool
runs; `craft` claims, isolates and integrates, and starts nothing itself:

```sh
craft plan-assign    --root <project> --plan checkout-retry --task api --session <host-id> [--shared]
craft task-start     --root <project> --session '<host-id>/checkout-retry/api' --plan checkout-retry --task api   # the worker
craft plan-integrate --root <project> --plan checkout-retry --task api --session <host-id> [--escalate --reason "..." | --discard]
```

- `plan-assign` claims the task for the worker session `<session>/<plan>/<task>`; the
  assigning session keeps its own binding. It is refused for a task without `scope.paths`,
  at `efficiency.delegation.maxWorkers` running workers of this session (default 3, 0
  disables), and while a running task that may work at the same time owns intersecting
  paths. It returns the envelope, the worker `brief` and the worker's start command.
- Isolation: for each repository of the task (its `repositories`, else the root when it is a
  repository's top folder) a worktree is made at `.craft/worktrees/<plan>/<task>[/<repo>]`
  on the new branch `craft/<plan>/<task>` from the current `HEAD`. Add `.craft/worktrees/` to
  `.gitignore`; `plan-assign` refuses until Git ignores it, and refuses when the task's paths
  hold uncommitted changes that the worktree would miss. An existing branch or folder is
  refused, never reused. A plain folder, or `--shared`, has no worktree: the worker changes
  only its owned paths in the shared checkout, which the overlap rule protects.
- The worker's `task-start` binds its worktrees as repositories, and the scope check of
  `task-step --complete` runs there. A completion commits what the worktrees hold on their
  branches (consumer commit hooks apply) and records each branch head it verified.
- A completed worktree task readies dependents, and lets its parent and the plan complete,
  only once integrated; `plan-next` lists it under `integrate`. `plan-integrate` (by the
  assigning session) refuses a branch that moved since verification, a checkout in the middle
  of a merge or rebase, and uncommitted changes in files the branch changes. It tries the
  merge without touching the checkout (Git 2.38+); when clean, it merges each repository
  with one `--no-ff` merge commit, removes the worktrees and deletes the merged branches.
  Nothing is pushed.
- On conflicts nothing is merged: the task returns to `running` in the integrating session,
  counting one attempt, with the conflicted paths and the coordinator commit to merge. That
  session continues it with `task-start --plan P --task T` (bound to the worktrees), merges
  the commit in the worktree, resolves, verifies and completes it again, then runs
  `plan-integrate` again. When no attempt is left, or after `--escalate --reason`, the task
  `needsUser`: `plan-next` and the session-start line show it until it is integrated or
  discarded.
- `--discard` drops the result: the task is `pending` again and its worktrees are removed
  when clean; the unmerged branch is kept and reported. `plan-release` by the assigning
  session, or `plan-update` setting a worktree task back, removes its worktrees the same way.
- A worker assignment recorded through the [host control](HOST-CONTROL.md) with scope
  `plan:<plan>:<task>` that ends `failed`, `cancelled` or `not-started` releases its task.

Plans live in `.craft/state/plans/<plan-id>/`: `plan.json` is the record; `plan.md` beside
it is regenerated after every change for people to read and is never read back.
`plans/index.json` is a rebuildable summary. A plan-bound task binding is version 2; older
`craft` versions refuse it, so stop them before upgrading, as for other state changes. A
plan is written at the first version holding the fields its tasks use: version 4 once a
task is assigned to a worker (`owner.parent`, `work`, `integration`); version 3 once a
task records `attempts` (its first claim), `maxAttempts` or `verified`; version 2 when tasks
use `parent`, `scope`, `contract` or `returns`; otherwise version 1. Older versions refuse
newer plans.

Opt-in acceptance contracts (`--contract`, `--acceptance-evidence`) are described in
[acceptance contracts](ACCEPTANCE.md).

### Several repositories

From a folder that contains several repositories, select the task's repositories
explicitly (up to eight); siblings are never scanned:

```sh
craft task-start --root <workspace> --session <host-id> --task update-contract \
  --goal "Update producer and consumer" --repository producer --repository consumer
```

Each repository keeps its own revision and fingerprints; a change in any selected one
invalidates evidence. Each repository's own instructions and rules still apply. Missing
repositories are reported as unavailable, never silently dropped.

### Private scratch

`workspace-open` gives the task a private scratch directory, created only when asked for:

```sh
craft workspace-write   --root <workspace> --session <host-id> --operation investigation --data artifact.json   # {"name","content"}
craft workspace-promote --root <workspace> --session <host-id> --operation investigation --data promotion.json  # {"name","purpose"}
craft workspace-release --root <workspace> --session <host-id> --operation investigation
```

Managed writes are bounded and never overwrite. Promotion keeps a named file as evidence with
its source fingerprint; never promote credentials, transcripts or prompts. An operation's
owner is its session/task/operation identity. Completing the task removes scratch once no
worker scope is still open; `workspace-clean` reclaims closed and stale workspaces. At
capacity, the oldest inactive workspace is evicted first, then the oldest open one of another
task. `craft settings` shows the limits (`workspace.*`).

## Recommendations for review, delegation and compaction

```sh
craft task-offer        --session <id> --root <project> --action review --scope review:current-source --benefit "Check the risky change" --evidence checks/current --data facts.json
craft task-decision     --session <id> --root <project> --offer <id> --decision approved --evidence user-message:12
craft task-action-check --session <id> --root <project> --offer <id> --data facts.json
```

An offer states trigger, benefit and scope. Ask the user through the host's question
mechanism and record `approved` or `declined` only from a real answer; silence is never
approval. `facts.json` holds caller-verified observations (for example
`{"allowed":true,"supported":true}`), not grants. Source, policy, scope or risk changes
require a new evaluation.

- **Review** is offered for declared risks (`task-start --risk security`) on verified source.
- **Compaction** needs a safe boundary, fresh occupancy, a fresh continuation and reliable
  host compaction.
- **Workers** need a configured role/model mapping, more than one independent task and a
  fresh `workers` observation (capacity, occupied slots, session, time; valid for 30
  seconds). All workers of a task tree share one ceiling.

`ready: true` means the declared decision and prerequisites passed revalidation, not that
anything ran. Execution belongs to the host or to an integration driving the
[host controls](HOST-CONTROL.md).

## What is measured

| Measure | Source | Limits |
|---|---|---|
| Pack context | `context-report`: selected files, hashes, bytes, estimated tokens | Selection is not proof of loading; estimates are not occupancy |
| Context occupancy | A fresh, session-bound host observation (`usage.kind: "current-context"`) | Cumulative or billing tokens never count; otherwise unknown |
| Pack activity | Receipts of pack commands for the session | Not host tools or provider usage |

For separately installed packages, pass each package root with `--bundle`. `task-features`
and `task-feature` show and declare feature outcomes for evaluations; declarations are
marked as such.

## Context pressure and continuation

Occupancy bands default to elevated at 60%, high at 75% and saturated at 85% (`policy.*`).
Useful work continues; a high-pressure step is finished before context is reconsidered. At a
safe boundary a stable task with reliable compaction gets "checkpoint, then compact"; a
saturated session with stale assumptions gets "checkpoint, then new session". Unknown
occupancy produces no pressure advice. Session age alone never triggers a reset.

Before any context recommendation, the saved continuation must match the task and the current
repository fingerprints. A continuation has ten required fields (`goal`, `currentState`,
`completed`, `approach`, `evidence`, `filesChanged`, `validation`, `remainingWork`,
`blockers`, `nextStep`):

```sh
craft checkpoint-save --root <project> --session <id> --task <task-id> --data handoff.json
craft checkpoint-read --root <project> --session <id>
```

A new session adopts the task ([handoff](#session-identity-and-handoff)), then reads the
checkpoint, the current diff and routed guidance; it does not replay the conversation and never
renews authorization.

## Checkpoint and recovery

A checkpoint stores explicit metadata (task, authorization reference, next step, sources,
continuation) with Git HEAD, status and working-tree fingerprints. Recovery compares them
with the current tree. Check `changeDetection.complete`: `changed: null` means "unchanged"
could not be established, and evidence for a changed revision must be rechecked.

Automatic snapshots at turn end and context transitions are on by default
(`checkpoint.enabled`). Some hosts fire compaction hooks asynchronously, so save the
continuation at milestones rather than relying on a final callback.

## State storage

```text
.craft/
  project.json
  telemetry.json
  state/
    sessions/<session-id>/
      manifest.json         # created, workspace identity, host when a hook created it
      checkpoint.json
      task.json             # active task binding
      activity-<id>.json    # per-task activity, lookups and attempts
      workers-<id>.json  control-<id>.json  routing-<id>.json(.generation)
    telemetry/              # events.jsonl(.1), key.json, captures.json
    intelligence/           # project.sqlite(.lock), project.sqlite.backup
    workspaces/             # guard.json, <workspace-id>/{owner.json,scratch/,evidence/}
    migrations/             # receipts for records copied from earlier layouts
```

Session IDs are hashes of the host session; raw names never become paths. Each root, whether
a Git worktree, a folder of repositories or a plain folder, has its own `.craft/state/`;
linked worktrees have their own. Add `.craft/state/` (and `.craft/worktrees/` for parallel
workers) to `.gitignore`; Craft never edits ignore rules, and changes Git metadata only
through `plan-assign` and `plan-integrate` (worker worktrees, their branches and merge
commits). Lookup, snapshots, review and exports exclude state regardless
of ignores. `craft settings` shows the directory.

The previous layout kept the same folders under `.craft/state/<worktree-id>/` (with
`shared/` around the families). Its entries move into place one at a time on the next
access; one held by a writer, or whose place is already taken, waits for a later access and
`state-clean` reports it. Its evidence database is not moved: `refresh` builds the current
one and `state-clean` removes the old one.

State from earlier layouts (`.craft/state/<worktree-id>/<session-id>/` and
`shared/` files, `.git/craft-state/`, flat `.craft/state/` files, `.capabilities/state/`) is
copied on first access under its current name; only `state-clean` removes sources it copied.
The newest layout holding a record is its only source. Workspaces and the evidence database
start fresh. Stop older installed plugins before upgrading: they do not honor the current
locks, and a checkpoint, binding or activity record they change after the copy is refused
until the stores are reconciled. Telemetry events and routing hints they keep writing are
ignored.

```sh
craft state-clean --root <project>            # preview
craft state-clean --root <project> --apply    # remove what the preview lists
```

Session directories are kept until cleaned explicitly. `state-clean` lists each one of either
layout with its size and idle days, and removes it with `--apply` only when no record in it
changed for 14 days, it holds only records Craft writes, no lock or lease is held and no open
workspace may belong to it; its checkpoint goes with it. Anything else is kept with a reason.
A directory is set aside, listed again and removed only if unchanged; an interrupted cleanup
finishes on the next run.

Files earlier layouts left in `.craft/state/<worktree-id>/shared/` are removed with
`--apply` once nothing depends on them: the evidence folder while no process holds it, a telemetry log once copied, the telemetry key and captures once copied and
unchanged since, that layout's copy receipts, its evidence database while no process holds
it (`refresh` rebuilds the current one), and its workspaces when inactive and past evidence
retention, then their guard. A record never copied, a locked one or an unrecognized file is
kept with a reason. Removing a copied source never lets an older store become the record's
source again. Current families are only measured: `shared` lists each one's bytes
and files, or why it could not be measured, and what keeps it bounded. Telemetry rotates,
workspaces follow `workspace-clean`, the evidence database is rebuilt on request
(`project-evidence rebuild`, then `refresh`) and copy receipts are kept.

## Lookup reuse

`locate` scans all fresh literal terms in one bounded pass. With a session and task it also
reuses earlier results while the revision, scoped files, manifests and configuration are
unchanged; any change rescans the affected scope. `--data` accepts staged requests where
dependent terms are returned as deferred instead of run. No match is not proof of absence.

## Attempts and capability classes

```sh
craft session-attempt --root <project> --session <id> --task <task-id> --data attempt.json
```

Record acceptance attempts, not every tool call: `operation`, `target`, `approach`,
`evidence`, `outcome`, optionally `profile` and `escalationReason`. The same failure on
unchanged source counts against a budget (default three). The first no-progress failure
suggests a different approach; at the limit, return control or escalate along a configured
chain. Permission and tool failures stop as blocked; a stronger model never retries them.

| Class | Typical work |
|---|---|
| economical | Lookup, extraction, formatting, deterministic checks; prefer a tool first |
| standard | Routine implementation, focused debugging, tests, bounded review |
| strong | Architecture, ambiguous debugging, distributed consistency, security-sensitive work |

Classes are recommendations, not price rankings. Exact model names live in the consumer's
`execution` configuration; see [model advice](ROUTING.md#testing-only-scope-and-model-advice).

## Host support

| Host | Automatic occupancy | Native controls offered as advice |
|---|---|---|
| Claude Code | Status-line payload (current input and cache counts) | `/compact`, `/clear` |
| Codex | None mapped | `/statusline`, `/compact`, `/new` |
| Others | None | None mapped |

## Settings

`craft settings --root <project>` shows the effective policy, where each value came from
and the private state directory. Precedence: bundled defaults, personal preferences
(`~/.config/craft/preferences.json` or `--preferences`), project `.craft/project.json`, task
overrides (`--data`). Preferences and task overrides accept only `routing`, `checkpoint` and
`efficiency`; they cannot grant permissions or map models.

A fully manual policy:

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

Explicit commands keep working under any policy.
