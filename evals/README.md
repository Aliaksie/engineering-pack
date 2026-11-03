# Engineering eval

Does Craft improve how a coding agent does engineering work, and does the agent use the right
Craft mechanism for the problem? Tasks here run a real agent in a container against a frozen
repository, then grade the result with hidden checks and with what Craft recorded.

Installation and behavior across hosts and operating systems are proven by the deterministic
tests in normal CI; runtime cost is measured by the performance workflow. This suite answers
only the engineering question, in one reference setup: Codex CLI on Linux, run by
[Harbor](https://docs.harborframework.com/), a pinned external tool. Nothing in the Craft
runtime depends on it; trials install the exported Codex packages like a user would.

## Layout

| Path | Contents |
|---|---|
| `image/Dockerfile` | Trial image: pinned Codex CLI with the exported Craft packages installed |
| `image/craft-rewards.sh` | Shared by every verifier: what Craft recorded in the collected repository |
| `codex/craft.toml`, `codex/vanilla.toml` | Codex configuration for each arm: Craft enabled, or no plugins |
| `jobs/qualify.yaml` | No model: the reference solution must pass every task and an idle agent must fail it |
| `jobs/diagnose.yaml` | Craft only: which mechanisms engaged on each task |
| `jobs/compare.yaml` | Matched vanilla and Craft arms |
| `probes/unfamiliar-repos.json` | Unfamiliar-repository probes: pinned public repositories, questions and ground truth |
| `image/answer-grade.sh` | Shared grader for cited answers against a probe's ground truth |
| `tasks/<name>/` | One [Harbor task](https://docs.harborframework.com/tasks/overview): instruction, fixture repository, reference solution, hidden verifier |

## Tasks

Each task is a short probe of one question, a few minutes of agent time, graded without a
model. `task.toml` names the actor, scenario and the Craft mechanism it expects.

| Task | Question | Graded by |
|---|---|---|
| `fix-discount-quantity` | Does Craft stay out of the way of a small bug fix? | Hidden behavior and conventions, untouched tests, scope; Craft overhead |
| `pi-callers-lookup` | Is a structural question (who calls a method) answered from Project Intelligence? | Exact caller set despite literal decoys, invented paths; lookups and shell searches |
| `literal-search-tool-choice` | Is a text question still answered by a literal search with Project Intelligence on? | Every mention found; lookups and shell searches |
| `plan-discount-cap` | Is a plan grounded in the code and kept as the session's planning evidence? | Cites the right files and functions, says how to verify, changes no source; `craft_planning_evidence` |
| `idle-typo-fix` | Does Craft add cost to a one-word fix? | The corrected text, nothing else changed; Craft overhead |
| `idle-one-file-question` | Does Craft add cost to a question about one named file? | The exact number; exploration until the answer |
| `idle-general-question` | Does Craft stay silent when the repository is irrelevant? | The exact number; exploration until the answer |

The `idle-*` tasks (keyword `probe-no-advantage`) have nothing for Craft to improve. Run them
with the `compare` job and `tasks: idle-*`; the Craft arm should cost what vanilla costs.

## Unfamiliar-repository probes

Does Project Intelligence help on a repository the agent has never seen? Each probe asks one
question with a deterministic, cited answer on a public repository fetched at a frozen commit
when the task image is built: one repository per supported language (TypeScript, Python, Java,
Go, Rust) and one documentation-heavy repository with architecture decision records. Questions
come from developers (where is it declared, who calls it), reviewers (what does this change
touch), architects (which file owns it, which decision record is current) and QA (where is it
tested).

`evals/probes/unfamiliar-repos.json` is the only source. `npm run eval:probes` writes one
Git-ignored task per question as `tasks/repo-<id>/`; the workflow does this before every run.
The answer is a list of paths, or `<path>#<function>` for callers, graded by
`image/answer-grade.sh`: every expected citation (or, for `"match": "any"`, at least one), no
citation outside the expected and allowed ones, and no invented path. Run them with
`tasks: repo-*`, or one repository with `repo-<name>-*`.

Before changing ground truth, clone each repository at its pinned commit into one directory and
run `node tooling/eval-probes.mjs --verify <directory>`: every cited path must exist and every
cited function must appear in its file. Questions use the repository's own words where a user
would, and never mention Craft.

## Run

In GitHub Actions, `Engineering eval` qualifies the tasks on every pull request that changes
them. Model-backed jobs run only on manual dispatch (`diagnose` or `compare`) with the
`CRAFT_CODEX_API_KEY` repository secret. `attempts` repeats every trial (3 before deciding
to keep, fix or remove a mechanism) and `tasks` narrows the run to task directories matching a
glob such as `pi-*`, for example a vanilla reference on only the probes that need one. Results are
kept as a workflow artifact for 90 days and summarized on the run page (see below). `compare`
runs three attempts per task and arm unless `attempts` says otherwise. `craft_ref` builds the Craft arm
from a tag or commit such as `v0.0.3` while the tasks still come from the checkout, so a
released version can be measured on probes written after it.

Locally, with Docker, Node, Rust and Harbor (`uv tool install harbor==0.25.0`). The image needs
the linux x86_64 runtime, so build the export on Linux x86_64 or inside a `linux/amd64`
container:

```sh
export HARBOR_TELEMETRY=off
export DOCKER_DEFAULT_PLATFORM=linux/amd64   # on Apple silicon
npm run build:codex && npm run build:native
node tooling/native-install.mjs dist/project-evidence dist/codex --current-target
docker build --file evals/image/Dockerfile --tag craft-eval:local .
npm run eval:probes
harbor run --config evals/jobs/qualify.yaml
OPENAI_API_KEY=... harbor run --config evals/jobs/diagnose.yaml
harbor view .eval-results/harbor
```

## Read the results

Each trial directory holds `result.json` (tokens, cost when the host reports it, timings,
errors), `agent/trajectory.json` (every step and tool call), the collected repository with
Craft's state under `artifacts/`, and `verifier/reward.json`.
`harbor view .eval-results/harbor` browses them.

`npm run eval:summary -- .eval-results/harbor/<job>` writes the table the workflow shows on the
run page (`--json` for the values). Per task and arm it gives N, correct trials and the median
exploration: tool calls, shell commands, searches, file reads (of them, reads of installed
package guidance), input tokens and seconds, plus the bytes injected outside the conversation
and Craft's hook time. A task whose `task.toml` names `answer_file` in its metadata is measured
until the agent first writes that file; others over the whole trial. The Craft arm is shown
relative to the vanilla arm, and a comparison with fewer than three attempts per arm is marked
as not deciding. Counts come from the trajectory and are best-effort; a value it does not carry
stays unknown.

`reward` is the hidden behavioral check. The other reward values explain it:

| Value | Meaning |
|---|---|
| `correct`, `conventions`, `repo_tests` | Hidden behavior, project rules, the repository's own tests |
| `tests_untouched`, `scope_ok` | The agent left existing tests alone and changed only the expected area |
| `craft_events`, `craft_hooks` | Craft's runtime ran inside the host; 0 means it never ran |
| `craft_hook_ms_total`, `craft_hook_ms_max`, `craft_failures` | What Craft cost the session, and its failed operations |
| `craft_routing`, `craft_guidance`, `craft_lookups`, `craft_checkpoints`, `craft_task_bound`, `craft_plan_created`, `craft_planning_evidence` | Which mechanisms engaged |
| `expected_skill_selected` | Craft selected the skill this task calls for (`tests/expected.json`) |

Task-specific values (`callers_found`, `invented_paths`, `mentions_found`, `cites_code`,
`noticed_defect`, ...) are defined in that task's `tests/test.sh`.

Unknown values stay unknown; one trial is a diagnosis, not evidence of a general benefit.

## Add a task

Copy a task directory, replace the fixture under `environment/repo`, and write the issue as
the instruction. Keep the instruction neutral: it describes the work, never Craft. Grade the
task's own outcome in `tests/test.sh` and merge `/opt/craft-eval/craft-rewards.sh` for
Craft's engagement. Keep the answer out of anything the agent can read: the oracle lives in
`tests/` and `solution/`. Before relying on the task, `jobs/qualify.yaml` must show the
reference solution passing and the idle agent failing, and `harbor lint --select ALL --ignore
unpinned-image evals/tasks/*/` must be clean.

## Data

Set `HARBOR_TELEMETRY=off`: Harbor otherwise sends usage statistics, including token counts,
cost and reward. Tasks run from this directory; do not upload results to Harbor Hub. The only
outbound traffic is installing pinned tools and the agent's own model calls.
