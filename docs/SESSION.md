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

A new session reads the checkpoint, the current diff and routed guidance; it does not replay
the conversation and never renews authorization.

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
    <worktree-id>/
      sessions/<session-id>/
        manifest.json         # created, workspace identity, host when a hook created it
        checkpoint.json
        task.json             # active task binding
        activity-<id>.json    # per-task activity, lookups and attempts
        workers-<id>.json  control-<id>.json  routing-<id>.json(.generation)
      shared/
        telemetry/            # events.jsonl(.1), key.json, captures.json
        evidence/             # project.sqlite
        workspaces/           # guard.json, <workspace-id>/{owner.json,scratch/,evidence/}
        migrations/           # receipts for records copied from earlier layouts
```

IDs are hashes of the canonical root and session; raw names never become paths. Git and
non-Git roots use the same layout; linked worktrees get their own ID. Add `.craft/state/`
to `.gitignore`; Craft never edits ignore rules or Git metadata. Lookup, snapshots, review
and exports exclude state regardless of ignores. `craft settings` shows the directory.

State from earlier layouts (`.craft/state/<worktree-id>/<session-id>/` and
`shared/` files, `.git/craft-state/`, flat `.craft/state/`, `.capabilities/state/`) is copied
on first access under its current name, and the sources are left untouched. The newest
layout holding a record is its only source. Workspaces and the evidence database start
fresh. Stop older installed plugins before upgrading: they do not honor the current locks,
and a checkpoint, binding or activity record they change after the copy is refused until
the stores are reconciled. Telemetry events and routing hints they keep writing are ignored.

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
