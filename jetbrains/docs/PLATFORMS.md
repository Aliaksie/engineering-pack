# Platform support and installation

Install directly from this GitHub repository's generated `distribution` branch. Users
need Git and their chosen agent, without npm dependencies, a source checkout or a local build.
Guidance-only use does not require Node. Bundled CLI commands and lifecycle hooks require
Node 20+ on PATH; local Git operations also require Git.
The commands become available after the first successful publication by CI on `main`.
No npm package or public gallery listing is required.

The branch contains generated copies of the authoritative `packages/` source. CI checks
and builds it; nobody maintains a separate vendor instruction tree. Official documentation
was checked on 2026-10-18. Local fixtures validate packaging, not live model behavior.

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
Verify the new skills in a fresh session. Package versions remain at the initial `0.0.1`;
if a host skips an unchanged version, uninstall/reinstall the affected plugin through its
native UI/CLI to refresh the cached copy. Publishing new code is not a version bump. Normal
versioned releases can be introduced when a release/version change is requested.

## Local development and publication

Contributors use Node 20+ and Git, without npm dependencies:

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
