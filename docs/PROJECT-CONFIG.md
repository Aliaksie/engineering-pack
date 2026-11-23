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

An organization file and a user file can declare a baseline every project inherits:
`routing`, `checkpoint`, `workspace`, `efficiency`, `standards`, `anchors`, `skills.disables`,
`safety`, `execution` and `review`. The project overrides only what differs and
`craft config` shows which layer won; see [configuration layers](SESSION.md#configuration-layers).
A layer file links `schemas/layer.schema.json`, this schema without the fields that name one repository.
`craft config --sync --apply` commits the organization file as `.craft/organization.json`, the
organization layer on machines without that file, such as CI runners.
Task overrides take the switch sections only (`routing`, `checkpoint`, `workspace`,
`efficiency`).

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
| `modules[].checks` | array of objects |  | Commands that verify this module, run in the module's path. Routes list them as argv; `craft check` runs those of the changed modules without a shell and reports each result. Craft never runs them from a hook or assumes they pass. |
| `modules[].checks[].command` | string |  | Executable, such as `./gradlew` or `npm`. |
| `modules[].checks[].args` | array of string |  | Arguments passed to the command. An argument that is exactly `{changedFiles}` becomes the changed files of the module, named from its path, deleted files left out; `craft check` skips the check when there is none. |
| `modules[].checks[].id` | string |  | Name of the check in `craft check` results. Default: the module name and the check's position, such as `api-1`. |
| `modules[].checks[].stage` | `step`, `commit`, `pr`, `ci` | `commit` | When `craft check` runs the command: at each step, before a commit, before a pull request or in CI. A run at one stage includes the earlier stages and lists later ones as left to their stage, never as passed. |
| `modules[].checks[].timeoutSeconds` | integer 1–3600 | `600` | Seconds `craft check` lets the command run before ending it and reporting it as timed out. |
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
| `standards[].run` | object |  | A consumer command that checks the standard. The agent is told to run it, and `craft check` runs it without a shell for changed files in the standard's scope. |
| `standards[].run.command` | string |  | Executable the agent runs to check the standard. |
| `standards[].run.args` | array of string |  | Arguments passed to the command. An argument that is exactly `{changedFiles}` becomes the changed files in the standard's scope, named from `cwd`, deleted files left out; `craft check` skips the command when there is none. |
| `standards[].run.cwd` | string |  | Project-relative working directory. Default: the project root. |
| `standards[].run.stage` | `step`, `commit`, `pr`, `ci` | `commit` | When `craft check` runs the command: at each step, before a commit, before a pull request or in CI. A run at one stage includes the earlier stages and lists later ones as left to their stage, never as passed. |
| `standards[].run.timeoutSeconds` | integer 1–3600 | `600` | Seconds `craft check` lets the command run before ending it and reporting it as timed out. |
| `standards[].checklist` | array of string |  | Items shown with the route and in the stop review when work reaches the scope. Defaults `level` to `review`. |
| `standards[].when` | array of `behavior`, `structure`, `prose` |  | For a `review` standard: the stop review lists it only when a changed file in its scope holds one of these facts. behavior: a line other than a comment or blank line changed in a file that is not a test. structure: an import of another declared module, a new top-level folder, or a new file in a language its module does not declare. prose: a comment or document line added, or a file in a commit the session made. Absent: listed whenever the scope matches. |
| `standards[].disabled` | boolean | `false` | With only `id`: remove the standard of this id that the organization or user layer declares. Removes nothing when no layer declares it; `craft config` says so. |

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
| `services.codehost` | object |  | Pull or merge requests: the `github` or `gitlab` setup, a `gh`/`glab` CLI binding, or exact operation mappings. See Service bindings. |
| `services.tasks` | object |  | Task tracker: the `github`, `github-project`, `gitlab` or `jira` setup, or a binding with explicit operation mappings. See Service bindings. |

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

## `safety`

object. Rules that refuse destructive shell commands, commands that reach outside the project and reads of files that commonly hold secrets, on hosts where a hook can refuse a tool call; Claude Code asks the user about a secret read instead. A rule only refuses: nothing here approves a tool call, and the host's own prompts, sandbox and deny rules keep applying. A guardrail, not a security boundary; `craft settings` lists the rules in force.

| Field | Type | Default | Description |
|---|---|---|---|
| `safety.enabled` | boolean | `true` | Whether the policy is on. Set `false` to turn it off: no command is refused. |
| `safety.disables` | array of `delete-outside-project`, `delete-project-root`, `force-push`, `delete-remote-branch`, `discard-changes`, `system-administration`, `read-secret-file` | `[]` | Built-in rules to leave out. |
| `safety.deny` | array of objects | `[]` | The project's own rules: a command whose leading words match is refused. |
| `safety.deny[].id` | string |  | Stable handle named in the refusal. |
| `safety.deny[].command` | array of string |  | Leading words of the command, such as `terraform`, `destroy`. The executable matches by name with or without a path; variable assignments and wrappers such as `env` before it are skipped. |
| `safety.deny[].reason` | string |  | What the rule refuses, shown with the refusal. |
| `safety.deny[].disabled` | boolean | `false` | With only `id`: remove the rule of this id that the organization or user layer declares. |
| `safety.secretFiles` | array of objects | `[]` | Secret-file patterns of the organization, user or project, judged with the built-in names by the `read-secret-file` rule and wherever Craft avoids quoting a secret file. Entries merge by `id` across layers; `craft config --explain` shows the layer that added each. |
| `safety.secretFiles[].id` | string |  | Stable handle a later layer uses to replace or remove the pattern. |
| `safety.secretFiles[].pattern` | string |  | A file name such as `*.vault` or `prod-*.json`, matched in any directory, or a path ending such as `.acme/token` or `config/keys/*`, matched against the last segments of a path. `*`, `?` and `{a,b}` stay within one segment; `**`, `.` and `..` segments and a pattern that matches every name are rejected. Case is ignored. A declared pattern also holds for templates and fixture directories the built-in names leave out. |
| `safety.secretFiles[].disabled` | boolean | `false` | With only `id`: remove the pattern of this id that an earlier layer declares. |

## `runtimes`

array of string. Runtimes in use, such as `node`. TypeScript does not imply Node; declare it. Default for modules that declare none.

## `routing`

object. Switches for what hooks show the agent or record. Each also takes personal preferences and task overrides; `craft settings` shows where each value came from.

| Field | Type | Default | Description |
|---|---|---|---|
| `routing.promptHints` | boolean | `true` | Routing hints on prompts. Set false to keep the request's intent as session state without showing a hint. |
| `routing.editHints` | boolean | `true` | Routing hints when the agent edits a file: the standards, project skills and references for the edited scope. |
| `routing.dedupeHints` | boolean | `true` | Session-scoped routing hint hash state; no prompts or evidence are stored. Set false to disable. |
| `routing.standardChecks` | boolean | `true` | Deterministic anchor checks after an edit or commit in a declared standard's scope. Set false to keep standards in routes without checking. |
| `routing.stopReview` | boolean | `true` | When a turn ends after changing files, one summary of new findings (module dependencies, debt markers) and the declared review items of the changed scope; the host continues the turn once to address it. Set false to never hold a stop. |
| `routing.debtMarkers` | boolean | `false` | Report debt markers a change adds against HEAD in the stop summary: TODO/FIXME comments, lint or type-check suppressions, skipped tests, removed assertions. |
| `routing.testRuns` | boolean | `true` | Record each test command the host runs with its verdict and the tree it ran on, tell the model at once when a run executed no test, and restate the runs on resume. Set false to record and show none; a declared verification pass is then not checked against an observed run. |
| `routing.workerStandards` | boolean | `true` | Give a subagent the host starts the checked and reviewed standards of the delegating session's scope. Set false to start workers without them. |
| `routing.sessionRecords` | boolean | `true` | Keep the session's established intent, its routed files and planning evidence, and the pending-resume mark under .craft/state. Set false to write none; features that read them then see no established work. |
| `routing.sessionSummary` | boolean | `true` | When a turn ends, show the user one line on what Craft did in the session (standards findings raised and fixed, test runs that ran no test, lookups answered, contexts restored after compaction), where the host displays a hook's message and only when the line changed. Set false to show none; `craft session` still reports the counts. Needs sessionRecords. |
| `routing.conventionProposals` | boolean | `true` | Hold a project that declares no standards to the ones its own files state (conventions named in agent instructions or the contributor document, lint and format configuration), and say once at session start how many are in effect and that `craft standards --propose` lists them. Said again only when the set changes. Set false to read nothing from the files; `craft standards --propose` still lists them. |
| `routing.projectChecks` | boolean | `true` | Let `craft check` run the commands declared in `modules[].checks` and `standards[].run`. Set false to have it run nothing and say so; routes still list the commands for the agent to run. |
| `routing.commitGate` | boolean | `true` | Hold a `git commit` the agent runs while a declared check of the commit stage (or the step stage) covers a changed file and has not passed on the current files in a `craft check` run of the session; the refusal names the command to run. The hook runs no check itself. Needs projectChecks, a session id and a host that lets a hook refuse a command. Set false to let commits through. |
| `routing.ticketOffer` | boolean | `true` | When a prompt starts work from a ticket (an issue or tracker URL, a key or number the prompt calls an issue or ticket or names after a start-work phrase such as `start on 12`, or a `#12`), tell the agent to state the ticket's facts to `craft ticket`, which decides direct delivery, plan first, or asking the ticket's open questions; a plan-first offer reaches the user through the host's question tool where it has one, otherwise as a visible line. Work no ticket names gets the same offer once per session, at its first prompt asking for a change or else its first edit, through `craft ticket --work <name>`. Set false to make no offer. |

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
| `efficiency.advice.nextStep` | boolean | `true` | Include a next-step suggestion, in session advice and at the end of a turn: a declared check of the commit stage that covers files the turn changed and has not run since, a required acceptance criterion of the session's task with no evidence yet, the next ready task of its Craft work plan once its task is completed, or an observed context past compactAt with where the saved continuation stands. |
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

One service binding: a named setup (`setup`), or a CLI (`transport: "cli"`) or MCP server (`transport: "mcp"`) written out in full.

### CLI binding

| Field | Type | Default | Description |
|---|---|---|---|
| `provider` | string |  | Provider name: `git`, `github`, `gitlab`, or your own such as `jira`. `github` and `gitlab` CLI bindings get built-in change operations; task operations need a setup or explicit mappings. |
| `transport` | `cli` |  | How the operation is invoked. |
| `command` | string |  | CLI executable, such as `gh`, `glab` or `team-task-cli`. |
| `host` | string |  | Code host name, such as `gitlab.example.com`. Nothing infers it from remotes. |
| `repository` | string |  | Repository path on the host, such as `team/project`. |
| `operations` | map |  | Exact mappings by operation name. They take precedence over built-in mappings. |
| `operations.<name>.args` | array of string |  | CLI arguments. `{name}` placeholders are filled from the operation parameters; the plan runs without a shell. |

### MCP binding

| Field | Type | Default | Description |
|---|---|---|---|
| `provider` | string |  | Provider name: `git`, `github`, `gitlab`, or your own such as `jira`. `github` and `gitlab` CLI bindings get built-in change operations; task operations need a setup or explicit mappings. |
| `transport` | `mcp` |  | How the operation is invoked. |
| `command` | string |  | Not used by MCP bindings; accepted for symmetry. |
| `host` | string |  | Code host name, such as `gitlab.example.com`. Nothing infers it from remotes. |
| `repository` | string |  | Repository path on the host, such as `team/project`. |
| `operations` | map |  | Exact mappings by operation name. They take precedence over built-in mappings. |
| `operations.<name>.tool` | string |  | Exact MCP tool name the host exposes. |
| `operations.<name>.arguments` | object |  | Tool arguments; string values may hold `{name}` placeholders; a whole-value placeholder keeps the parameter's JSON type. |

### Setup binding

| Field | Type | Default | Description |
|---|---|---|---|
| `setup` | `github`, `github-project`, `gitlab`, `jira` |  | Named setup. `github`: issues and pull requests through `gh` or the GitHub MCP server. `github-project`: issues with their GitHub Project's status, priority and release through `gh`. `gitlab`: issues and merge requests through `glab`. `jira`: work items through Atlassian `acli` or the Atlassian MCP server. Each supplies `task.read` and `task.search`; `github` and `gitlab` also `change.read` and `change.diff`. |
| `transport` | `cli`, `mcp` | `cli` | `cli` runs the setup's CLI; `mcp` calls its MCP server, for `github` and `jira` only. |
| `command` | string |  | CLI executable instead of the setup's own (`gh`, `glab` or `acli`). |
| `server` | string |  | MCP server name the host gave the connection, when it is not `github` or `atlassian`. |
| `host` | string |  | GitHub Enterprise or self-managed GitLab host name. Absent: github.com or gitlab.com. `github-project` takes none: it reads github.com only. |
| `repository` | string |  | Repository on the host: `owner/name` for GitHub, `group/project` for GitLab. Required for every setup except `jira`. |
| `project` | object |  | GitHub Project whose fields `github-project` reads. Required for that setup only. |
| `project.owner` | string |  | User or organization login that owns the Project. |
| `project.number` | integer 1– |  | Project number from its URL. |
| `project.fields` | object |  | Project field names read for the shared task fields. Defaults: `Status`, `Priority`, `Release`. |
| `project.fields.status` | string | `Status` | Field read as the task status. |
| `project.fields.priority` | string | `Priority` | Field read as the task priority. |
| `project.fields.release` | string | `Release` | Field read as the task release. |
| `site` | string |  | Atlassian site, such as `acme.atlassian.net`; passed as the cloud ID. Required by `jira` over MCP. |
| `operations` | map |  | Exact mappings by operation name, shaped for the binding's transport: `args` for CLI, `tool` and `arguments` for MCP. They take precedence over the setup's own. |
| `operations.<name>.args` | array of string |  | CLI arguments. `{name}` placeholders are filled from the operation parameters; the plan runs without a shell. |
| `operations.<name>.tool` | string |  | Exact MCP tool name the host exposes. |
| `operations.<name>.arguments` | object |  | Tool arguments; string values may hold `{name}` placeholders; a whole-value placeholder keeps the parameter's JSON type. |
