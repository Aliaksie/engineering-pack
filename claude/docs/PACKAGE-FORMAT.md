# AI Capability Package format

`packages/<name>/capability.json` is the canonical manifest. Format version 1 is a local
contract validated by `tooling/lib/capabilities.mjs` and described by
[the JSON schema](../schemas/capability.schema.json). It does not replace host plugin formats.

| Field | Meaning |
|---|---|
| `formatVersion` | Authoring contract version, currently 1 |
| `name`, `version`, `description`, `author` | Stable capability identity, purpose and attribution |
| `instructions` | Shared `AGENTS.md` instructions |
| `skills` | Skill directory paths; each has name/description frontmatter and SKILL.md |
| `workflows` | Markdown files with name/description frontmatter; exported as workflow skills |
| `standards` | Canonical quality guidance, loaded when relevant |
| `evals` | JSON scenarios: id, prompt and named rubric criteria |
| `tools` | Executable paths with runtime and network effect declarations |
| `hooks` | Normalized lifecycle handler declarations; v1 supports session, prompt, tool, compaction and turn events |
| `mcp` | Optional server declarations; no live defaults or credential literals |
| `optionalDependencies` | Other capabilities useful when installed; absence is a reported limitation |

Declare empty arrays for unused features; do not create empty placeholder directories.
All paths are package-relative, stay inside the package and may not traverse symlinks.
Unknown top-level manifest fields and unsupported hook/MCP formats fail validation.
Runtime/network declarations describe tools; they do not grant permissions or sandbox execution.

Skill frontmatter contains only portable `name` and `description`. Put host tool grants,
model selections and permission settings in an adapter, never in shared skills. Logical
cross-package references use `package:skill`. Plain local skill names are scoped to their
own package; reference files stay next to their owning skill.

Workflows get `workflow-<name>` entry points. Claude keeps package-qualified native names;
Codex and AGENTS exports prefix skill names with the package name to avoid collisions.
Long-form workflows, standards, tools and cases also remain available as package resources.

## Hook interface

```javascript
export function evaluate({ event, tool, command, cwd, routing, checkpoint, project }) {
  return { decision: 'allow' };
  // or { decision: 'deny', reason: 'actionable explanation' }
  // or { decision: 'allow', context: 'bounded routing/recovery context' }
}
```

Handlers are synchronous entry points that return decision/context data. Portable decisions
belong in `engine/`; application use cases translate lifecycle events into facts and collect
observations. `hosts/` owns native parsing, execution and serialization. The bundled package
handlers delegate to these shared use cases rather than reimplementing policy.

The shipped bin/hook entry points import shared application modules. `tooling/export/`
rebases those imports into the installed package's `support/` tree, which includes the engine,
application, platform, integrations and CLI. Installed packages remain self-contained; do not
import authoring tooling or a sibling package at runtime. Hook tools are
`shell` or `any`; normalized events are `session-start`, `before-prompt`, `before-tool`,
`after-tool`, `before-compact`, `after-compact` and `turn-end`. A denial is mapped only where
the host supports blocking. Context is injected only where its wire schema supports it. Adapters own
stdin normalization, event/tool mapping, registration and output. A plain AGENTS bundle
has no lifecycle runtime and therefore installs no hooks.

## MCP declarations

For HTTP: `{ "name": "docs", "transport": "http", "url": "https://example.com/mcp" }`.
For stdio: `{ "name": "local", "transport": "stdio", "command": "server-on-path", "args": [],
"env": { "SERVICE_TOKEN": "${SERVICE_TOKEN}" } }`.

Commands must be available in the consumer environment; no installer is run. Environment
values refer to external variables. HTTP endpoints reject credentials, query strings and
fragments. V1 deliberately omits host-specific OAuth/header configuration; unsupported fields
fail instead of being silently discarded. Claude/Codex emit native connection files;
the AGENTS export includes declarations for a future host adapter to register.
