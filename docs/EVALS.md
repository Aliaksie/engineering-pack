# Portable evaluations

Each capability ships versioned scenarios under `evals/`. A scenario contains a self-contained
prompt and named pass/fail criteria. Use the same scenario against each exported host package;
record the actual response and evidence, not just a summary saying it worked.

Run model evals through your chosen host or eval harness, with live side effects disabled
unless explicitly authorized. Compare baseline and candidate against the same cases. Include
provider failures, incomplete runs, latency and cost where relevant in the surrounding evidence.
No model runner or cloud credential is assumed by this repository.

The `adapter` must name a supported export: `claude`, `codex`, `cursor`, `copilot`,
`gemini` or `agents`. Selecting a host records where the evidence was obtained; it does
not certify that host or replace the actual response and criterion evidence.

A result consumed by `npm run eval:report -- <file.json>` has this shape:

```json
{
  "package": "craft-engineering",
  "packageVersion": "0.0.1",
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

The report checker validates case/version identity, complete criterion coverage, boolean
verdicts and evidence, then summarizes those recorded verdicts. It does not call a model or
verify that a judge’s reasoning is correct. Exit 0 means all recorded criteria passed;
1 means a recorded failure; 2 means an invalid report. Never count provider errors or missing
responses as passes. Keep real transcripts and credentials out of committed results.

Node tests exercise deterministic behavior, build portability and report validation.
They are not semantic evaluations. No cross-platform model quality claim follows until
actual responses are collected and reviewed.
