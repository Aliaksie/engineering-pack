# Host adapters and hook support

Host adapters export shared capabilities and map lifecycle events. Service bindings separately
choose Git/codehost/tracker tools. Build output is not automatically installed or trusted.

## Installation by host

Run `npm run build` from the source checkout with Node 20+. Each export is a separate
bundle; use the one for your host. Keep Node available on the host's PATH for the bundled
CLI and hook wrappers. Local Git operations also require Git. No npm install is needed.

| Host | Generated directory | How to adopt it |
|---|---|---|
| Claude Code | `dist/claude` | Add this directory as a marketplace, then install the desired plugins |
| Codex | `dist/codex/packages/<package>` | Use each directory as a local portable plugin through your host's manual plugin installation flow |
| Cursor | `dist/cursor` | Merge `.cursor/skills`, hook configuration and supporting resources into the target project |
| Copilot | `dist/copilot` | Merge `.github/skills`, instructions, hook configuration and supporting resources |
| Gemini CLI | `dist/gemini` | Merge `.gemini/skills`, settings, GEMINI.md and supporting resources |
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

Each package has a root `plugin.json`, namespaced skills and its own runtime. Hook-bearing
packages declare their hook file through `extensions.com.openai`. The output is a portable
plugin directory, not a generated Codex marketplace. Follow the current host's manual local
installation flow in [OpenAI's packaging guide](https://developers.openai.com/plugins/build/plugins).
Review and trust hooks separately using the host's controls; host support and settings determine
whether they run. Packages containing lifecycle hooks are intended for manual installation;
the current public plugin directory does not accept hook-bearing plugins.

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
PowerShell launch fields; other shell environments need their own live-host verification.
The plain AGENTS bundle registers no hooks or MCP connections. For guidance-only use of a
project overlay, omit its hook configuration and invoke the discovered skills explicitly.

Non-Claude skill names include the package prefix, for example
`craft-engineering-foundations` and `craft-stories-workflow-prepare-stories`.
Use your host's skill picker or invocation syntax. A source checkout alone does not place
these skills in the host's discovery directory.

Consumer routing configuration is optional. For module ownership, service bindings or
checkpoints, create `.capabilities/project.json` in the target project; see
[routing](ROUTING.md) and [integrations](INTEGRATIONS.md).

## Hook support

The table describes mappings implemented by this repository. Exports and wire responses
have local fixture coverage; live installation has not been certified for any host.

| Target | Export layout | Before-tool policy | Compaction / recovery |
|---|---|---|---|
| Claude Code | Generated marketplace/plugins | PreToolUse; denial via exit 2 | PreCompact snapshot; SessionStart compact restores context |
| Codex | Local portable plugin | PreToolUse JSON denial | PreCompact snapshot; SessionStart compact restores context |
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

The package hooks use no LLM calls. Shared handlers are pure; wrappers normalize events and
invoke the local runtime. State snapshots are opt-in. A context-handler failure remains
advisory; a before-shell policy that cannot evaluate returns a denial with a configuration
error. Host timeout/trust/failure behavior still applies and differs by platform.

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

Copilot overlays include both `bash` and `powershell` command fields, each launching the
same Node wrapper. Shared normalization recognizes PowerShell shell tool names, so the
read-only review policy does not silently skip those calls. Payload/configuration fixtures
verify this path; actual host loading remains a separate check.
