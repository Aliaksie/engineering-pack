# Installation

Install from this repository's generated branches. You need Git and your agent host; no npm
dependencies, source checkout or local build. The branches contain generated copies of
`packages/`; nobody maintains a separate vendor instruction tree.

## Channels

| Branch | Contents | Runtime requirement |
|---|---|---|
| `distribution` | Current stable release | Bundled commands and hooks need Node 22+ on PATH |
| `distribution-next` | Candidate built from `main` on every successful CI run | None: each package bundles a native `craft` executable |

The commands below use `distribution`. To try the candidate, substitute `distribution-next`
in a disposable project. Guidance-only use needs neither Node nor a native executable; local
Git operations need Git.

Candidate bundles include prebuilt executables for macOS 15+ on Apple Silicon, Linux x86-64
(glibc, tested on Ubuntu 24.04) and Windows x86-64 (tested on Windows Server 2022). Generated
launchers pick the executable for the current OS and CPU; checksums are verified when the
bundle is assembled. macOS Intel, Linux ARM and Alpine/musl have no executable: there, the
read-only review guard denies shell commands, other hooks continue and an explicitly
requested command fails with a clear error. Checksums detect damaged bundle contents; they
are not signatures.

Keep the complete previous bundle to roll back, and select it again through the host's
installation controls. Do not mix executables and adapters from different bundles. Replacing
a bundle does not delete the consumer's private state.

## Claude Code

```sh
claude plugin marketplace add Aliaksie/engineering-pack#distribution
claude plugin install craft-engineering@craft-capabilities
```

In the target project invoke `/craft-engineering:foundations`. Install `craft-delivery`,
`craft-review` or `craft-stories` from the same marketplace when needed; workflow entry
points use the `workflow-` prefix, for example `/craft-stories:workflow-prepare-stories`.
Installation does not configure a task tracker.

