# Commands

Every exported package ships one native executable, `craft`, behind generated launchers:
`<package-root>/bin/craft` for POSIX shells (including Git Bash) and `bin\craft.cmd` for cmd
and PowerShell. A source checkout runs the same commands through the `npm run` scripts in
`package.json` or as `target/debug/craft` after `cargo build`.

`craft --help` lists the commands; `craft <command> --help` prints usage and effects. Routine
receipts are compact; `--json` or `--verbose` returns the full structured result. Failures
carry a stable error `code` with exit status 1; callers branch on the code, never on the
message. Git and `gh`/`glab` are prerequisites only for operations that need them.

No command grants permission. A routing, service or publication plan describes work; it does
not execute or authorize it. No command calls a model, switches models, starts workers,
compacts a host session or posts to a code host.

## Routing and lookup

| Command | Purpose | Effects |
|---|---|---|
| `route` | Intent, paths and risks → guidance loading plan, owners, checks and execution role | Reads configuration; see [routing](ROUTING.md) |
| `lookup` | Module inventory or owners of paths | Reads configuration |
| `locate` | Batched literal terms in scoped paths → path/line/snippet evidence | Bounded scan; optional task-scoped reuse receipts |
| `context-report` | Selected guidance → paths, hashes, bytes, estimated tokens | Reads package resources; never measures live occupancy |
| `model-advice` | Attempts and configured profiles → profile recommendation | Pure decision; no model call |

## Tasks and sessions

| Command | Purpose | Effects |
|---|---|---|
| `task-start` | Bind a task to the session, optionally with repositories and an acceptance contract; `--plan P --ready` claims the plan's next ready task | Writes task state; snapshots selected Git worktrees |
| `task-adopt` | Continue an earlier session's unfinished task in this session | Copies its task state and checkpoint; the earlier session is unchanged |
| `task-step` | Record a milestone, evidence and continuation | Writes task state; advances the held plan task |
| `plan`, `plan-create`, `plan-update` | Show, create or change a work plan whose tasks sessions claim with `task-start --plan` | Writes the local plan record and its Markdown view; never reads a tracker |
| `plan-next` | A plan's next ready task and its held, stale, orphaned, waiting, retryable and exhausted tasks | Reads the plan; claims nothing |
| `plan-release` | Return a held plan task to pending | Writes the plan record; removes this session's binding to the task |
| `plan-assign` | Delegate a ready plan task that owns paths to a worker session, in isolated worktrees unless `--shared` | Writes the plan record; creates `.craft/worktrees/<plan>/<task>-<attempt>` on branch `craft/<plan>/<task>-<attempt>`; starts no worker |
| `plan-integrate` | Merge a completed worktree task into the coordinator checkout, hand its conflicts back, `--escalate` or `--discard` it | Clean: one merge commit per repository, worktrees and merged branches removed, nothing pushed; conflicts: merges nothing |
| `plan-envelope` | One plan task's small context: goal, acceptance, owned scope, dependency results, parent checkpoint reference; `--brief` as a worker brief | Reads the plan and the parent holder's checkpoint metadata |
| `session`, `session-advice` | Status and next-action advice from task state and host observations | Reads state; recommendation only |
| `session-attempt` | Record one bounded attempt and its outcome | Writes a receipt; executes no retry |
| `task-offer`, `task-decision`, `task-action-check` | Recommend, record a user decision on, and revalidate a review, delegation or compaction | Writes receipts; dispatches nothing |
| `task-feature`, `task-features` | Classify and list feature outcomes for evaluation | Writes/reads receipts |
| `worker-status` | Worker reservations and host observations for a task tree | Reads state |
| `settings` | Effective settings, value origins and the private state directory | Reads configuration |
| `workspace-open`, `-write`, `-promote`, `-release`, `-clean` | Bounded private scratch for the active task | Writes only under private state |
| `state-clean` | Preview, or with `--apply` remove, session directories idle for 14 days, plans completed or abandoned 14 days ago; report open plans and shared family sizes | Reads state; `--apply` removes only what the preview lists |

See [session behavior](SESSION.md).

## Checkpoints

| Command | Purpose | Effects |
|---|---|---|
| `checkpoint-save` | Save explicit task metadata with Git state | Local Git reads; atomic, locked state write |
| `checkpoint-read` | Compare a saved checkpoint with the current tree | Read-only |

Recovery reports `changeDetection.complete`; `changed: null` means unchanged state could not
be established. See [continuation checkpoints](SESSION.md#checkpoint-and-recovery).

## Services

| Command | Purpose | Effects |
|---|---|---|
| `prepare-services` | Configured bindings and observed tool names → compact operation map | Pure; no discovery, probes or credentials |
| `operation` | Operation, parameters and binding → CLI argv or MCP call plan | Pure; no subprocess or network call |

See [integrations](INTEGRATIONS.md).

## Review

| Command | Purpose | Effects |
|---|---|---|
| `review-prepare` | Committed base/head → snapshot with identities and diff | Bounded Git reads; writes a new artifact only |
| `review-check` | Snapshot and review result → structural verdict | Reads artifacts; does not judge findings |
| `review-publication` | Checked review, current remote revisions and authorization → comment plan | Reads artifacts; never contacts a code host |
| `review-followup` | Fetched pull request observation → next follow-up status | Reads one file; stores nothing |

See [review](REVIEW.md).

## Acceptance

| Command | Purpose | Effects |
|---|---|---|
| `grounded-context` | Cited task context from supplied sources | Read-only |
| `acceptance-view` | QA, product or architecture view of a contract and its evidence | Read-only |

See [acceptance contracts](ACCEPTANCE.md).

## Telemetry

| Command | Purpose | Effects |
|---|---|---|
| `telemetry-report` | Retained local events → counts, loss and limits | Reads the local log |
| `telemetry-capture`, `telemetry-forget` | Retain or remove an explicit report | Writes private state |

See [observability and privacy](OBSERVABILITY.md).

## Project evidence

`project-evidence version|refresh|status|query|backup|rebuild --enable` maintains an opt-in
local index of declarations and imports. See [project evidence](PROJECT-EVIDENCE.md).

## Host entry points

These are called by hosts and integrations, not typed by users:

| Entry | Caller |
|---|---|
| `craft hook <host> <lifecycle>` | Generated hook configuration; reads the host payload and writes its wire output ([adapters](ADAPTERS.md)) |
| `craft statusline <host> --package <name>` | Host status line or an explicit status read ([installation](INSTALL.md#first-use)) |
| JSON request on standard input (`workers`, `actions`, `control`) | An integration that drives worker, review and compaction phases ([host controls](HOST-CONTROL.md)) |

Without a bundled executable for the platform, the launcher answers as a failed runtime
would: a shell enforcement hook denies, observation stays silent and context delivery reports
itself unavailable.
