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

## Decision replay

`tests/decision-replay.test.mjs` replays realistic prompts and session situations through the
real routing hook, without a model. Each case in `tests/fixtures/decisions/` records the
decision a user would expect and the decision Craft makes today. A changed decision fails
until `current` is updated on purpose; the distance to `expected` is reported (in CI, on
the job summary) but not enforced, so the fixtures double as a scorecard. Add a case when a
real session shows a wrong or missing decision.

## Session audit

`npm run eval:audit -- <transcript.jsonl>` prints an engagement card for one real session. It
joins the host's own transcript (a Claude Code session file or a Codex rollout) with the
events Craft recorded for the same session in that project: tool calls, shell searches,
rereads and tokens next to hooks and their latency, routing, selected guidance, lookups,
checkpoints and failures. Signals such as `no-routing`, `search-without-lookup` or a failure
repeated on every prompt point at sessions worth a closer look; they are not a grade.
`--json` prints the same card as data and `--root` names the project when the transcript
does not.

Nothing leaves the machine. Transcript formats belong to each host, so values a transcript
does not carry show as unknown. Craft's local log rotates, so audit a session soon after it
ends, or retain its summary with `npm run telemetry:capture -- --session <id>`.

## Engineering eval

The engineering eval (`evals/` in the source repository) runs a real coding agent in a container against
frozen tasks, then grades the result with hidden checks and with what Craft recorded. Its
qualification job (no model) runs in CI when tasks change; model-backed jobs run only on
manual dispatch. Runtime cost is measured separately by the performance workflow.

When comparing a baseline with Craft, keep the host, model, repository snapshot, prompt,
tools, authorization and acceptance checks the same, and start each arm clean. Retain every
attempt, including failures and timeouts. A single pair can expose a failure; it is not
evidence of general benefit. Alternate order and repeat before deciding to keep, change or
remove guidance.

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
