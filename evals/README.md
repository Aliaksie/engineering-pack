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
| `tasks/<name>/` | One [Harbor task](https://docs.harborframework.com/tasks/overview): instruction, fixture repository, reference solution, hidden verifier |

## Tasks

Each task is a short probe of one question, a few minutes of agent time, graded without a
model. `task.toml` names the actor, scenario and the Craft mechanism it expects.

| Task | Question | Graded by |
|---|---|---|
| `fix-discount-quantity` | Does Craft stay out of the way of a small bug fix? | Hidden behavior and conventions, untouched tests, scope; Craft overhead |
| `pi-callers-lookup` | Is a structural question (who calls a method) answered from Project Intelligence? | Exact caller set despite literal decoys, invented paths; lookups and shell searches |
| `literal-search-tool-choice` | Is a text question still answered by a literal search with Project Intelligence on? | Every mention found; lookups and shell searches |
| `plan-discount-cap` | Is a plan grounded in the code and kept as a Craft work plan? | Cites the right files and functions, says how to verify, changes no source; `craft_plan_created` |

## Run

In GitHub Actions, `Engineering eval` qualifies the tasks on every pull request that changes
them. Model-backed jobs run only on manual dispatch (`diagnose` or `compare`) with the
`CRAFT_CODEX_API_KEY` repository secret. `attempts` repeats every trial (3 before deciding
to keep, fix or remove a mechanism) and `tasks` narrows the run to a glob such as
`craft/pi-*`, for example a vanilla reference on only the probes that need one. Results are
kept as a workflow artifact for 90 days and summarized on the run page: reward, Craft
engagement and overhead, model steps, tool calls, shell searches, tokens, cost and time.

Locally, with Docker, Node, Rust and Harbor (`uv tool install harbor==0.25.0`). The image needs
the linux x86_64 runtime, so build the export on Linux x86_64 or inside a `linux/amd64`
container:

```sh
export HARBOR_TELEMETRY=off
export DOCKER_DEFAULT_PLATFORM=linux/amd64   # on Apple silicon
npm run build:codex && npm run build:native
node tooling/native-install.mjs dist/project-evidence dist/codex --current-target
docker build --file evals/image/Dockerfile --tag craft-eval:local .
harbor run --config evals/jobs/qualify.yaml
OPENAI_API_KEY=... harbor run --config evals/jobs/diagnose.yaml
harbor view .eval-results/harbor
```

## Read the results

Each trial directory holds `result.json` (tokens, cost when the host reports it, timings,
errors), `agent/trajectory.json` (every step and tool call), the collected repository with
Craft's state under `artifacts/`, and `verifier/reward.json`.
`harbor view .eval-results/harbor` browses them.

`reward` is the hidden behavioral check. The other reward values explain it:

| Value | Meaning |
|---|---|
| `correct`, `conventions`, `repo_tests` | Hidden behavior, project rules, the repository's own tests |
| `tests_untouched`, `scope_ok` | The agent left existing tests alone and changed only the expected area |
| `craft_events`, `craft_hooks` | Craft's runtime ran inside the host; 0 means it never ran |
| `craft_hook_ms_total`, `craft_hook_ms_max`, `craft_failures` | What Craft cost the session, and its failed operations |
| `craft_routing`, `craft_guidance`, `craft_lookups`, `craft_checkpoints`, `craft_task_bound`, `craft_plan_created` | Which mechanisms engaged |
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
