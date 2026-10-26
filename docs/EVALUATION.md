# Evaluation

Repository tests establish deterministic behavior and packaging. Whether guidance improves
model output needs actual responses judged against fixed criteria. Never count provider
errors, missing responses or static checks as passes; record unmeasured values as unknown.

## Package scenarios

Each package ships scenarios under `evals/`: a self-contained prompt and named pass/fail
criteria. Run them through your host or harness with live side effects disabled, and record
the actual response with per-criterion evidence:

```json
{
  "package": "craft-engineering",
  "packageVersion": "0.0.2",
  "case": "mixed-stack",
  "adapter": "codex",
  "model": "actual-model-id",
  "revision": "source-revision-or-working-tree-id",
  "evaluator": "human-or-versioned-judge-id",
  "response": "The actual model response goes here.",
  "verdicts": [
    { "criterion": "c1", "passed": true, "evidence": "Observed selection and source citation." },
    { "criterion": "c2", "passed": false, "evidence": "Observed mismatch and its location." }
  ]
}
```

`npm run eval:report -- <file.json>` checks identity and complete criterion coverage, then
summarizes the verdicts: exit 0 all passed, 1 a recorded failure, 2 an invalid report. It
does not call a model or judge the judge. `adapter` names an export (`claude`, `codex`,
`cursor`, `copilot`, `gemini`, `agents`, `gemini-extension`, `jetbrains`). Keep transcripts and
credentials out of committed results.

## Paired runs

Compare a baseline and a candidate with the same host, model, repository snapshot (including
uncommitted sources), prompt, tools, authorization and acceptance criteria. The baseline gets
the project's ordinary instructions; the candidate additionally gets only the selected
guidance or bundle. Start each arm clean. For each arm retain the response and artifacts,
correctness, critical defects, safety violations, rework, unrelated changes, tool and model
calls, tokens or cost when available, wall time and retries.

Suggested cases cover investigation, implementation, review, story refinement, manual and
automation QA and architecture, plus easy cases where extra context can hurt. Judge with the
rubric and neutral arm labels, and rerun executable acceptance checks independently. A single
pair is a pilot that can expose a failure, not evidence of general benefit. Alternate order
and repeat before deciding to keep, merge or remove guidance.

## Collecting a frozen pair

```sh
npm run eval:pair -- prepare <config.json> <new-output-directory>
npm run eval:pair -- run <prepared-output-directory>
```

`prepare` freezes the fixture, prompt and guidance with hashes and makes no model calls.
`run` executes the two runner calls, in `order` if given. The configuration names
`fixture`, `prompt`, `guidance` and `runner: {command, args, identity, timeoutMs}`. The
runner reads the prompt on stdin, answers on stdout and receives `{workspace}` (the isolated
arm copy). Commands run without a shell and with a timeout of at most five minutes. The
harness inherits your environment and is not a sandbox: choose a read-only runner and keep
secrets out of the configuration. Changed inputs, a failed first run or a workspace changed
by the previous arm stop collection; failures stay as artifacts.

To compare two complete bundles, replace `guidance` with `bundles: {baseline, candidate}`.
The runner must consume `{bundle}` and `{profile}` itself; `profileFiles` copies explicit
settings into fresh profile directories. A copied bundle and a clean exit do not prove the
plugin loaded. Retain native startup, model and worker evidence.

## Cost and context changes

Run separate experiments, never changing content and model together:

| Experiment | Varies | Needs |
|---|---|---|
| Deterministic extraction | Ordinary discovery vs bounded extraction/reuse | Result agreement, missed scope, bytes read, time |
| Reduced context | Current vs one reduced guidance bundle | Observed loads, native usage, unchanged acceptance quality |
| Economical reasoning | Standard vs consumer-mapped economical profile | Observed model and settings, acceptance, retries, native cost |

Freeze cases, hashes, prompts, settings, order, repetitions, rubric and adoption scope before
collecting. Keep tuning and held-out cases apart. Reject a candidate on any correctness,
scope, permission, stale-evidence or fabricated-completion regression. Adopt it only within
the evaluated task classes, after held-out outcomes meet the unchanged rubric and the
intended measure improves. Inconclusive results keep the current policy. Unknown prices
cannot support savings claims, and fewer loaded bytes do not prove fewer billed tokens.

## Recorded assessment: persistent knowledge

On 2026-10-22, a measurement of the existing lookup reuse returned **no-go** for a
persistent knowledge store. Validated reuse (about 87 ms) was slower than a fresh bounded
lookup (about 22 ms) on small source replays. All warm results matched fresh ones, and every
controlled change rescanned. Reopen this only with repeated costly discovery on real held-out
tasks, under equal tools and unchanged acceptance, and with measured lower cost and no
correctness or freshness regressions.
