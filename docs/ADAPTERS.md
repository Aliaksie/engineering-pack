# Host adapters and hook support

The exporters in `tooling/export/` construct host bundles; `hosts/` holds the host hook
mapping, session and feature tables that the build reads and `craft` embeds. Service bindings separately
choose Git/codehost/tracker tools. Build output is not automatically installed or trusted.

All exports include the portable runtime and a per-feature support matrix in
`SETUP.json`/`adapter-report.json`. Worker, review and compaction controls are
[native phases](HOST-CONTROL.md) that an integration you own drives with its host transport;
no export ships a transport. JetBrains and AGENTS depend on an explicit underlying agent;
guidance layouts alone provide no native execution. Missing protocol capabilities remain
unsupported, not simulated.

See [current target-platform installation and support](PLATFORMS.md) for Codex, Claude,
Gemini extensions and JetBrains, including update instructions and official sources.

## First-use setup and session visibility

After installing any adapter, ask: **"Check Engineering Pack setup, hook activity
and session usage."** Each exported package includes a self-contained `SETUP.md`
and `SETUP.json`, linked from its package instructions. The shared policy lives in
`packages/craft-engineering/standards/session-setup.md`; native controls stay in
`hosts/`. The builder copies the policy and compact adapter facts into every package
without a runtime dependency on craft-engineering or another installed package.
The first-use notice uses that page; the full `SETUP.json` feature matrix is reserved
for explicit setup and capability inspection.

On first skill use in a conversation, the agent is instructed to give one short
notice: what the actual host supports, what activation remains unverified, and
what the user can do next. It should reuse this notice across packages. Delivery
depends on loading and following the skill's linked instructions; this is not a
guaranteed installation popup, and does not depend on hooks already running.

The agent should check the actual host surface and version before offering native
commands. Missing UI controls require a documented fallback, not invented commands.
Guidance-only adapters must explain that automatic hooks are not registered.
An enabled plugin, trusted project or generated hook file does not prove automatic
hook invocation. With local telemetry enabled, observe ordinary work for fresh
`host.hook` events in runtime `telemetry.jsonl` (`settings --root <project>` shows its
directory). Explicit CLI calls and direct wrapper runs verify different paths.
Missing events alone do not diagnose the cause.

Where shell execution is available, an explicit status read works without hooks:

```sh
"<package-root>/bin/craft" statusline <host> --package <package> --root "<project-root>" --json
```

Resolve placeholders from the loaded skill's package links and the target project; the
package's `SETUP.json` names the launcher and arguments as `sessionAdvice.entry` and `args`. On Windows, run
`bin\craft.cmd`. Claude Code can run the same command without `--json` as its status
line: it reads the status-line payload from standard input.
This writes operational telemetry by default and does not activate hooks or change
host settings. Without shell access, use the supported native UI or manual guidance.
Token counters and current context occupancy are separate observations; unavailable
counts stay unknown. Adapter metadata records available usage input and controls;
do not promise a persistent display on a surface that has none.

