# Paired workflow evaluation

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
