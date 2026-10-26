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

Installation, first use and updates are in [installation](INSTALL.md). This page describes
what each adapter generates and how hooks behave per host.

## Export layouts

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
See [updates](INSTALL.md#updates) for refreshing cached installations.

### Project overlays and plain Agent Skills

Overlay hook commands resolve from the target project root. Copilot includes Bash and
PowerShell launch fields. Gemini runs hooks with Bash, or PowerShell on Windows, so its
commands start with an unquoted package path. Codex plugins add a `commandWindows` field for
the PowerShell session shell Codex uses on Windows. Cursor's Windows hook shell is not
documented; other shell environments need their own live-host verification.
The plain AGENTS bundle registers no hooks or MCP connections. For guidance-only use of a
project overlay, omit its hook configuration and invoke the discovered skills explicitly.

Non-Claude skill names include the package prefix, for example
`craft-engineering-foundations` and `craft-stories-workflow-prepare-stories`. A source
checkout alone does not place these skills in the host's discovery directory.

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

Hooks make no model calls. Each hook runs `craft hook <host> <lifecycle>` through the
package launcher; `craft` normalizes the host payload into lifecycle facts, runs the engine
decision and writes the host's native response. Milestone snapshots default
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
