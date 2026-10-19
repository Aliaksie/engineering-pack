# Routing and delegation

The router is a deterministic lookup, not another LLM conversation. Inputs are an explicit
task intent, changed paths, optional risk tags, project configuration and a host-provided
list of installed skills. Unknown intent is an error; hook prompt matching supplies only
conservative hints for explicit review/plan/locate requests.

```text
npm run route -- --root examples/project --intent review --file services/orders/Order.java
```

For a module inventory or direct owner lookup, use `node cli/main.mjs lookup --root
<project> [--file <path> | --module <name>]`. The most specific matching module owns a path.

The route returns:

- A [logical loading plan](PLAYBOOKS.md) chosen from workflow, practice and independent
  language/runtime/framework/layer/provider evidence, with inference and missing context.
- Logical skills and conditionally selected references.
- Matching module owners, transitive dependent modules and declared checks.
- Agent-provider references only for agent work using the selected provider; CI/CD
  provider detail only from affected pipeline paths or explicit task evidence.
- Execution mode, role, optional configured model and reasons.
- Tool plans and missing bindings/skills, with no work executed.

Logical playbook IDs such as `runtime:jvm` map to focused reference targets such as
`craft-engineering:foundations/references/languages/jvm.md`. Other selections load skills.
These targets are independent of native skill names. Hook
adapters add the host-native names; interactive callers resolve them through their host’s
skill list. `availableSkills` can be passed to the library to check actual installation.
Without that list the result explicitly says availability is unverified.

Module paths match directory boundaries. `dependsOn` supplies impact propagation, not
permission to edit every dependent. Module checks are command/argv records; the router does
not infer that a named test passed. The consumer owns the map and the agent verifies relevant
paths against the actual checkout. Architectural decisions are still read from source.

## Deterministic work and workers

| Work | Initial execution choice |
|---|---|
| Name/path location | Bounded repository search tool |
| Git status | Planned local git invocation |
| Snapshot, revision checks, report shape, checkpoint IO | Bundled deterministic scripts |
| Declared module check execution (`run-check`) | Tool plan; missing checks require verification design |
| Verification design/interpretation (`verify`) or manual exploration (`explore`) | Main thread |
| Planning, implementation, ambiguous ownership | Main thread |
| Review or bounded research | Main thread unless a delegated role is explicitly selected |

Batch independent lookup terms in one deterministic scan:

```text
npm run locate -- --root <project> --path src --term Handler --term configKey --max-matches 20
```

The command respects Git ignores, excludes symlinks/binary/oversized files, and reports work
and output limits. Without a Git inventory, explicit paths are required. Matches give
path/line/snippet evidence; `termIndexes` point into the top-level term list. They do not
classify definitions or prove absence. This applies to ADR indexes/templates/records as well
as code. A bounded economical worker can classify/extract cited evidence when needed; keep
architecture choices, product decisions and final review judgments with the reasoning task.

`--delegate` selects a configured worker role only when both that role and the selected
host enable it. A logical profile is mapped to a concrete model in that host’s project
configuration. Missing mappings are reported; no model alias or universal “cheap model”
is hardcoded. Routing never spawns a worker or extends the user’s original authorization.
The caller briefs the worker with scope, evidence bar, affected files and compact return shape.
For lookup, configure `execution.roles.locator` with a logical `lookup` profile and map it in
the selected host's `execution.hosts.<host>.models`. Delegation remains explicitly selected
and authorized. Avoid passing the worker the accumulated parent conversation; reuse its
compact source-scoped packet across stages and refresh only affected evidence.

