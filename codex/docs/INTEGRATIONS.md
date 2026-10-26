# Project services and tool bindings

Host adapters cover the agent; service bindings in project configuration choose the Git,
codehost and tracker tools, and the engine turns them into invocation plans. A Codex session
can use GitLab and Jira while a Claude session uses GitHub and local task files. Delivery and
review need no MCP connector.

`craft-delivery:configure-service` sets up a binding and verifies an authorized real read;
`craft-delivery:resolve-service` only plans invocations for a working binding.

## Configuration

`<project>/.craft/project.json` is trusted configuration; see
`examples/project/.craft/project.json` and the [project schema](../schemas/project.schema.json).
Bots load bindings and permissions from trusted job or base configuration, never from an
untrusted PR head.

| Service | Operations | Binding |
|---|---|---|
| Local Git | `git.status`, `git.diff` | git CLI by default |
| Codehost | `change.read`, `change.diff` | Explicit gh/GitHub or glab/GitLab binding, or an exact CLI/MCP mapping |
| Task tracker | `task.read`, `task.search` | Explicit CLI argv template or exact MCP tool and arguments |
| Mutations | `change.publish`, task transitions/comments, commit/push | Explicit mapping, project write policy and separate authorization |

Codehost reads name the repository and host explicitly; nothing infers the intended account or
remote. Tracker mappings are always explicit because similarly named CLIs and MCP servers have
different schemas:

```json
{
  "provider": "jira",
  "transport": "mcp",
  "operations": {
    "task.read": { "tool": "mcp__team_jira__read_issue", "arguments": { "issueKey": "{id}" } }
  }
}
```

Replace the tool and arguments with the schema your host actually exposes. Authentication
belongs to the host connection, never to configuration. A pasted task or local file needs no
tracker; an inaccessible task ID is reported as a missing capability.

## Prepare and plan

```sh
npm run services:prepare -- --root <project>
npm run operation -- --root <project> --operation task.read --params '{"id":"TEAM-123"}'
```

`services:prepare` summarizes every binding once per session: commands or MCP tool names,
required parameters and missing mappings. It makes no network calls; pass observed names with
`--available-command` or `--available-mcp-tool`, and anything unseen stays "not observed".
Reuse the result for that session and account, and refresh it after binding, connection,
account or schema changes. Nothing persists credentials or a global discovery cache.

`operation` returns a plan and executes nothing. A CLI plan is a command with an argument array
and `shell: false`; task text is never concatenated into a shell program, and a custom binding
must not feed parameters into an interpreter. An MCP plan is an exact tool name with structured
arguments. Read access never implies publication permission.

Callers validate provider responses themselves; there is no universal response decoder or
remote executor. Save remote diffs to a file and read them in bounded sections. Built-in plans
follow the [GitHub CLI](https://cli.github.com/manual/gh_pr_view) and
[GitLab CLI](https://docs.gitlab.com/cli/mr/view/) interfaces.
