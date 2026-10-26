# Reduce uncertainty with discriminating evidence

Describe the observed result, intended result and smallest known trigger. Keep assumptions
separate from observations. Establish whether the failing path is reached and which inputs,
state and environment distinguish it from a successful run.

Develop a few plausible explanations, then select an observation that separates them. Use a
focused test, debugger, logpoint, trace or bounded experiment according to the uncertainty.
Instrument the relevant boundary instead of collecting an entire process log. Runtime values
and branch reachability can settle questions that source inspection alone cannot.

```text
Hypotheses: a message never arrived, or it arrived but was rejected as expired.
Observe the receive boundary and decision timestamp for one correlation token.
Changing the expiry threshold before observing arrival would not distinguish the causes.
```

Once evidence identifies a cause, fix the responsible contract or state transition. Check the
original trigger and an affected neighboring case. Preserve a regression test when it protects
meaningful behavior; remove temporary instrumentation that no longer serves the consumer.

Time-box experiments and avoid unrelated changes. If reproduction is unavailable, report the
source-based inference and remaining uncertainty instead of presenting a hypothesis as a confirmed
runtime cause. For a deployed incident, respect the consumer's access and operational boundaries.
