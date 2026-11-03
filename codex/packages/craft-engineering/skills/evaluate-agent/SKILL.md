---
name: evaluate-agent
description: "Compare agent behavior on matched tasks with retained outputs, acceptance evidence and explicit limits."
---

<!-- Generated from packages/; edit the portable source and rebuild. -->

Read the shared [package instructions](../../AGENTS.md) when using this capability.

# Evaluate an agent change

Choose representative tasks and acceptance criteria before running the comparison. Include normal,
failed and adversarial paths plus a small task where additional guidance could waste work. Separate
objective contract checks from judgment about usefulness, reasoning or groundedness.

Run baseline and candidate with matched task inputs, tool permissions and relevant configuration.
Record model/provider identity, revision, raw output, tool effects, acceptance results and observable
time/cost. Provider outages and missing tool access are distinct from behavioral failures. Keep
failures and regressions visible; do not report a schema-valid result as semantic success.

Use held-out tasks where practical and disclose tuning overlap. For stochastic behavior choose
repetitions proportionate to the decision. Review evaluator calibration with concrete examples,
separate generation from judgment when feasible and record disagreement. Diagnose whether missing
evidence came from retrieval or interpretation before changing both at once.

Evaluation runs must stay inside authorized resource and effect boundaries. Portable scenario files
provide prompts and criteria; they are not execution evidence. The authoring `eval:report` command
checks recorded evidence structure, whereas installed packages use the consumer's evaluation tools.
Report the scope of any measured benefit rather than generalizing a small successful pilot.