For implementation workers, assign independent outcomes and paths, establish a base revision,
and isolate concurrent writers with separate branches/worktrees where supported. Read-only
lookup normally needs no extra checkout. The coordinator owns review and integrated checks;
no runtime command here creates worktrees or commits. See
[Git discipline](../packages/craft-engineering/skills/foundations/references/git-discipline.md#parallel-changes-and-worktrees)
and [worker handoffs](../packages/craft-engineering/skills/foundations/references/context-discipline.md#assign-work-that-can-finish-independently).

Prompt hooks return compact loading hints rather than full catalog evidence. Per-edit hints
are off by default; a consumer can opt in with `routing: { "editHints": true }`. Full routing
data stays available through the CLI. Configured service metadata is prepared on supported
session-start/resume hooks; this is local metadata, not live discovery or authentication.

Optional `routing: { "dedupeHints": true }` stores only the last emitted hint's hash and
timestamp under `.capabilities/state/`, scoped by session and host. Ignore this directory in
Git. Identical hints are suppressed for at most 15 minutes; changed hint text, project
configuration or router/catalog source causes emission again. Session start/resume and
compaction events invalidate reuse through a small generation marker, including when an older
hint is still being emitted. Scope changes that produce the same routing text need
no new message; this is hint reuse, not cached repository evidence or verification.

Only host events that can emit context consume a reuse entry, after serialization.
Unsupported delivery, missing session identity, busy state, malformed state and read/write
errors never justify suppressing a fresh hint or blocking work. State paths reject symlinks,
use a writer lock and atomic replacement. A busy or abandoned lock can cause repeated hints;
it cannot turn a permission denial into an allow. This setting does not enable checkpoints,
cache tool schemas, save prompts or change authorization. It remains off by default.

## Guidance size without a live token meter

```text
npm run context:report -- --root examples/project --intent review --file services/orders/Order.java --budget-bytes 20000
```

`context-report` uses the same routing plan and reads only selected entry files, explicitly
selected references and required package instructions. It reports logical identities, paths,
SHA-256 hashes, unique-path bytes, repeated selections and identical content. Linked detail
is not recursively loaded. Estimates use `ceil(UTF-8 bytes / 4)` per unique path; this is a
disclosed heuristic, not a tokenizer or measurement of actual host loading.

`--budget-bytes` is optional and report-only: an overrun never silently removes guidance.
Missing optional packages/files make totals a lower bound and the within-budget status
unknown. The report excludes host discovery metadata, tool schemas, conversation and tool
output; current session occupancy and capacity remain unknown. Use native host observations
for live usage, not these file estimates or cumulative billed tokens.
Full exports measure their package resources; host-discovered overlay copies may contain
additional wrappers. This report does not claim to measure those copies or native UI rules.

The CLI defaults to its own source/bundle/package root. `--bundle <path>` selects an explicit
source checkout, full export or standalone package to inspect; `--root` remains the consumer
project configuration. Installed packages use `node support/cli/main.mjs context-report`.
The shared implementation ships in every adapter and does not inspect user plugin caches,
activate tools, change models or emit per-tool context reports.

Use `verify` to choose or interpret acceptance evidence. Use `run-check` with affected paths
to plan the consumer's declared module commands; commands remain `not-run` until a caller
executes them within authorization. Use `explore` for a bounded manual testing charter or
actual observations when an application and tools are available. It selects only the
exploratory workflow, without assuming a language, repository, runner or browser connector.

## Checkpoint and recovery

```text
npm run checkpoint:save -- --root <project> --session <id> --data <metadata.json>
npm run checkpoint:read -- --root <project> --session <id>
```

Metadata may contain `task`, `taskId`, `authorization`, `nextStep`, `sources` and a bounded
`continuation` object described in [session status](SESSION.md#continuation-checkpoint).
Application checkpoint use cases add observed Git HEAD, working-tree status, bounded
working-tree/index fingerprints and a timestamp through platform ports. They do not guess
task completion, capture prompts/transcripts or turn saved authorization into new authority. Recovery compares
saved state with the current tree and flags changes; verification evidence must be rechecked
when its revision no longer matches.

Inspect `changeDetection.complete` before relying on the comparison. `changed: null` means
unchanged state cannot be established. Inspection limits, read failures, submodules and older
checkpoints without fingerprints leave coverage incomplete; they do not establish freshness.

Automatic snapshots require `checkpoint.enabled: true` in trusted project configuration.
They run at supported tool/milestone/compaction events. Writes are atomic and reject symlinked
state paths; concurrent writers are reported as busy. Ignore `.capabilities/state/` in Git.
Because some hosts expose asynchronous/observational compaction hooks, persist semantic
metadata at milestones rather than relying on a final callback. Read recovery state explicitly
on hosts with no after-compaction injection. This preserves continuity; it does not replace
the host’s summarization algorithm or promise lossless context compression.

Risk tags such as `failure-path`, `timeout`, `retry`, `partial-write` and `cancellation`
select the cross-language error-handling reference without requiring Node or a provider.
For example: `npm run route -- --intent implement --file src/Main.java --risk timeout`.
A risk selects guidance; it is not evidence that the corresponding defect exists.

## Testing-only scope and model advice

`implement` selects product-code delivery. `verify` selects the testing-only workflow;
`--verification-mode manual|automation|test-code` distinguishes observations, existing
checks and explicitly requested test changes. Manual mode omits stack guidance. All modes
exclude module-added implementation skills; product fixes require separate authorization.
This routing contract does not replace host permissions. `explore` remains a focused charter.

`npm run model:advice -- --root <project> --data <request.json>` recommends a configured
logical profile without calling a model. Requests name `host`, `role`, `task`, `revision`,
optional `requiredCapabilities`, `capabilityClass`, `currentProfile` and bounded `attempts`. The `verifier` role is available.
Consumer `execution.profiles` declares capabilities, `maxAttempts` (1–5, default 1) and
optional `class` (`economical`, `standard`, `strong`) and `escalateTo`; existing role/host
delegation settings and exact model mappings
remain necessary. Profiles and escalation chains are optional and contain no vendor defaults.

Each attempt supplies a unique `id`, `profile`, `task`, `revision`, `outcome` and `evidence`
reference. Only matching task/revision quality failures count toward escalation. Capability
names are declarations, and cited outcomes are caller records, not independent measurements.
Current-target `permission-denied` or `tool-unavailable` stops advice across all profiles;
resolve that prerequisite before supplying a fresh attempt set. No chain means stop rather
than invent a stronger model. A configured host mapping alone establishes no quality benefit.
