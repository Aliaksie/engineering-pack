# Project services and tool bindings

Coding-agent hosts and external systems have separate owners: `hosts/` handles native
observations and controls; `integrations/` maps external service transports. A Codex session can use
GitLab and Jira; a Claude session can use GitHub and local task files. No MCP connector is
required by delivery or review.

Use `craft-delivery:configure-service` to discover/configure a binding and verify an authorized
real read. Use `craft-delivery:resolve-service` when a working binding only needs invocation
planning. Configuration and supported tooling belong to the consumer; builds activate nothing.

## Prepare once, reuse within scope

At session start or first service need, prepare configured bindings in one call:

```text
npm run services:prepare -- --root <project>
```

The summary includes exact commands/MCP tool names, required parameter names and missing
mappings. It makes no network calls and runs no tool discovery. A host can supply observed
names with repeated `--available-command` or `--available-mcp-tool` flags; unseen names remain
not observed, not proven unavailable. Tools, authentication and account identity are distinct.

Use host discovery once for the relevant missing schemas/connections, then reuse the compact
operation/tool/parameter map for that session and account/scope. Refresh after binding or
connection changes, account/scope changes, or relevant authorization/schema failures. Avoid
eager probes of unused services. Stable metadata can be reused; each operation still uses
current project policy and actual authorization, and mutable remote results need their own
freshness check. The runtime persists neither a credential cache nor a global discovery cache.

`<project>/.capabilities/project.json` is trusted project configuration. See the configuration
skeleton at `examples/project/.capabilities/project.json` and the
[project schema](../schemas/project.schema.json). For bots, load bindings and permissions
from trusted job/base configuration, never let an untrusted PR head replace privileged settings.

| Service | Operations | Binding |
|---|---|---|
| Local Git | `git.status`, `git.diff` | git CLI by default |
| Codehost | `change.read`, `change.diff` | Explicit gh/GitHub or glab/GitLab binding, or exact custom CLI/MCP mapping |
| Task tracker | `task.read`, `task.search` | Explicit CLI argv template or exact discovered MCP tool and arguments |
| Mutations | `change.publish`, task transitions/comments, Git commit/push | Explicit mapping, project write policy and separate user/job authorization |

```sh
npm run operation -- --root examples/project --operation task.read --params '{"id":"TEAM-123"}'
```

The example uses POSIX shell quoting for JSON; adapt quoting to your shell. It returns a
tool plan, not execution. CLI plans contain command, argument array and
`shell: false`; the planner never concatenates task text into a shell program. A consumer’s executable
must still treat its arguments as data, and a custom binding must not feed untrusted parameters
into an interpreter/eval template. MCP plans contain an
exact tool name and structured arguments. Tools/credentials remain unverified until the host
resolves them. Read access does not imply publication permission.

The built-in codehost read mappings use an explicit repository/host; they do not infer
which account or remote the user intended. Tracker mappings are explicit because similarly
named third-party CLIs and MCP servers have different schemas. A pasted task or local file
bypasses tracker access; an inaccessible task ID is reported as missing capability.

For example, an MCP task binding can be supplied as:

```json
{
  "provider": "jira",
  "transport": "mcp",
  "operations": {
    "task.read": { "tool": "mcp__team_jira__read_issue", "arguments": { "issueKey": "{id}" } }
  }
}
```

This is an example mapping, not a claimed universal Jira tool. Replace it with the exact
available tool schema. Authentication belongs to the host environment/connection, not the
package or config examples. A mapping does not bypass host permissions or execute itself.

For tool-output normalization beyond local snapshots, the caller must validate the actual
provider response. This iteration supplies invocation planning, not a universal Jira/GitLab
response decoder or remote executor. Remote diff output should be saved to an artifact,
then read in bounded sections by the reviewer.

Built-in codehost read plans follow the [GitHub CLI](https://cli.github.com/manual/gh_pr_view)
and [GitLab CLI](https://docs.gitlab.com/cli/mr/view/) command interfaces; live availability and
authentication still need verification in the consuming environment.
