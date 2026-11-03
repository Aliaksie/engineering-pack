---
name: resolve-service
description: "Produce an explicit CLI or MCP invocation plan for a configured Git, codehost or tracker operation."
---

<!-- Generated from packages/; edit the portable source and rebuild. -->

Read the shared [package instructions](../../AGENTS.md) when using this capability.

# Resolve the invocation, then assess execution

Match the requested action to the consumer's `.craft/project.json` bindings. Local
Git, the code host and the task tracker have different identities and permission boundaries.
Do not choose an account, remote or project merely because a tool can reach it. Local story
text needs no tracker connection.

Use the exported runtime:

```text
<package-root>/bin/craft operation --root <project> --operation <name> --params <json>
```

The authoring equivalent is `npm run operation -- ...`. Supported normalized operations
include `git.status`, `git.diff`, `change.read`, `change.diff`, `task.read` and `task.search`;
write mappings additionally depend on explicit project write policy. Preserve argv arrays
or structured MCP arguments through execution. Never embed remote text into a shell command.

Reuse the session's prepared service metadata. `prepare-services` reads configuration without
network calls and can compare exact command/tool names with inventories supplied by the host.
A name not observed in an inventory is not proof of absence. Cached metadata also does not
prove current authorization, account identity or remote state freshness.

Before invoking the plan, confirm the actual target, host capability and existing user scope.
Return planning errors as missing configuration, not a fabricated successful connection.
Use `craft-delivery:configure-service` when a mapping needs setup or a real authorized read
needs verification. A writable mapping and read credentials do not themselves authorize
posting, transitions, pushes or publication.
