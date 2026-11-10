# Project configuration

<!-- Generated from schemas/project.schema.json by `npm run docs:config`. Edit the schema, not this file. -->

Craft project configuration in `.craft/project.json`. Every field except `formatVersion` is optional; an absent field takes the default shown.

[Routing](ROUTING.md#project-standards) shows standards, anchors and skills by example;
`examples/project/.craft/project.json` adds modules, services and execution.
`craft settings --root <project>` prints the effective values with the origin of each.

For completion and hover help in an editor, link the schema from the file:

```json
{ "$schema": "https://raw.githubusercontent.com/Aliaksie/engineering-pack/main/schemas/project.schema.json", "formatVersion": 1 }
```

Fields that only switch something on or off can also be set in personal preferences or
task overrides (`routing`, `checkpoint`, `workspace`, `efficiency`); see
[settings](SESSION.md#settings).

## `formatVersion`

`1`. Configuration format. Always 1.

## `languages`

array of `javascript`, `typescript`, `python`, `jvm`, `csharp`, `go`, `java`, `kotlin`, `rust`, `ruby`. Languages the code is written in. Selects language guidance; nothing else is inferred from it. Default for modules that declare none.

## `layers`

array of `ui`, `backend`, `agent`. Application layers: `ui`, `backend` or `agent`. Selects layer guidance. Default for modules that declare none.

## `frameworks`

array of string. Frameworks in use, such as `react`. Selects framework guidance when Craft has it. Default for modules that declare none.

## `providers`

array of string. Model or cloud providers in use, such as `foundry`. Selects provider guidance when Craft has it. Default for modules that declare none.

## `modules`

array of objects. Parts of the repository with an owner, stack and checks. Routes name the modules a change touches, their dependents and their checks. Without modules the project is one unit.

| Field | Type | Default | Description |
|---|---|---|---|
| `modules[].name` | string |  | Unique lowercase name (letters, digits, hyphens) used in `dependsOn`, scopes and routes. |
| `modules[].path` | string |  | Project-relative folder the module owns. The most specific path owns a file. |
| `modules[].owner` | string |  | Team or person shown as the owner in routes. |
| `modules[].languages` | array of `javascript`, `typescript`, `python`, `jvm`, `csharp`, `go`, `java`, `kotlin`, `rust`, `ruby` |  | Languages the code is written in. Selects language guidance; nothing else is inferred from it. Overrides the project value; an empty array clears it. |
| `modules[].layers` | array of `ui`, `backend`, `agent` |  | Application layers: `ui`, `backend` or `agent`. Selects layer guidance. Overrides the project value; an empty array clears it. |
| `modules[].skills` | array of string |  | Logical skills (`package:skill`) added to routes for `implement`, `plan`, `investigate` and `review` work in this module. |
| `modules[].dependsOn` | array of string |  | Modules this module may import. With `hexagonal-architecture`, `clean-architecture` or a `module-dependencies` check, an added import into any other declared module is reported. Absent: imports are not checked. |
| `modules[].providers` | array of string |  | Model or cloud providers in use, such as `foundry`. Selects provider guidance when Craft has it. Overrides the project value; an empty array clears it. |
| `modules[].frameworks` | array of string |  | Frameworks in use, such as `react`. Selects framework guidance when Craft has it. Overrides the project value; an empty array clears it. |
| `modules[].checks` | array of objects |  | Commands that verify this module. Routes list them as argv to run; Craft never runs them or assumes they pass. |
| `modules[].checks[].command` | string |  | Executable, such as `./gradlew` or `npm`. |
| `modules[].checks[].args` | array of string |  | Arguments passed to the command. |
| `modules[].runtimes` | array of string |  | Runtimes in use, such as `node`. TypeScript does not imply Node; declare it. Overrides the project value; an empty array clears it. |

## `standards`

array of objects. Project standards: consumer-owned declarations routed by scope and, for anchors, checked without a model.

| Field | Type | Default | Description |
|---|---|---|---|
| `standards[].id` | string |  | Stable handle shown in routes and check results. |
| `standards[].file` | string |  | Project-relative file holding the standard. |
| `standards[].folder` | string |  | Project-relative folder of standard documents. |
| `standards[].anchor` | string |  | An established standard by name: a Semantic Anchors catalog id, a Craft alias such as semver, or an id from the project's anchor catalogs. Names with a Craft check are checked deterministically; others are advised or reviewed. |
| `standards[].summary` | string |  | One line shown in routes instead of the standard's content. |
| `standards[].level` | `advise`, `check`, `review` |  | advise: named when work reaches the scope; check: an anchor check or a run command; review: a checklist. Defaults from the declaration's form. |
| `standards[].scope` | object |  | Where the standard applies; all keys must match, any value within a key. Absent: the whole project. |
| `standards[].scope.modules` | array of string |  | Module names from `modules`. |
| `standards[].scope.languages` | array of `javascript`, `typescript`, `python`, `jvm`, `csharp`, `go`, `java`, `kotlin`, `rust`, `ruby` |  | Languages of the affected files. |
| `standards[].scope.layers` | array of `ui`, `backend`, `agent` |  | Layers of the affected modules. |
| `standards[].scope.paths` | array of string |  | Project-relative path prefixes of the affected files. |
| `standards[].scope.intents` | array of string |  | Kinds of work: story, plan, verify, explore, review, implement and the other routing intents. |
| `standards[].run` | object |  | A consumer command the agent is told to run to check the standard. |
| `standards[].run.command` | string |  | Executable the agent runs to check the standard. |
| `standards[].run.args` | array of string |  | Arguments passed to the command. |
| `standards[].run.cwd` | string |  | Project-relative working directory. Default: the project root. |
| `standards[].checklist` | array of string |  | Items shown with the route and in the stop review when work reaches the scope. Defaults `level` to `review`. |

## `anchors`

object. Where the project's own anchors come from and which Craft defaults apply. A project entry replaces the built-in entry with the same id.

| Field | Type | Default | Description |
|---|---|---|---|
| `anchors.catalogs` | array of string | `[]` | Project-relative anchor catalog files, read in order; a later file wins on the same id. Each holds {"anchors": [{id, title?, summary?, checklist?, check?, intents?, targets?}]}. |
| `anchors.defaults` | boolean | `false` | Whether Craft adds its anchors for the kind of work to routes, advised and never checked; off unless true. A declared standard for the kind of work replaces them. |
| `anchors.disables` | array of string | `[]` | Anchors Craft must not add as defaults. |

## `skills`

object. The project's own skills in routes. Hosts list project skills themselves; a declaration tells Craft which one a route recommends. Routes name them project:<name>; hosts are shown the bare name.

| Field | Type | Default | Description |
|---|---|---|---|
| `skills.declared` | array of objects |  | Project skills to name in routes. |
| `skills.declared[].name` | string |  | The name in the skill's SKILL.md front matter. |
| `skills.declared[].path` | string |  | The skill folder at the project root. A host that does not read this folder keeps the package skill and the route reports why. |
| `skills.declared[].replaces` | array of string |  | Package skills this skill takes the place of wherever a route selects them; one declaration per package skill. |
| `skills.declared[].scope` | object |  | Where the skill applies, as for standards: with replaces it narrows the replacement, alone it adds the skill to matching routes. |
| `skills.declared[].scope.modules` | array of string |  | Module names from `modules`. |
| `skills.declared[].scope.languages` | array of `javascript`, `typescript`, `python`, `jvm`, `csharp`, `go`, `java`, `kotlin`, `rust`, `ruby` |  | Languages of the affected files. |
| `skills.declared[].scope.layers` | array of `ui`, `backend`, `agent` |  | Layers of the affected modules. |
| `skills.declared[].scope.paths` | array of string |  | Project-relative path prefixes of the affected files. |
| `skills.declared[].scope.intents` | array of string |  | Kinds of work: story, plan, verify, explore, review, implement and the other routing intents. |
| `skills.disables` | array of string | `[]` | Package skills routes no longer recommend. Hosts may still list them; Craft never edits host configuration. |

## `services`

object. Git, code host and tracker bindings that `craft operation` turns into CLI or MCP invocation plans. Nothing here runs a command or opens a connection; authentication stays with the host.

| Field | Type | Default | Description |
|---|---|---|---|
| `services.git` | object |  | Local Git. Default: the `git` CLI. See Service bindings. |
| `services.codehost` | object |  | Pull or merge requests: `gh` for GitHub, `glab` for GitLab, or exact operation mappings. See Service bindings. |
| `services.tasks` | object |  | Task tracker. Always needs explicit operation mappings. See Service bindings. |

## `permissions`

object. Which mutating service operations may be planned at all. Each still needs separate authorization when it runs.

| Field | Type | Default | Description |
|---|---|---|---|
| `permissions.writeOperations` | array of `git.commit`, `git.push`, `change.publish`, `task.transition`, `task.comment` | `[]` | Write operations `craft operation` may plan. Default: none, so every mutation is refused. |

## `execution`

object. Worker roles, model profiles and host support. Logical names only; nothing vendor-specific is built in, and nothing here starts a worker or switches a model.

| Field | Type | Default | Description |
|---|---|---|---|
| `execution.roles` | object |  | How each kind of work runs. Defaults: `locator` uses a tool with the `economical` profile, `reviewer` runs in the main session with `strong`, the others in the main session with `standard`. |
| `execution.roles.locator` | object |  | How `locator` work runs. |
| `execution.roles.locator.mode` | `main`, `delegate`, `tool` |  | `main`: in the current session. `delegate`: a worker, when the host allows it. `tool`: a deterministic tool first. |
| `execution.roles.locator.profile` | string |  | Profile name from `profiles`. |
| `execution.roles.researcher` | object |  | How `researcher` work runs. |
| `execution.roles.researcher.mode` | `main`, `delegate`, `tool` |  | `main`: in the current session. `delegate`: a worker, when the host allows it. `tool`: a deterministic tool first. |
| `execution.roles.researcher.profile` | string |  | Profile name from `profiles`. |
| `execution.roles.implementer` | object |  | How `implementer` work runs. |
| `execution.roles.implementer.mode` | `main`, `delegate`, `tool` |  | `main`: in the current session. `delegate`: a worker, when the host allows it. `tool`: a deterministic tool first. |
| `execution.roles.implementer.profile` | string |  | Profile name from `profiles`. |
| `execution.roles.reviewer` | object |  | How `reviewer` work runs. |
| `execution.roles.reviewer.mode` | `main`, `delegate`, `tool` |  | `main`: in the current session. `delegate`: a worker, when the host allows it. `tool`: a deterministic tool first. |
| `execution.roles.reviewer.profile` | string |  | Profile name from `profiles`. |
| `execution.roles.planner` | object |  | How `planner` work runs. |
| `execution.roles.planner.mode` | `main`, `delegate`, `tool` |  | `main`: in the current session. `delegate`: a worker, when the host allows it. `tool`: a deterministic tool first. |
| `execution.roles.planner.profile` | string |  | Profile name from `profiles`. |
| `execution.roles.verifier` | object |  | How `verifier` work runs. |
| `execution.roles.verifier.mode` | `main`, `delegate`, `tool` |  | `main`: in the current session. `delegate`: a worker, when the host allows it. `tool`: a deterministic tool first. |
| `execution.roles.verifier.profile` | string |  | Profile name from `profiles`. |
| `execution.hosts` | map |  | What each host supports, keyed by host name (`claude`, `codex`, ...). |
| `execution.hosts.<name>.delegation` | boolean |  | The host can start workers. Delegated roles are planned only when both the role and the host allow it. |
| `execution.hosts.<name>.models` | map |  | Exact host model name for each profile name. |
| `execution.hosts.<name>.mainModelSwitch` | boolean |  | Explicit host support declaration; never grants permission to switch models. |
| `execution.hosts.<name>.contextLimits` | map |  | Context limits by exact host model name, over the bundled host table. windowTokens applies only when the host reports no window; budgetTokens is an optional working budget past which advice suggests resuming from the checkpoint in a new session. |
| `execution.hosts.<name>.contextLimits.<name>.windowTokens` | integer 1– |  | Context window in tokens. |
| `execution.hosts.<name>.contextLimits.<name>.budgetTokens` | integer 1– |  | Working budget in tokens. |
| `execution.profiles` | map |  | Named model profiles that roles refer to. Defaults: `economical` escalates to `standard`, `standard` to `strong`. |
| `execution.profiles.<name>.capabilities` | array of string |  | Capability tags the profile offers, used when advice asks for one. |
| `execution.profiles.<name>.maxAttempts` | integer 1–5 | `3` | Failed attempts on the same task and revision before advice escalates. Default: 3. |
| `execution.profiles.<name>.escalateTo` | string |  | Profile to recommend after `maxAttempts`. Absent: advice stops instead of inventing a stronger model. |
| `execution.profiles.<name>.class` | `economical`, `standard`, `strong` |  | Logical class: `economical` (lookups, formatting, deterministic checks), `standard` (routine work) or `strong` (hard review and design). |

## `checkpoint`

object. Automatic task snapshots.

| Field | Type | Default | Description |
|---|---|---|---|
| `checkpoint.enabled` | boolean | `true` | Save a snapshot at turn end and context transitions, and show the session identity and resume lines. |

## `review`

object. Review session safety.

| Field | Type | Default | Description |
|---|---|---|---|
| `review.readOnly` | boolean | `false` | Deny common `gh`/`glab` publishing commands and `git push` from the shell in this project. A heuristic, not a replacement for read-only credentials. |

## `runtimes`

array of string. Runtimes in use, such as `node`. TypeScript does not imply Node; declare it. Default for modules that declare none.

## `routing`

object. Switches for what hooks show the agent or record. Each also takes personal preferences and task overrides; `craft settings` shows where each value came from.

| Field | Type | Default | Description |
|---|---|---|---|
| `routing.promptHints` | boolean | `true` | Routing hints on prompts. Set false to keep the request's intent as session state without showing a hint. |
| `routing.editHints` | boolean | `true` | Routing hints when the agent edits a file: the skills and references for the edited scope. |
| `routing.dedupeHints` | boolean | `true` | Session-scoped routing hint hash state; no prompts or evidence are stored. Set false to disable. |
| `routing.standardChecks` | boolean | `true` | Deterministic anchor checks after an edit or commit in a declared standard's scope. Set false to keep standards in routes without checking. |
| `routing.stopReview` | boolean | `true` | When a turn ends after changing files, one summary of new findings (module dependencies, debt markers) and the declared review items of the changed scope; the host continues the turn once to address it. Set false to never hold a stop. |
| `routing.debtMarkers` | boolean | `false` | Report debt markers a change adds against HEAD in the stop summary: TODO/FIXME comments, lint or type-check suppressions, skipped tests, removed assertions. |
| `routing.testRuns` | boolean | `true` | Record each test command the host runs with its verdict and the tree it ran on, tell the model at once when a run executed no test, and restate the runs on resume. Set false to record and show none; a declared verification pass is then not checked against an observed run. |
| `routing.workerStandards` | boolean | `true` | Give a subagent the host starts the checked and reviewed standards of the delegating session's scope. Set false to start workers without them. |
| `routing.sessionRecords` | boolean | `true` | Keep the session's established intent, its routed files and planning evidence, and the pending-resume mark under .craft/state. Set false to write none; features that read them then see no established work. |
| `routing.sessionSummary` | boolean | `true` | When a turn ends, show the user one line on what Craft did in the session (standards findings raised and fixed, test runs that ran no test, lookups answered, task contexts restored), where the host displays a hook's message and only when the line changed. Set false to show none; `craft session` still reports the counts. Needs sessionRecords. |

## `efficiency`

object. Session advice: lookups, attempts, review, context and delegation. Recommendations only; Craft never acts on them itself.

| Field | Type | Default | Description |
|---|---|---|---|
| `efficiency.lookup` | object |  | Hook answers for lookup questions. |
| `efficiency.lookup.enabled` | boolean | `true` | Answer certain lookups (callers, declarations, configuration keys, mentions, decision records) in the hook, without a model turn. |
| `efficiency.lookup.reuse` | boolean | `true` | Reuse earlier `craft locate` results in a task while revision, files and configuration are unchanged. |
| `efficiency.batching` | object |  | Accepted for policy files. Batching independent tool calls is a workflow instruction; `craft` does not act on these values. |
| `efficiency.batching.enabled` | boolean | `true` | Recorded only. |
| `efficiency.batching.maxConcurrency` | integer 1–64 | `3` | Recorded only. |
| `efficiency.attempts` | object |  | Acceptance attempt budget. |
| `efficiency.attempts.enabled` | boolean | `true` | Recorded only; the budget below always applies to recorded attempts. |
| `efficiency.attempts.maxAttempts` | integer 1–5 | `3` | Same failure on unchanged source this many times: return control or escalate. |
| `efficiency.advice` | object |  | Next-step advice in session status. |
| `efficiency.advice.enabled` | boolean | `true` | Give advice. False removes the next-step suggestion. |
| `efficiency.advice.at` | `meaningful-boundaries` | `meaningful-boundaries` | When advice is given. Only `meaningful-boundaries` exists. |
| `efficiency.advice.nextStep` | boolean | `true` | Include a next-step suggestion. |
| `efficiency.review` | object |  | Review offers. |
| `efficiency.review.mode` | `manual`, `recommend`, `execute-when-authorized` | `recommend` | `manual`: no review offers. `recommend`: offer a review where it helps. `execute-when-authorized`: may proceed once the user approves the offer. |
| `efficiency.context` | object |  | Context pressure advice. Bands must rise: `elevatedAt` < `compactAt` < `restartAt`, each a fraction of the window. |
| `efficiency.context.elevatedAt` | number 0–1 | `0.6` | Occupancy at which pressure is elevated. |
| `efficiency.context.compactAt` | number 0–1 | `0.75` | Occupancy at which advice is "checkpoint, then compact". |
| `efficiency.context.restartAt` | number 0–1 | `0.85` | Occupancy at which advice is "checkpoint, then new session". |
| `efficiency.context.action` | `manual`, `recommend`, `execute-when-authorized` | `recommend` | `manual`: no compaction or restart advice. `recommend`: advise at a safe boundary. `execute-when-authorized`: may proceed once the user approves. |
| `efficiency.delegation` | object |  | Worker plans. |
| `efficiency.delegation.mode` | `manual`, `recommend`, `execute-when-authorized` | `recommend` | `manual`: no worker plans. `recommend`: plan workers where a role and host allow it. `execute-when-authorized`: may proceed once the user approves. |
| `efficiency.delegation.maxWorkers` | integer 0– | `3` | Most running workers per session. 0 disables worker plans. |

## `workspace`

object. Private scratch directories and promoted evidence under the state directory. `craft settings` shows the limits.

| Field | Type | Default | Description |
|---|---|---|---|
| `workspace.enabled` | boolean | `true` | Allow task scratch workspaces. |
| `workspace.maxSessions` | integer 1–64 | `20` | Most workspaces kept; at capacity the oldest inactive one is evicted first. |
| `workspace.scratch` | object |  | Scratch directory limits. |
| `workspace.scratch.staleAfterHours` | integer 1–8760 | `24` | Hours without a write before `workspace-clean` treats a workspace as stale. |
| `workspace.scratch.maxBytes` | integer 1–67108864 | `8388608` | Most bytes in one scratch directory. |
| `workspace.scratch.maxFiles` | integer 1–1024 | `64` | Most files in one scratch directory. |
| `workspace.evidence` | object |  | Files promoted from scratch as evidence. |
| `workspace.evidence.enabled` | boolean | `true` | Allow promoting scratch files as evidence. |
| `workspace.evidence.retentionDays` | integer 1–365 | `7` | Days promoted evidence is kept. |
| `workspace.evidence.maxArtifacts` | integer 1–64 | `16` | Most promoted files per workspace. |
| `workspace.evidence.maxBytes` | integer 1–67108864 | `1048576` | Most bytes of promoted evidence per workspace. |

## `evidence`

object. Project Intelligence enablement and inventory scope.

| Field | Type | Default | Description |
|---|---|---|---|
| `evidence.enabled` | boolean | `true` | Route structural lookups (declarations, callers, imports, document sections, links and mentions, decision records, configuration keys) through Project Intelligence before literal search. On unless set false. |
| `evidence.exclude` | array of string | `[]` | Project-relative path prefixes left out of the evidence inventory, such as generated samples or vendored bundles. Whole path components; a trailing / is accepted. |

## Service bindings

One service binding: a CLI (`transport: "cli"`) or an MCP server (`transport: "mcp"`).

### CLI binding

| Field | Type | Default | Description |
|---|---|---|---|
| `provider` | string |  | Provider name: `git`, `github`, `gitlab`, or your own such as `jira`. `github` and `gitlab` CLI bindings get built-in change operations. |
| `transport` | `cli` |  | How the operation is invoked. |
| `command` | string |  | CLI executable, such as `gh`, `glab` or `team-task-cli`. |
| `host` | string |  | Code host name, such as `gitlab.example.com`. Nothing infers it from remotes. |
| `repository` | string |  | Repository path on the host, such as `team/project`. |
| `operations` | map |  | Exact mappings by operation name. They take precedence over built-in mappings. |
| `operations.<name>.args` | array of string |  | CLI arguments. `{name}` placeholders are filled from the operation parameters; the plan runs without a shell. |

### MCP binding

| Field | Type | Default | Description |
|---|---|---|---|
| `provider` | string |  | Provider name: `git`, `github`, `gitlab`, or your own such as `jira`. `github` and `gitlab` CLI bindings get built-in change operations. |
| `transport` | `mcp` |  | How the operation is invoked. |
| `command` | string |  | Not used by MCP bindings; accepted for symmetry. |
| `host` | string |  | Code host name, such as `gitlab.example.com`. Nothing infers it from remotes. |
| `repository` | string |  | Repository path on the host, such as `team/project`. |
| `operations` | map |  | Exact mappings by operation name. They take precedence over built-in mappings. |
| `operations.<name>.tool` | string |  | Exact MCP tool name the host exposes. |
| `operations.<name>.arguments` | object |  | Tool arguments; string values may hold `{name}` placeholders; a whole-value placeholder keeps the parameter's JSON type. |
