# Session continuation paired evaluation

This is a reproducible task fixture, not a measured improvement claim. Both arms receive
the same bug, ordinary project instructions, acceptance checks, checkpoint, five previously gathered
symbol lookups and two failed-attempt receipts. Candidate additionally receives the selected
session/context guidance. Copying a fixture changes file versions, so the locator must invalidate those packets unless
they are primed again in place. The checkpoint remains content-based. Independent known
symbols can be gathered in one bounded lookup; dependent interpretation still requires reasoning.

Use the existing [paired collector](../../../docs/EVALUATION.md#collect-one-frozen-pair).
Prepare a local fixture under ignored `.eval-results/`: copy `fixture/`, copy `bin/` and
`support/` from an exported package with its native bundle installed (for example
`dist/codex/packages/craft-delivery`), initialize a disposable Git repository and commit its inputs.
Create `.craft/task.json` with sessionId `eval-session` and task id `retry-fix`, stage
`implement`, status `working` and its actual revision. Use `locate --session eval-session
--task retry-fix --path src` to prime the first five symbols listed in `prompt.txt`. Record two
`quality-failure` acceptance attempts with the same operation/target/approach using
`session-attempt`. Save all ten continuation fields with `checkpoint-save --session eval-session
--task retry-fix`. Ignore `.craft/state/` before creating any local receipts.

Point the collector config's `fixture` to that prepared copy, `prompt` to `prompt.txt`, and
`guidance` to `docs/SESSION.md` and the canonical foundations `context-discipline.md` reference.
Supply the consumer's explicitly chosen runner, argv, identity and bounded timeout. Freeze and
run both arms in fresh isolated sessions; alternate order across repetitions. Store configs,
raw responses, model events, measurements and judgments outside Git under `.eval-results/`.
No private runner settings or provider identity belongs in this public fixture.

Judge the original four acceptance checks on each resulting workspace, changed-file scope,
checkpoint recovery and source citations. Record actual model turns/tool calls, repeated
reads, batches, reuse, retries, escalation, wall time and checkpoint quality where observable.
Keep billing tokens distinct from occupancy. Record unknown counters as unknown. The expected
initial acceptance failure validates fixture setup; preparation or static checks do not judge
model behavior. This compact continuation fixture does not simulate an entire provider context
or establish the effect of a genuinely saturated long conversation.
