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
| `tools` | Executables with runtime and network effect declarations: a `native` `command` of the bundled runtime, run as `bin/craft <command>`, or a package `path` run by `node` or `bash` |
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

A hook declaration names a normalized event, a tool scope and the native handler that runs it:

```json
{ "name": "prompt-routing", "event": "before-prompt", "tool": "any", "handler": "deliver" }
```

| Handler | Behavior |
|---|---|
| `deliver` | Routing hints, checkpoints, service metadata and hint reuse |
| `review-read-only` | Read-only review publication guard (shell scope) |
| `observe` | Lifecycle telemetry only: no decision, no context, nonblocking on any failure |

Hooks and native tools run in the bundled `craft` executable through the generated
`bin/craft` launcher (`bin/craft.cmd` on Windows), which selects the executable installed
under the package's `support/native-bin/`. Installed packages remain self-contained; do not
import authoring tooling or a sibling package at runtime. Hook tools are `shell` or `any`;
normalized events are `session-start`, `before-prompt`, `before-tool`, `after-tool`,
`before-compact`, `after-compact` and `turn-end`. A denial is mapped only where the host
supports blocking, and context is injected only where its wire schema supports it. `craft
hook` owns payload normalization, event/tool mapping and output; the build owns registration.
A plain AGENTS bundle has no lifecycle runtime and therefore installs no hooks.

## MCP declarations

For HTTP: `{ "name": "docs", "transport": "http", "url": "https://example.com/mcp" }`.
For stdio: `{ "name": "local", "transport": "stdio", "command": "server-on-path", "args": [],
"env": { "SERVICE_TOKEN": "${SERVICE_TOKEN}" } }`.

Commands must be available in the consumer environment; no installer is run. Environment
values refer to external variables. HTTP endpoints reject credentials, query strings and
fragments. V1 deliberately omits host-specific OAuth/header configuration; unsupported fields
fail instead of being silently discarded. Claude/Codex emit native connection files;
the AGENTS export includes declarations for a future host adapter to register.

In this repository, `package.json.version` owns the pack release version. Required
capability `version` fields remain explicit for standalone portability, but are synchronized
by `npm run version:sync` and checked for drift before builds. Native exports and installed
runtime metadata use that same value.