See [Codex CLI setup controls](PLATFORMS.md#first-use-hooks-and-token-visibility)
for that specific surface. Other adapters retain their own native controls and limits.

## Installation by host

For direct GitHub installation use the [platform guide](PLATFORMS.md). The following
paths are for local development. Run `npm run build` from the source checkout with Node 22+. Each export is a separate
bundle; use the one for your host. Hooks run the bundled native runtime through generated
launchers and need no Node. Keep Node available on the host's PATH for the bundled CLI, status
line and host controls. Local Git operations also require Git. No npm install is needed.

| Host | Generated directory | How to adopt it |
|---|---|---|
| Claude Code | `dist/claude` | Add this directory as a marketplace, then install the desired plugins |
| Codex | `dist/codex/packages/<package>` | Use each directory as a native Codex plugin through your host's local plugin installation flow |
| Cursor | `dist/cursor` | Merge `.cursor/skills`, hook configuration and supporting resources into the target project |
| Copilot | `dist/copilot` | Merge `.github/skills`, instructions, hook configuration and supporting resources |
| Gemini CLI | `dist/gemini` | Merge `.gemini/skills`, settings, GEMINI.md and supporting resources |
| Gemini native extension | `dist/gemini-extension` | `gemini extensions install ./dist/gemini-extension`; guidance only |
| JetBrains AI Chat | `dist/jetbrains` | Merge project rule/resources; select Always in IDE |
| AGENTS / Agent Skills | `dist/agents` | Merge `.agents/skills` and supporting resources; merge the instruction index into your own instructions |

### Claude Code

From the checkout root:

```text
claude plugin marketplace add ./dist/claude
claude plugin install craft-engineering@craft-capabilities
```

Replace `craft-engineering` with another package name to install it. In your target project's
Claude session, use `/craft-engineering:foundations` for an engineering task or, after installing
`craft-stories`, `/craft-stories:workflow-prepare-stories` for a story-drafting workflow.
Other workflow entry points also use the `workflow-` prefix. Installation does not configure
a task tracker or enable automatic checkpoints.

### Codex

Each package has a generated `.codex-plugin/plugin.json`, namespaced skills and its
own runtime. Hook files are declared through the native manifest's `hooks` field;
optional MCP servers use `.mcp.json`. The exporter deliberately omits a root
Agent Plugins 1.0 `plugin.json`: Codex 0.163.0 skips hook loading for that format,
even when a native fallback is present ([upstream issue](https://github.com/openai/codex/issues/47925)).
Canonical portable source remains in `packages/`; the native manifest is generated,
not maintained as another version copy.

The output includes a local Codex marketplace. Install or update the selected packages
through the host's local plugin installation flow, then review and trust hooks separately.
Rebuilding the marketplace does not replace already cached plugin installations, even
when the package version is unchanged. Refresh/reinstall each installed package from the
rebuilt marketplace, reopen the session, and inspect hook discovery before reviewing trust.
Packages containing lifecycle hooks are intended for manual installation; the public
plugin directory does not accept hook-bearing plugins.

### Project overlays and plain Agent Skills

Review the selected export before merging it into your target project:

1. Include hidden skill/configuration directories. Copying only visible files omits discovery metadata.
2. Preserve the export's relative layout, including `packages/`, shared `docs/`, `schemas/` and
   license files. Skills link to package resources and hook commands reference package paths.
3. Merge settings, hooks and instruction files with existing content. In particular, preserve
   the consumer's `AGENTS.md`, `.github/copilot-instructions.md` and `GEMINI.md` policies.
4. Inspect `adapter-report.json` for supported events and limitations, then verify discovery
   and one intended workflow in a disposable project before adopting hooks in an existing project.

Overlay hook commands resolve from the target project root. Copilot includes Bash and
PowerShell launch fields. Gemini runs hooks with Bash, or PowerShell on Windows, so its
commands start with an unquoted package path. Codex plugins add a `commandWindows` field for
the PowerShell session shell Codex uses on Windows. Cursor's Windows hook shell is not
documented; other shell environments need their own live-host verification.
The plain AGENTS bundle registers no hooks or MCP connections. For guidance-only use of a
project overlay, omit its hook configuration and invoke the discovered skills explicitly.

Non-Claude skill names include the package prefix, for example
`craft-engineering-foundations` and `craft-stories-workflow-prepare-stories`.
Use your host's skill picker or invocation syntax. A source checkout alone does not place
these skills in the host's discovery directory.

Consumer routing configuration is optional. For module ownership, service bindings or
checkpoints, create `.craft/project.json` in the target project; see
[routing](ROUTING.md) and [integrations](INTEGRATIONS.md).

## Hook support

The table describes mappings implemented by this repository. Exports and wire responses
have local fixture coverage; live installation has not been certified for any host.

| Target | Export layout | Before-tool policy | Compaction / recovery |
|---|---|---|---|
| Claude Code | Generated marketplace/plugins | PreToolUse; denial via exit 2 | PreCompact snapshot; SessionStart compact restores context |
| Codex | Native plugin | PreToolUse JSON denial | PreCompact snapshot; SessionStart compact restores context |
| Cursor | `.cursor/skills` and hook overlay | preToolUse permission response | preCompact is observational; explicit recovery fallback |
| Copilot | `.github/skills`, instructions and hook overlay | preToolUse decision response | preCompact is notification; explicit recovery fallback |
| Gemini CLI | `.gemini/skills`, GEMINI.md and settings overlay | BeforeTool denial | PreCompress is asynchronous/advisory; explicit recovery fallback |
| AGENTS / Agent Skills | `.agents/skills` and instruction/resource bundle | Not installed | Explicit checkpoint CLI only |

Sources: [Claude hooks](https://code.claude.com/docs/en/hooks),
[Codex hooks](https://learn.chatgpt.com/docs/hooks), [Cursor hooks](https://cursor.com/docs/hooks),
[Copilot hooks](https://docs.github.com/en/copilot/reference/hooks-reference),
[Gemini hooks](https://geminicli.com/docs/hooks/reference/).
The mappings were checked against these references on 2026-10-16. Native platforms change;
a fixture-tested export is not a live-host compatibility certification.

## Shared repository instructions

This repository uses `AGENTS.md` directly and does not maintain a root `CLAUDE.md` wrapper.
Current Claude Code supports that instruction source when its AGENTS support and project
instruction settings permit it. Older or explicitly restricted sessions may still need a
consumer-owned import; the build does not add one or change user settings. See the
[official instruction-loading rules](https://code.claude.com/docs/en/memory#agentsmd).
Installed plugin instruction loading is separate: exported skill bodies carry their required
package context rather than relying on automatic loading of the plugin's instruction file.

## Routing and context injection

Hooks can provide hints or run deterministic state checks; they do not universally activate a
skill or select a model. Claude/Codex support context in the mapped prompt/tool/session events.
Cursor supports context after tools/session start, not our pre-tool or prompt mapping.
Copilot’s mapping injects session-start context; tool/prompt callbacks provide checkpoints
or policy decisions, not automatic skill loading. Gemini supplies prompt/session/after-tool
context. `adapter-report.json` exposes every hook’s native event, denial and context capability,
and marks unsupported events. Native skill names are added to routing hints by the adapter.

Package hook entry points use no LLM calls. Host wrappers normalize native events;
application use cases collect observations and translate lifecycle facts for portable engine
decisions. Hosts render those decisions into native responses. Milestone snapshots default
to enabled; set `checkpoint.enabled` to false to disable them.
A context-handler failure remains advisory; a valid before-shell policy call that cannot
evaluate returns a denial with a configuration error. Malformed or non-object JSON receives
the default allow decision. Inputs over 64 KiB are rejected before evaluation. Purely
observational handlers return silently with exit 0 on input/runtime failure; policy
handlers retain their existing failure behavior. Host
timeout, trust and process-failure behavior still applies and differs by platform.

Claude/Codex exports include native MCP configuration when declared. Workspace overlays and
the plain bundle include declarations only; connect them using the host’s normal flow.
No current package ships a default live MCP connection. Tool/model permissions are not granted
by exports. Codex plugin hooks require host support and trust; see
[OpenAI packaging](https://developers.openai.com/plugins/build/plugins).

## Resource layout and verification

Shared instructions are linked from generated skills, since a package instruction file must
not be assumed to load automatically. Namespaced skill names avoid collisions. Reference links
are rebased for each layout. The canonical manifest is retained as `source-capability.json`
for provenance, while native manifests and adapter reports describe the actual output.

Builds and fixtures validate layouts, hook payload translation, unsupported-feature reporting,
resource links and relocation. Live host loading, actual tool availability, account auth and
semantic model evals are separate checks. See [Cursor skills](https://cursor.com/docs/skills),
[Copilot skills](https://docs.github.com/en/copilot/concepts/agents/about-agent-skills) and
[Gemini skills](https://geminicli.com/docs/cli/using-agent-skills/) for discovery behavior.

Copilot overlays include both `bash` and `powershell` command fields, launching the
package's `bin/craft` and `bin/craft.cmd` launchers. Shared normalization recognizes
PowerShell shell tool names, so the read-only review policy does not silently skip those calls. Payload/configuration fixtures
verify this path; actual host loading remains a separate check.
