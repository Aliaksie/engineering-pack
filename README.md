# Craft Capabilities

Engineering practices and workflows for AI coding agents, packaged once and installed into
Claude Code, Codex, Gemini, Cursor, Copilot, JetBrains AI Assistant or any Agent Skills host.

Agents change; your engineering standards should not. Craft Capabilities keeps those
standards in one portable source and adds a small native tool, `craft`, that makes the
mechanical decisions deterministic: which guidance applies to a change, what a review
covers, what a session has verified and what is still unknown.

## Why it helps

Ask an agent to review a change to `services/orders/Order.java`.

**Without it**, the agent works from whatever it already knows. It may assume Spring because
the file is Java, review your uncommitted working tree while describing a pull request, and
report "looks good" without saying what it checked.

**With it**, the review loads the JVM and backend guidance your project actually declares,
and nothing for frameworks it does not. The review is bound to one committed base/head
snapshot. Findings are validated against the files that changed, with a severity and
evidence for each. The verification notes say what was not run. With `review.readOnly`
enabled, a guard also blocks common shell commands that would post to the code host, so
publishing stays a separate, authorized step.

The same approach covers planning, implementation, testing, story writing and shipping:
selected guidance instead of everything at once, evidence instead of assertions, and
explicit "unknown" instead of guesses.

## Install

Claude Code:

```sh
claude plugin marketplace add Aliaksie/engineering-pack#distribution
claude plugin install craft-engineering@craft-capabilities
claude plugin install craft-delivery@craft-capabilities
```

Codex CLI:

```sh
codex plugin marketplace add Aliaksie/engineering-pack --ref distribution
codex plugin add craft-engineering@engineering-pack
```

Gemini CLI:

```sh
gemini extensions install https://github.com/Aliaksie/engineering-pack --ref distribution
```

The stable `distribution` branch needs Node 22+ on PATH for its bundled commands and hooks.
The `distribution-next` branch carries the candidate with a bundled native executable and
no Node requirement. Swap the branch name in the commands above to try it. Other hosts,
updates and rollback are covered in [installation](docs/INSTALL.md).

## Try it

In your project, start a new session and ask:

> Check Craft Capabilities setup, hook activity and session usage.

The agent reports what your host supports and what is still unverified. Then give it a real
task through a workflow, for example in Claude Code:

```text
/craft-delivery:workflow-deliver-change Fix the rounding error in the order total
```

Codex and other hosts use the package-prefixed name, such as `$craft-delivery-workflow-deliver-change`.
No configuration is required. Add `.craft/project.json` when you want module ownership,
declared checks or tracker and code-host bindings ([routing](docs/ROUTING.md),
[integrations](docs/INTEGRATIONS.md)).

## Packages

| Package | Use it for |
|---|---|
| [craft-engineering](packages/craft-engineering/README.md) | Design, six languages, backend, UI, security, testing and agent engineering guidance |
| [craft-delivery](packages/craft-delivery/README.md) | Planning, implementation, routing, service setup and shipping workflows |
| [craft-review](packages/craft-review/README.md) | Revision-bound review of local diffs, GitHub PRs and GitLab MRs, including bots |
| [craft-stories](packages/craft-stories/README.md) | Code-grounded specifications and user stories |

Install only what you need; packages work alone and use each other when present.

## Hosts and platforms

| Host | Skills | Hooks |
|---|---|---|
| Claude Code, Codex | Plugins | Yes |
| Cursor, Copilot, Gemini CLI | Project overlay (Gemini also as an extension) | Yes, in the overlay |
| JetBrains AI Assistant | Project rule | No |
| Any Agent Skills host | `.agents/skills` overlay | No |

The native executable is bundled for macOS on Apple Silicon, Linux x86-64 and Windows
x86-64. See [adapters](docs/ADAPTERS.md) for what each host supports.

## What it does not do

`craft` never calls a model, switches models, starts workers, compacts your session or posts
to a code host. Plans and recommendations are advice; your host's permissions and your
authorization still decide what happens. Nothing is sent remotely unless you configure an
OpenTelemetry endpoint; bounded local usage metadata is on by default and can be disabled
([observability and privacy](docs/OBSERVABILITY.md)).

## Direction

- Project Intelligence: a local index of declarations and imports that agents cite instead
  of searching again ([preview](docs/PROJECT-EVIDENCE.md)).
- Session continuity across compaction and handoffs, driven by host observations rather than
  estimates.
- Measured, paired evaluations of whether guidance improves outcomes. Today's checks
  establish deterministic behavior, not better model results ([evaluation](docs/EVALUATION.md)).
- Promotion of the native runtime to the stable channel.

## Learn more

- [Documentation index](docs/README.md)
- [Commands](docs/COMMANDS.md)
- [Architecture](docs/ARCHITECTURE.md)
- [Contributing](docs/CONTRIBUTING.md)
- [Changelog](CHANGELOG.md)

Licensed under [MIT](LICENSE).
