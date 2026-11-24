# Project services and tool bindings

Host adapters cover the agent; service bindings in project configuration choose the Git,
codehost and tracker tools, and the engine turns them into invocation plans. A Codex session
can use GitLab and Jira while a Claude session uses GitHub and local task files. Delivery and
review need no MCP connector.

`craft-delivery:configure-service` sets up a binding and verifies an authorized real read;
`craft-delivery:resolve-service` only plans invocations for a working binding.

## Configuration

`<project>/.craft/project.json` is trusted configuration; see
`examples/project/.craft/project.json` and [project configuration](PROJECT-CONFIG.md#service-bindings).
Bots load bindings and permissions from trusted job or base configuration, never from an
untrusted PR head.

| Service | Operations | Binding |
|---|---|---|
| Local Git | `git.status`, `git.diff` | git CLI by default |
| Codehost | `change.read`, `change.diff` | `github` or `gitlab` setup, gh/glab binding, or an exact CLI/MCP mapping |
| Task tracker | `task.read`, `task.search` | `github`, `github-project`, `gitlab` or `jira` setup, or an exact CLI/MCP mapping |
| Mutations | `change.publish`, task transitions/comments, commit/push | Explicit mapping, project write policy and separate authorization |

Codehost reads name the repository and host explicitly; nothing infers the intended account or
remote.

### Setups

A setup is one name plus its target; it supplies the read operations, so moving a project from
one tracker to another changes this binding and nothing in skills or workflows:

```json
{
  "codehost": { "setup": "github", "repository": "acme/shop" },
  "tasks": {
    "setup": "github-project",
    "repository": "acme/shop",
    "project": { "owner": "acme", "number": 3 }
  }
}
```

| Setup | Serves | CLI (default) | MCP (`"transport": "mcp"`) |
|---|---|---|---|
| `github` | codehost, tasks | `gh pr`, `gh issue` | GitHub MCP server: `pull_request_read`, `issue_read`, `search_issues` |
| `github-project` | tasks | `gh api graphql` for one issue with its Project fields, `gh project item-list --query=` | — |
| `gitlab` | codehost, tasks | `glab mr`, `glab issue` | — (use an exact MCP mapping) |
| `jira` | tasks | Atlassian `acli jira workitem` | Atlassian MCP server: `getJiraIssue`, `searchJiraIssuesUsingJql`; needs `site` |

`task.read` takes `id` (an issue number, or a Jira key such as `TEAM-123`); `task.search` takes
`query` in the tracker's own syntax: GitHub issue search, Project filters, GitLab search text or
JQL. A query is passed as one `--flag=value` argument, so it can never become an option.

Task plans carry `response.fields`: where `id`, `title`, `body`, `status`, `priority`,
`release`, `labels` and `url` sit in that provider's response, as dotted paths with `[]` for
each element, and for a search `response.list`, the array of results. A field a provider does
not have is absent. `github-project` returns status, priority and release under those names;
`project.fields` renames the Project fields it reads, which default to `Status`, `Priority` and
`Release`; a name the Project does not have reads as `null`, like an unset field. Its search
leaves out draft items, which have no issue to read or link. MCP plans name the setup's `server` (`github`, `atlassian`, or the binding's
`server`) and the server's own tool name; the host qualifies it in its own way, and
`services:prepare` counts a qualified inventory name such as `mcp__github__issue_read` as that
tool. `github-project` reads github.com only, because `gh project` commands take no host.

A setup supplies no writes. An `operations` entry in a setup binding replaces that one built-in
mapping, shaped for the binding's transport. The `github` setup and Project reads were verified
against a live repository and Project; the `gitlab` and `jira` plans follow the
[glab](https://docs.gitlab.com/cli/issue/view/) and
[acli](https://developer.atlassian.com/cloud/acli/reference/commands/jira-workitem-view/)
references and the Atlassian MCP tool schemas without a live read.

### Exact mappings

Any other tracker needs explicit mappings, because similarly named CLIs and MCP servers have
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
