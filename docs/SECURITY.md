# Security

Capability code runs with the host's permissions. Package metadata grants no approval,
installs nothing and sandboxes nothing: review tools, MCP declarations and hooks as code that
runs with a developer's access. Specifications, repository files, PR text, model responses and
retrieved content are untrusted data. Packages cannot authorize remote writes, deployments or
publication, and must not contain credentials, private endpoints or production transcripts.

## Trust boundaries

- Consumer configuration and bot runner policy are trusted; an untrusted PR or MR head must
  not replace them or grant itself publication rights.
- Invocation plans keep argv and structured arguments but neither execute nor authorize them.
  Read/write policy, tool availability, credentials and user or job authorization are separate
  checks. Publication requires matching base/head revisions and explicit authorization.
- `craft check` starts the commands the project configuration declares, without a shell and
  with the caller's permissions. It runs only when called, never from a hook, and nothing it
  reports authorizes a commit, push or publication. Run it on untrusted configuration only
  where running that project's own build would be acceptable.
- The organization and user configuration layers are files outside the repository, read only
  when no other account can write them. They set defaults a project can override, so they are
  not a control over a repository's own configuration, and they hold no credentials. A check
  command a layer declares runs under `craft check` like the project's own.
- Building untrusted PR code can run arbitrary code even in a disposable checkout.
- For agent changes, test tool authorization, idempotency, isolation and output validation
  independently of model behavior; a prompt or content filter does not replace them.

## Read-only review guard

The optional review hook recognizes direct publishing commands (PR/MR and issue creation,
`api` calls whose flags imply a write), quoting, command separators, Git global options,
`-c` scripts of supported shells and active `$()`/backtick substitutions, and denies nesting
deeper than 16 levels. It is a guardrail, not a security boundary: aliases, other
interpreters and executable paths, and variable expansion are outside it. Enforce with host
tool permissions, read-only credentials and isolation. `npm run hooks:report` measures it
against a labeled corpus ([contributing](CONTRIBUTING.md#hook-diagnostics)).

## Safety policy

`safety` in `.craft/project.json` turns on rules that refuse destructive shell commands
before they run, on hosts where a hook can refuse a tool call (Claude Code, Codex, Cursor,
Copilot CLI, Gemini CLI). It is off until the project sets `enabled`:

```json
"safety": {
  "enabled": true,
  "disables": ["system-administration"],
  "deny": [{ "id": "infra-destroy", "command": ["terraform", "destroy"],
             "reason": "tearing down shared infrastructure" }]
}
```

| Rule | Refuses |
|---|---|
| `delete-outside-project` | `rm -r` whose target is outside the project root, under `~`, or an unexpanded variable or substitution |
| `delete-project-root` | `rm -r` of the project root, of everything in it (`*`, `.`) or of its `.git` |
| `force-push` | `git push` with `--force`, `-f`, `--mirror` or a `+refspec`; `--force-with-lease` passes |
| `discard-changes` | `git reset --hard`, `git checkout`/`git restore` of the whole tree, `git checkout -f`, `git clean -f` (not a dry run), `git stash drop` and `clear` |
| `system-administration` | `sudo`, `doas`, shutdown and reboot, stopping or disabling services, mounting, formatting and writing to devices |

`disables` leaves built-in rules out. `deny` adds the project's own: a command whose leading
words match is refused, with the executable matched by name and variable assignments or
wrappers such as `env` skipped. `craft settings` lists the rules in force and where each
comes from; refused commands are counted in the session summary.

The policy only refuses. It never approves a command, so the host's prompts, sandbox and
deny rules apply as before, and what the host already blocks stays blocked: Claude Code
refuses deletes of critical paths in every mode and has more rules in auto mode; Codex
confines writes with its sandbox. The policy adds one command-level rule set that is the same
on every host and still holds where the host has none, such as Claude Code's bypass mode or
Codex with full access.

Targets are judged by name, from the directory the command runs in; nothing is read from
disk, so a link that leads outside is not seen, and Windows drive paths are not recognized as
outside. Commands are read like the review guard reads them, with the same limits; a command
nested deeper than 16 levels is refused. Personal preferences and task overrides cannot
change the policy. Use host permissions, sandboxing and credentials for enforcement.

## Builds and state

Builds reject symlinked or escaping package paths, stage output, replace only managed
directories, and never touch host configuration or contact services. MCP declarations reference
credentials through host environment variables; no package ships a live server.

Runtime state lives in `.craft/state/sessions/<session-id>/`, with project-wide records in
sibling folders (`telemetry/`, `intelligence/`, `workspaces/`, `notices/`); ignore `.craft/state/` in Git, which Craft never edits. Worker worktrees live in
`.craft/worktrees/` (ignore it too); `plan-integrate` makes local merge commits and never
pushes. State paths
reject symlinks and use bounded, locked, atomic writes. On POSIX, state directories are
created owner-only (0700) and files, including the evidence database, its lock and backup,
owner-readable only (0600). Checkpoints hold explicit task data
and Git state, not transcripts or tool output, and are evidence, never authorization.

State left by earlier plugin versions is never read, written or removed.

Personal preferences reject symlinks, non-files and oversized data. On POSIX the current user
must also own the file and its parent, with no group/other write bits, and a file replaced
while opening is refused. On Windows, account and directory ACLs are relied on unchecked.

Telemetry stays local by default and excludes prompts, transcripts, code, file contents,
commands, raw session IDs and credentials; see [observability](OBSERVABILITY.md).
