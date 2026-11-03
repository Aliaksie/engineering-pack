# Host adapters and hook support

`tooling/export/` builds one bundle per host from `packages/`; `hosts/` holds the hook,
session and feature tables the build reads and `craft` embeds. Every bundle carries the native
runtime and reports per-feature support in `SETUP.json` and `adapter-report.json`; missing host
capabilities are reported as unsupported, not simulated. Installation is in
[INSTALL](INSTALL.md); worker, review and compaction controls are [native phases](HOST-CONTROL.md)
driven by an integration you own.

Builds and fixtures cover layouts, hook payload translation, resource links and relocation.
Live host loading, tool availability, account auth and model behavior are not certified for
any host.

## Export layouts

| Target | Layout | Hooks | MCP |
|---|---|---|---|
| Claude Code | Marketplace with one plugin per package | Plugin hooks | Native config when declared |
| Codex | `.codex-plugin/plugin.json` per package, local marketplace | Manifest `hooks` field, `commandWindows` for PowerShell | `.mcp.json` when declared |
| Cursor | `.cursor/skills` and hook overlay | Project overlay | Declarations only |
| Copilot | `.github/skills`, instructions and hook overlay | `bash` and `powershell` fields | Declarations only |
| Gemini CLI | `.gemini/skills`, `GEMINI.md` and settings overlay | Bash, or PowerShell on Windows | Declarations only |
| AGENTS / Agent Skills | `.agents/skills` and instruction bundle | None | None |

- Codex gets no root Agent Plugins 1.0 `plugin.json`: Codex 0.163.0 skips hook loading for that
  format ([upstream issue](https://github.com/openai/codex/issues/47925)).
- Overlay hook commands resolve from the project root. Cursor's Windows hook shell is
  undocumented and unverified.
- Non-Claude skill names carry the package prefix (`craft-engineering-foundations`).
- Generated skills link their package instructions instead of relying on the host to load
  them; links are rebased per layout, and `source-capability.json` keeps the canonical manifest.
- No package ships a live MCP connection, and exports grant no tool or model permissions.
- JetBrains and AGENTS depend on an explicit underlying agent; guidance alone executes nothing.
  For guidance-only use of an overlay, drop its hook configuration.

## Hook support

| Target | Before-tool policy | Compaction / recovery |
|---|---|---|
| Claude Code | PreToolUse; denial via exit 2 | PreCompact snapshot; SessionStart compact restores context |
| Codex | PreToolUse JSON denial | PreCompact snapshot; SessionStart compact restores context |
| Cursor | preToolUse permission response | preCompact is observational; explicit recovery |
| Copilot | preToolUse decision response | preCompact is a notification; explicit recovery |
| Gemini CLI | BeforeTool denial | PreCompress is advisory; explicit recovery |
| AGENTS | Not installed | Explicit checkpoint CLI only |

Checked against the [Claude](https://code.claude.com/docs/en/hooks),
[Codex](https://learn.chatgpt.com/docs/hooks), [Cursor](https://cursor.com/docs/hooks),
[Copilot](https://docs.github.com/en/copilot/reference/hooks-reference) and
[Gemini](https://geminicli.com/docs/hooks/reference/) references on 2026-10-16.

Each hook runs `craft hook <host> <lifecycle>`: `craft` turns the payload into lifecycle facts,
the engine decides, and `craft` writes the host's native response. Hooks make no model calls and
never activate a skill or pick a model; they add routing hints and run state checks where the
host accepts context (`adapter-report.json` lists it per event).

Failure behavior:

- Input over 64 KiB is rejected; malformed JSON gets the default allow.
- A before-shell policy that cannot evaluate denies with a configuration error. PowerShell tool
  names are normalized, so the read-only review policy also covers them.
- Context and observational handlers stay advisory and exit 0 on failure.
- Set `checkpoint.enabled` to `false` to turn off milestone snapshots.

## Repository instructions

This repository uses `AGENTS.md` without a root `CLAUDE.md` wrapper. Claude Code reads it when
its [AGENTS support](https://code.claude.com/docs/en/memory#agentsmd) is enabled; restricted
sessions may need an import the consumer adds. The build changes no user settings.
