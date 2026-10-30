# Developer acceptance fixture

This fixed corpus tests completion reporting, reuse and project constraints.
`tests/developer-acceptance.test.mjs` exercises context composition → contract → task
commands → real source fix → separate checks → evidence → completion. It also covers
unavailable/irrelevant checks, later source/instruction edits and a direct tiny fix.
The deliberate bug remains in `fixture/`; do not repair the shared evaluation input.

Model-backed evaluation of agents on tasks like this one lives in the
[engineering eval](../../../evals/README.md).