Sources: [plugins](https://code.claude.com/docs/en/plugins),
[marketplaces](https://code.claude.com/docs/en/plugin-marketplaces),
[project instructions](https://code.claude.com/docs/en/memory).

## Codex CLI

```sh
codex plugin marketplace add Aliaksie/engineering-pack --ref distribution
codex plugin add craft-engineering@engineering-pack
codex plugin list
```

Restart Codex, verify the skill appears in `/skills` and invoke
`$craft-engineering-foundations`. Install other packages from the same marketplace when
needed. Desktop users can instead use the Plugins Directory. Hook-bearing packages are for
manual installation; the public plugin directory does not accept them. For guidance without
plugins or hooks, merge the complete `agents` overlay into the project; Codex discovers
`.agents/skills`.

| Need | Codex terminal CLI | Desktop app or IDE |
|---|---|---|
| Verify installed guidance | Invoke an installed skill and confirm it loads | Use the surface's skill/plugin discovery controls |
| Review hook setup | Open `/hooks`, inspect sources, and review/trust the current definitions | Establish the surface and version; use documented controls if available |
| Inspect session usage | `/status` | Use native session information if exposed |
| Persistent token/context display | `/statusline`, then select token/context counters | Native UI availability must be verified for that surface |

Installing a plugin and trusting a project do not trust plugin hooks; changed hook definitions
need review again. If `/hooks` is missing, identify the host surface and version rather than
assuming hooks are active. The adapter has no automatic Codex token-count input, so
unavailable counts stay unknown.

Sources: [CLI commands](https://learn.chatgpt.com/docs/cli/reference),
[plugin packaging](https://developers.openai.com/plugins/build/plugins),
[skill discovery](https://learn.chatgpt.com/docs/build-skills),
[hook discovery and trust](https://learn.chatgpt.com/docs/hooks),
[CLI hook browser and status controls](https://learn.chatgpt.com/docs/developer-commands?surface=cli).

## Gemini CLI

```sh
gemini extensions install https://github.com/Aliaksie/engineering-pack --ref distribution
gemini extensions list
```

Restart Gemini and verify the extension and its skills are listed. The extension contains
guidance and tools, with no automatically registered hooks or MCP connections. For hooks, use
the `gemini` project overlay instead ([local builds](#local-builds-and-project-overlays)); do
not install both into the same project, since they expose the same skills.

Source: [extension reference](https://geminicli.com/docs/extensions/reference/).

## JetBrains AI Assistant

1. Download the [distribution ZIP](https://github.com/Aliaksie/engineering-pack/archive/refs/heads/distribution.zip),
   extract it and merge the complete `jetbrains/` folder into the target project, including
   `.aiassistant`, `.agents` and `packages`. Preserve existing files.
2. Open `.aiassistant/rules/engineering-pack.md` and choose **Always** as its rule type.
3. Start a chat and verify the rule appears in the response's attachments.

The rule is generated from the foundations skill and links to its packaged references.
Attach a reference or workflow manually if the chat cannot follow a local link. A Markdown
rule adds no shell execution, hooks or enforcement. See
[project rules](https://www.jetbrains.com/help/ai-assistant/configure-project-rules.html).
[Reusable skills](https://www.jetbrains.com/help/ai-assistant/agent-skills.html) are
documented for Claude Agent and Codex; keep linked package resources next to any skill you
move.

External agents inside JetBrains (Claude, Codex, Gemini through
[ACP](https://www.jetbrains.com/help/ai-assistant/acp.html)) use their own installation above.
Verify skill discovery in the selected agent: an IDE-managed process may use a separate
profile or installation.

## First use

Ask the agent: **"Check Craft Capabilities setup, hook activity and session usage."**

Each exported package includes `SETUP.md` and `SETUP.json`, linked from its instructions. On
first skill use in a conversation the agent gives one short notice: what the host supports,
which activation remains unverified, and what you can do next. This depends on the agent
loading the skill's linked instructions; installation alone cannot show a message.

An enabled plugin, a trusted project or a generated hook file does not prove that hooks run.
With local telemetry enabled (the default), do some ordinary work and look for fresh
`host.hook` events in `telemetry.jsonl`; `craft settings --root <project>` shows its
directory. Missing events alone do not tell discovery, trust, disabled telemetry and storage
failures apart.

Where shell execution is available, a status read works without hooks:

```sh
"<package-root>/bin/craft" statusline <host> --package <package> --root "<project-root>" --json
```

Use `bin\craft.cmd` on Windows; `SETUP.json` names the launcher and arguments as
`sessionAdvice.entry` and `args`. Claude Code can run the same command without `--json` as
its status line. The read writes local telemetry and changes no host settings. Token counters
and context occupancy are separate observations; unavailable counts stay unknown.

## Updates

| Host | Update |
|---|---|
| Claude Code | `claude plugin marketplace update craft-capabilities`, then `claude plugin update <package>@craft-capabilities` per installed package |
| Codex | `codex plugin marketplace upgrade engineering-pack` |
| Gemini | `gemini extensions update engineering-pack`, then restart |
| Overlays | Review and merge regenerated files, preserving consumer changes |

Verify the skills in a fresh session. Rolling updates between releases can keep the same
version; if a host skips an unchanged version, reinstall the affected plugin. Rebuilding a
local Codex marketplace does not replace cached installations either: refresh each package,
reopen the session and inspect hook discovery before reviewing trust. See
[releasing](RELEASING.md) for versions.

## Local builds and project overlays

From a source checkout (see [contributing](CONTRIBUTING.md) for prerequisites):

```sh
git clone https://github.com/Aliaksie/engineering-pack.git
cd engineering-pack
npm run build
```

Each export is a separate bundle; use the one for your host. Building installs, trusts and
connects nothing. `npm run build:distribution` assembles `dist/distribution` with the exact
layout published to GitHub.

| Host | Generated directory | How to adopt it |
|---|---|---|
| Claude Code | `dist/claude` | `claude plugin marketplace add ./dist/claude`, then install plugins |
| Codex | `dist/codex` | `codex plugin marketplace add ./dist/codex`, then add packages |
| Cursor | `dist/cursor` | Merge `.cursor/skills`, hook configuration and supporting resources into the project |
| Copilot | `dist/copilot` | Merge `.github/skills`, instructions, hook configuration and supporting resources |
| Gemini CLI | `dist/gemini` | Merge `.gemini/skills`, settings, GEMINI.md and supporting resources |
| Gemini extension | `dist/gemini-extension` | `gemini extensions install ./dist/gemini-extension`; guidance only |
| JetBrains AI Chat | `dist/jetbrains` | Merge the project rule and resources; select Always in the IDE |
| AGENTS / Agent Skills | `dist/agents` | Merge `.agents/skills` and resources; merge the instruction index into your own instructions |

`npm run build` writes exports without native executables. To run commands and hooks from a
local export, add the executable for the current platform to a fresh build:

```sh
npm run build:native
node tooling/native-install.mjs dist/project-evidence dist/claude --current-target
```

Replace `dist/claude` with the export you use. The install step records the source revision
and refuses executables built from uncommitted changes, so run it from a clean checkout. CI
bundles every supported target.

Before merging an overlay into a project:

1. Include hidden skill and configuration directories; copying only visible files omits
   discovery metadata.
2. Preserve the relative layout, including `packages/`, shared `docs/`, `schemas/` and license
   files. Skills link to package resources and hook commands reference package paths.
3. Merge settings, hooks and instruction files with existing content, preserving the project's
   `AGENTS.md`, `.github/copilot-instructions.md` and `GEMINI.md`.
4. Inspect `adapter-report.json` for supported events and limits, then verify discovery and
   one workflow in a disposable project before adopting hooks in a real one.

For guidance-only use of an overlay, omit its hook configuration and invoke skills explicitly.
Non-Claude skill names carry the package prefix, for example `craft-engineering-foundations`.
Optional project configuration lives in `.craft/project.json`; see [routing](ROUTING.md) and
[integrations](INTEGRATIONS.md). Host-specific hook behavior is described in
[adapters](ADAPTERS.md).
