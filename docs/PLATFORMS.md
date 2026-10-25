# Platform support and installation

Install directly from this GitHub repository's generated `distribution` branch. Users
need Git and their chosen agent, without npm dependencies, a source checkout or a local build.
Guidance-only use does not require Node. Bundled CLI commands and lifecycle hooks require
Node 22+ on PATH; local Git operations also require Git.
The commands become available after the first successful publication by CI on `main`.
No npm package or public gallery listing is required.

The branch contains generated copies of the authoritative `packages/` source. CI checks
and builds it; nobody maintains a separate vendor instruction tree. Official documentation
was checked on 2026-10-18. Local fixtures validate packaging, not live model behavior.

## Native runtime in candidate bundles

The stable `distribution` channel stays frozen. Native-enabled candidates use
`distribution-next`; substitute that branch in the installation commands below after
its first successful publication. Each complete host package and independently cached
plugin includes the prebuilt runtime. Hooks start it through a generated launcher that
selects the executable for the current OS and CPU; the Node CLI and host controls also verify
its SHA-256 checksum before launching it. No platform argument, Rust toolchain or
compilation step is required. On a platform without an artifact, hooks answer as a failed
runtime would: the read-only review guard denies shell commands and other hooks continue.

The candidate build matrix covers macOS 15+ on Apple Silicon, Ubuntu 24.04+ on
x86-64, and Windows x86-64 (tested on Windows Server 2022). macOS Intel, Linux ARM and Alpine/musl have no candidate artifact.
An enabled native operation on an unsupported platform fails explicitly. Project
Intelligence remains opt-in; disabled access does not require a native executable.
Checksums detect damaged or inconsistent bundle contents; they are not signatures.

Keep the complete previous bundle to roll back. Select that bundle again through the
host's installation controls; do not mix executables and adapters from different
bundles. Replacing a bundle does not delete the consumer's private evidence state.

## Codex CLI

```sh
codex plugin marketplace add Aliaksie/engineering-pack --ref distribution
codex plugin add craft-engineering@engineering-pack
codex plugin list
```

Restart Codex and verify the skill appears in `/skills`; invoke
`$craft-engineering-foundations`. Install `craft-delivery`, `craft-review` or `craft-stories`
from the same marketplace when needed. The first command registers the marketplace;
`plugin add` installs the chosen package. Desktop users can instead use the Plugins Directory.
For guidance without plugins or hooks, merge the complete `dist/agents` overlay into a
consumer project, preserving existing instructions; Codex discovers `.agents/skills`.

