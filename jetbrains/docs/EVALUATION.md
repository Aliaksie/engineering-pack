# Paired workflow evaluation

## Discovery reuse and persistent knowledge gate

Measure the existing [bounded lookup reuse](SESSION.md#reuse-before-bounded-discovery)
before proposing persistent observations. Freeze source and implementation identities,
query/scope, configuration, runtime and acceptance criteria. Retain raw requests/results,
timings and the available `telemetry-report`; its retained events are not complete history.
Use the existing paired-eval protocol below for model comparisons.

Compare fresh batched lookup, validated task-bound reuse and explicit source references.
Alternate operation order and retain individual samples. Measure end-to-end lookup and
inventory revalidation separately; inventory timing is a component measurement, not an
additive estimate of total overhead. Report source bytes read, matches, skipped/truncated
coverage and exact literal-result agreement. A known-file read with a checked content
identity measures retrieval only; it cannot replace discovery of unknown callers or prove
semantic relevance. Filesystem/process timings do not measure model latency or billing.

Classify misses using controlled changes or attributable evidence: first lookup, changed
session/task, changed source/revision/configuration, incomplete coverage, capacity eviction,
disabled reuse and unavailable state/inventory. Explicit repository selections currently
scan fresh. The existing telemetry aggregates hits/misses; it does not distinguish all these
causes. An unattributed production miss stays unknown. Do not infer semantic rediscovery
from a cache miss, repeated command text or repeated reads.

The initial discovery assessment on 2026-10-22 returned **no-go for persistent knowledge**.
Ten alternating samples per case replayed frozen source from the development pilot and
the QA/product projection implementations in disposable Git repositories on macOS/Node 24.
These are small deterministic source replays, not three real user tasks or cold OS-cache runs.

| Source case | Fresh lookup median | Validated reuse median | Inventory median | Known-source read/hash median | Fresh source bytes |
|---|---:|---:|---:|---:|---:|
| Development pilot | 22.27 ms | 87.33 ms | 42.14 ms | 0.071 ms | 4480 |
| QA projection | 22.21 ms | 87.09 ms | 42.24 ms | 0.053 ms | 3357 |
| Product/architecture projection | 22.20 ms | 87.49 ms | 42.51 ms | 0.049 ms | 2274 |

All 30 warm lookups matched fresh literal results and read zero source bytes. Controlled
task/session changes, absent identity, source/manifest edits and incomplete prior scans
all scanned fresh. These results establish local mechanics; the small scopes do not
establish cache performance on large repositories. Keep timing diagnostic-only.

The retained real development pilot has one task in two model arms: 13/19 completed shell
commands, with five commands containing read/search operations in each arm and no explicit
Craft lookup invocation. This lexical classification does not measure semantic effort or
show that existing reuse was exhausted. QA/product examples are synthetic. Repeated real
tasks, a useful durable fact with measured retrieval benefit, and held-out acceptance
quality remain unavailable. Model-cost savings, native-host behavior and cross-OS performance
are unknown. No runtime behavior or cache policy changed in this assessment.

Reopen the gate only when repeated costly semantic discovery remains after existing reuse
and cited source references. Name one bounded observation and its dependencies; compare it
on held-out real tasks under equal tools/authorization and unchanged acceptance thresholds.
Require lower measured retrieval/revalidation cost without correctness, freshness or isolation
regressions. Otherwise retain the no-go and add no persistent-knowledge feature.

## Paired model runs

Do not assume the current Git HEAD includes working changes. Obtain a fixed repository
snapshot, including relevant uncommitted sources, before paired model runs and record its
identity in each report. Do not label deterministic routing fixtures as evidence of better
engineering output.

Use the same host, model, repository snapshot, prompt, tools, authorization and acceptance
criteria in each arm. Baseline receives the ordinary project instructions and tools without
this suite. Candidate additionally receives only the selected playbook context. Do not weaken
the baseline. Start each arm with a clean checkout/session and record missing prerequisites.

Package scenarios cover failure policy, verification boundaries, test reliability,
duplicate-delivery/crash windows and required-audit failure.
These are prompts/rubrics, not model runs. The [tool and hook audit](authoring/TOOLS.md) records static
behavior and measurement gaps.

Initial cases:

| Workflow | Task | Acceptance evidence |
|---|---|---|
| Investigate/plan | Diagnose a Kotlin worker blocking a coroutine path | Cited call path, runtime/version evidence, bounded options and executable checks; no Spring assumption |
| Implement | Correct an observable timeout failure in a Java service | Reproduced failure, focused fix, meaningful verification and transaction boundary evidence |
| Review | Review a React change introducing inaccessible error feedback | Revision-bound finding, affected interaction, severity, verification recommendation |
| Implement | Small browser TypeScript change | Accepted behavior with no Node/server assumptions or unnecessary tests/context |
| Task authoring | Refine a local story with incomplete requirements | Grounded acceptance criteria and explicit unknowns; no implementation/provider loading |
| Review | Review a Bedrock agent retry change | Pinned SDK evidence, side-effect/idempotency analysis and provider isolation |
| Manual QA | Prepare password-reset exploration without code or browser access | Bounded charter, grounded expected behavior, test prerequisites and no invented execution |
| Automation QA | Investigate a parallel-only integration failure | Discriminating isolation check, preserved failure evidence and no unauthorized cleanup |
| Architecture | Assess a migration with conflicting proposed and accepted decisions | Cited decision status, compatibility risks, bounded alternatives and explicit unknowns |

For the manual QA case use `craft-engineering/evals/exploratory-testing.json`; for automation
QA use `craft-engineering/evals/test-reliability.json`. Keep product refinement and architecture
inputs usable without a tracker. New routing/context fixtures are deterministic evidence only;
they do not constitute any of these paired model runs.

For each arm retain the actual response/artifacts, judged correctness, accepted completion,
critical defects, safety violations, rework, unrelated changes, evidence quality, loaded
references, tool/model calls, tokens/cost when available, wall time and retries. Record
unavailable measurements as unknown, not zero. Failures and incomplete runs remain visible.
Use the existing `eval:report` format for package scenario verdicts; retain paired measurements
alongside the report rather than claiming the report checker judges semantic quality.

Compare workflow guidance, specialization and deterministic tooling separately when practical.
Include easy cases where extra context can hurt. Report inconclusive results. Only then decide
which deferred content to improve, merge or remove and whether independent packages are useful.

For a local pilot, freeze the supplied fixture and selected guidance with content hashes.
Use isolated baseline/candidate copies, preserve actual responses and changed artifacts,
and independently rerun executable acceptance checks. Give a separate reviewer the predefined
rubric and responses with neutral labels; normalize arm names in paths without changing their
meaning. Preserve original responses alongside that presentation. Disclose any label leakage,
missing model identity or unobserved cost/timing. A single paired task can expose a concrete
failure, but cannot establish general benefit or settle a collection's retention.

Preflight fixture configuration and tool paths before model runs. An intentionally failing
behavior assertion is useful; an unintended schema or missing-file error is a setup failure.
Preserve invalid runs and correct the fixture before a fresh comparison, rather than judging
the guidance from an unavailable tool path or silently rewriting the original evidence.

## Collect one frozen pair

`npm run eval:pair -- prepare <config.json> <new-output-directory>` freezes a fixture,
common prompt and selected guidance. `npm run eval:pair -- run <prepared-output-directory>`
explicitly executes two runner calls. Preparation makes no model calls. Optional
`order: ["candidate", "baseline"]` reverses the default order; record and alternate order
across matched tasks rather than always running the baseline first.
Existing outputs, changed inputs and repeated/concurrent runs are rejected. A failed or empty
first run stops collection rather than repeating a setup failure. Failures remain artifacts.
Both arms are checked before collection and each arm is checked again immediately before
invocation. If the preceding runner changes the next prompt or workspace, collection stops
and retains the first result without a completed-pair summary. These checks detect changes;
they do not isolate files from concurrent writes during a runner's execution.

The configuration supplies `fixture` (directory), `prompt` (text file), `guidance` (file array)
relative to the config, and `runner: { command, args, identity, timeoutMs }`. The runner reads
the prompt from stdin and returns its response on stdout; `{workspace}` in argv resolves to
the isolated arm copy. Commands run with `shell: false`; timeout is bounded to five minutes.
Choose a read-only runner/sandbox and existing authentication explicitly. The harness inherits
the caller's environment and does not create credentials, restrict network access or sandbox
the process by itself. Do not include secrets in configuration or fixtures.

Both arms receive the ordinary fixture unchanged; candidate gets only the selected guidance
appended to its prompt. Retained artifacts include fixture/guidance/prompt hashes, source Git
state, stdout, stderr, exit/error, wall time and post-run workspace inventory. `identity` is
a declaration: verify actual model/provider/settings from the runner evidence. The collector
does not infer token billing or a semantic verdict; record available native counters with
their precise meaning separately. Inspect observable tool effects, not just file changes.

Run acceptance checks and assess the original criteria against actual outputs. Record who
judged, blinding limitations, tuning overlap and failures. One baseline-first pair is a pilot,
not statistical evidence; vary order/repetitions for decisions about retaining guidance.

## Compare actual runtime bundles

For integration evaluation, replace `guidance` with
`bundles: { baseline: "path/to/frozen-baseline", candidate: "path/to/frozen-candidate" }`.
Both prompts stay identical. The runner must explicitly consume `{bundle}` and `{profile}`
arguments using the target host's supported controls. Preparation copies and hashes both
actual bundles, creates fresh profile directories and never reads an active user profile.
Optional `profileFiles: { baseline: { "settings.json": "baseline-settings.json" },
candidate: { "settings.json": "candidate-settings.json" } }` copies only those explicit,
bounded files. Paths resolve relative to the config; do not include credentials.

The runner owns isolated installation, trust, authentication and native startup. Copying
files and a successful process exit do not prove a plugin was loaded. Retain native startup,
actual model, worker and action receipts with the outputs. The collector inherits its
environment, so a profile argument alone does not isolate ambient host settings or credentials.
Use a runner that actually directs the host to the fresh profile and prevents unintended
fallback to the active installation.

Both bundle/profile inventories are checked before execution and at each arm boundary.
Post-run inventories retain profile changes and detect mutations of the frozen bundle;
inspection failures stop collection. These checks are evidence collection, not a sandbox.
Token usage, cost and semantic verdict remain unknown until supplied by actual native
measurements and acceptance review. Test runners establish the collector contract only.
