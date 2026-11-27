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

`craft service-setup` proposes these bindings from facts already on the machine and names the
fact behind each choice. The Git remote gives the code host: `origin`, or the one repository
every remote names, on github.com, gitlab.com, or a host where a GitLab issue URL was named; a
host's name alone does not count. The tracker comes from issue, GitHub Project and Jira Cloud
(`*.atlassian.net`) URLs that the project's prompts or plan links named. A hook keeps only their
provider, host, repository, project or site, never the prompt. Issues count only for the code
host's own repository, and a GitHub Project only when its owner owns that repository. `gh`,
`glab` or `acli` on `PATH`, or a read tool the host reports with `--available-mcp-tool`, picks
the transport. An unknown host, trackers of different kinds, or no fact at all propose nothing,
with the reason. `--apply` adds only the bindings the project lacks to `.craft/project.json`; a
configured binding is never replaced. A session start mentions a new proposal once, and `craft
doctor` lists it while the project has no `codehost` or `tasks` binding.

`task.read` takes `id` (an issue number, or a Jira key such as `TEAM-123`); `task.search` takes
`query` in the tracker's own syntax: GitHub issue search, Project filters, GitLab search text or
JQL. A query is passed as one `--flag=value` argument, so it can never become an option.
GitHub MCP search adds `is:issue`, because GitHub's issue search also matches pull requests.

Task plans carry `response.fields`: where `id`, `repository` (where the response names it),
`title`, `body`, `status`, `priority`,
`release`, `labels` and `url` sit in that provider's response, as dotted paths with `[]` for
each element, and for a search `response.list`, the array of results. A field a provider does
not have is absent. `github-project` returns status, priority and release under those names;
`project.fields` renames the Project fields it reads, which default to `Status`, `Priority` and
`Release`; a name the Project does not have reads as `null`, like an unset field.

A GitHub Project can hold issues from several repositories, pull requests and drafts. Its search
returns issues only, each with its `repository`; drafts and pull requests are left out, because
neither is an issue that `task.read` can read or a ticket can link. `task.read` on
`github-project` also takes `repository` (`owner/name`), so a searched task reads back from its
own repository as the same issue: `{"id": 12, "repository": "acme/api"}`. Without it, the read
uses the binding's repository. No other read takes `repository`; one passed there is refused.

`craft operation --operation task.read --response <file>` takes the JSON the read returned and
gives the task in Craft's shape, whatever the setup:

```json
{"operation": "task.read", "setup": "github-project",
 "task": {"id": "12", "repository": "acme/shop", "title": "…", "body": "…", "status": "Ready", "labels": ["feature"],
          "priority": "P1", "release": "2.4", "url": "https://github.com/acme/shop/issues/12"}}
```

`id` is text; `repository` is the issue's `owner/name` (or GitLab path) — from the response for
`github-project`, otherwise the binding's, and `null` for Jira; `labels` is a list; `release` is one name, or a list when a task names several,
such as Jira fix versions. A field the response does not hold is `null`, and an empty label
list stays `[]`. Values are the tracker's own: status `OPEN`, `opened` or `In Progress` is not
mapped to another vocabulary. A Jira description in Atlassian Document Format is read as
Markdown text: headings, paragraphs, and bulleted, numbered and task lists in their order, so its
acceptance list is found like any other body's; a description with nothing to show, such as an
empty document or one holding only attachments, or any other body that is not text, is `null`. A `url` must be `https`; Jira's comes from `site`. `craft ticket
--issue-data` reads the issue from the same translation.

`--operation task.search --response <file>` gives `{"operation", "setup", "tasks": [...]}`, each
task in the same shape, from the result list the setup's search returns. An empty result is
`[]`; a response that is not that list, or a result without an id, is refused.

`--operation change.read --response <file>` gives the change a `github` or `gitlab` code host
returned, through `gh pr view`, the GitHub MCP server or `glab mr view`:

```json
{"operation": "change.read", "setup": "github",
 "change": {"id": "212", "title": "…", "status": "MERGED",
            "base": {"ref": "main", "sha": "e1b3598…"}, "head": {"ref": "fix/hooks", "sha": "9ac0f11…"},
            "url": "https://github.com/acme/shop/pull/212"}}
```

`ref` is the branch name and `sha` the commit the code host reports for each side; GitLab's
base `sha` is its merge-request base, and until GitLab has computed it the head `sha` is the merge
request's own `sha`. As with tasks, a missing field is `null` and status is the provider's own
(`MERGED`, `OPEN`, `opened`), with one exception: the GitHub MCP server reports a merged pull
request as `closed` with a separate flag, and Craft gives `merged` for it. `setup` is `null` for a code host bound by
provider without a setup. A code host whose binding maps `change.read` itself has no Craft
shape: Craft cannot know where that mapping's fields sit. `change.diff` output stays a diff.

MCP plans name the setup's `server` (`github`, `atlassian`, or the binding's
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

Apart from a setup's `task.read` translation, callers validate provider responses
themselves; there is no remote executor. Save remote diffs to a file and read them in bounded sections. Built-in plans
follow the [GitHub CLI](https://cli.github.com/manual/gh_pr_view) and
[GitLab CLI](https://docs.gitlab.com/cli/mr/view/) interfaces.