Sources: [Codex CLI installation commands](https://learn.chatgpt.com/docs/cli/reference),
[plugin packaging and local marketplaces](https://developers.openai.com/plugins/build/plugins),
[skill discovery](https://learn.chatgpt.com/docs/build-skills),
[project instructions](https://developers.openai.com/codex/guides/agents-md/).

### First use: hooks and token visibility

Ask the agent: **"Check Engineering Pack setup, hook activity and session usage."**
On first skill use, all exported packages ask the agent to give one brief setup
notice per conversation; see the [shared setup policy](ADAPTERS.md#first-use-setup-and-session-visibility).
This works independently of hooks, but requires
the agent to load and follow the skill's linked package instructions; installation
alone cannot display a welcome message.

| Need | Codex terminal CLI | Desktop app or IDE |
|---|---|---|
| Verify installed guidance | Invoke an installed skill and confirm it loads | Use the surface's skill/plugin discovery controls |
| Review hook setup | Open `/hooks`, inspect sources, and review/trust the current definitions | Establish the surface and version; use documented controls if available |
| Inspect session usage | `/status` | Use native session information if exposed |
| Persistent token/context display | `/statusline`, then select token/context counters | Native UI availability must be verified for that surface |

All four packages supply lifecycle telemetry on hook-capable adapters. Engineering,
Stories and Review include observational hooks; Delivery records telemetry through its
existing routing and checkpoint hooks. Review also includes publication controls.
Installing a plugin and trusting a project do not automatically
trust plugin hooks. Changed hook definitions require review again.

If `/hooks` is missing, do not keep retrying it or assume hooks are active. Identify
the host surface and version first. A separately installed CLI does not establish
the desktop/IDE runtime's capabilities or share verified activation with it.

With default local telemetry enabled, observe a normal edit/tool action and look
for fresh `host.hook` events in runtime `telemetry.jsonl`; `settings --root <project>`
shows its directory. Direct wrapper execution and explicit CLI
events establish only those invocation paths. Missing events do not by themselves
distinguish missing discovery, trust, disabled telemetry or a storage failure.

For a status read without hooks, the agent can resolve the package root from its
loaded skill and run:

```sh
node "<package-root>/support/session.mjs" --root "<project-root>" --json
```

Replace the placeholders with the actual installed package and target project.
This command requires Node 22+, writes local operational telemetry by default,
and does not activate hooks or configure a token display. The current pack adapter
has no automatic Codex token-count input: unavailable counts stay unknown. Cumulative
session tokens and current context occupancy are separate measurements.

Sources: [hook discovery and trust](https://learn.chatgpt.com/docs/hooks),
[CLI hook browser and status controls](https://learn.chatgpt.com/docs/developer-commands?surface=cli).

## Claude Code

```sh
claude plugin marketplace add Aliaksie/engineering-pack#distribution
claude plugin install craft-engineering@craft-capabilities
```

In the target project invoke `/craft-engineering:foundations`. Install `craft-delivery`,
`craft-review` or `craft-stories` from the same marketplace when needed. The existing
marketplace identifier is retained for compatibility.

Sources: [plugins](https://code.claude.com/docs/en/plugins),
[marketplaces](https://code.claude.com/docs/en/plugin-marketplaces),
[project instructions](https://code.claude.com/docs/en/memory).

## Gemini CLI

```sh
gemini extensions install https://github.com/Aliaksie/engineering-pack --ref distribution
gemini extensions list
```

Restart Gemini; verify the extension and its skills are listed. This native extension
contains guidance and tools, with no automatically registered hooks or MCP connections.
The existing `dist/gemini` project overlay remains available for explicit hook adoption.
Do not install both layouts into the same project: they expose the same skills.

Source: [extension manifest, skills, installation and updates](https://geminicli.com/docs/extensions/reference/).
Extension sub-agents are documented as preview; this pack does not require them.
`GEMINI.md` supplies extension context; the consumer's project context remains authoritative.

## JetBrains native AI Chat

Use AI Assistant's documented project rules mechanism:

1. Download the [ready-built distribution ZIP](https://github.com/Aliaksie/engineering-pack/archive/refs/heads/distribution.zip).
   Extract it and merge the complete contents of its `jetbrains/` folder into the target
   project, including `.aiassistant`, `.agents`, and `packages`. Preserve existing files.
2. Open `.aiassistant/rules/engineering-pack.md`; choose **Always** as its Rule type
   in the IDE. The pack does not invent a metadata format for that IDE setting.
3. Start a chat and verify that the rule appears in the response's attachments.

The rule is generated from the portable foundations skill and links to its packaged
references. Attach the relevant reference or workflow manually if the native chat cannot
follow a local link. This fallback is a manual workflow, not guaranteed recursive imports.
Native chat does not gain shell execution, lifecycle hooks or deterministic enforcement
from a Markdown rule.

[Project rules](https://www.jetbrains.com/help/ai-assistant/configure-project-rules.html)
are officially supported. [Reusable skills](https://www.jetbrains.com/help/ai-assistant/agent-skills.html)
are currently documented for Claude Agent and Codex, not every native chat model.
Their settings can use local skill directories; moving an isolated skill without its
linked package resources can break references, so preserve the complete bundle.
AI Assistant also supports [prompt library](https://www.jetbrains.com/help/ai-assistant/prompt-library.html)
and [MCP clients](https://www.jetbrains.com/help/ai-assistant/mcp.html); neither is needed
to distribute this pack. A repository URL is not an installable JetBrains IDE plugin.

## External agents in JetBrains

Use the underlying agent's installation above. In AI Chat, select **Install From ACP
Registry**, or configure an actual ACP-compatible agent through **Settings | Tools |
AI Assistant | Agents**. Follow that agent's instructions for authentication and arguments;
do not assume every CLI has an `acp` command. Verify skill discovery in the selected agent
because an IDE-managed process may use a separate profile or installation.

[ACP support](https://www.jetbrains.com/help/ai-assistant/acp.html) is official; availability
is subject to IDE version, registry and organization policy. The documentation currently
excludes WSL. ACP is an agent communication protocol, not a practices marketplace.
No second JetBrains copy of Codex/Claude/Gemini instructions is needed.

## Updates and verification

Users refresh the Codex marketplace with
`codex plugin marketplace upgrade engineering-pack`. For Claude use
`claude plugin marketplace update craft-capabilities`, then
`claude plugin update craft-engineering@craft-capabilities` (repeat for installed packages).
For Gemini use `gemini extensions update engineering-pack`, then restart the session.
For project overlays, review and merge regenerated files; preserve consumer changes.
Verify the new skills in a fresh session. Pack versions are synchronized from the canonical
root version; see [versioned releases](RELEASING.md#manual-versioned-release). Rolling
updates between releases can retain a version. If a host skips an unchanged version,
uninstall/reinstall the affected plugin through its native UI/CLI to refresh its cache.

## Local development and publication

Contributors use Node 22+ and Git, without npm dependencies:

```sh
git clone https://github.com/Aliaksie/engineering-pack.git
cd engineering-pack
npm run check
```

For local testing use `claude plugin marketplace add ./dist/claude`,
`codex plugin marketplace add ./dist/codex` or
`gemini extensions install ./dist/gemini-extension`. `npm run build:distribution` assembles
`dist/distribution` with the exact layout published to GitHub. Nothing is installed or
published by that build command. See [publishing](RELEASING.md) for CI activation and checks.

`npm run check` validates canonical manifests, generated references, script syntax and
behavioral fixtures. Tests rebuild and compare new bundles for deterministic output and
check native discovery paths. Published schemas are declared where supported (Codex portable
plugin manifests); local validators cover the contracts used here, not an exhaustive online
schema certification. Human live-host smoke checks and model evals remain separate.
