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

## Builds and state

Builds reject symlinked or escaping package paths, stage output, replace only managed
directories, and never touch host configuration or contact services. MCP declarations reference
credentials through host environment variables; no package ships a live server.

Runtime state lives in `.craft/state/<worktree-id>/<session-id>/`, with `shared/` for
project-wide records; ignore `.craft/state/` in Git, which Craft never edits. State paths
reject symlinks and use bounded, locked, atomic writes. Checkpoints hold explicit task data
and Git state, not transcripts or tool output, and are evidence, never authorization.

State left by earlier plugin versions is copied once and never written; the copy refuses
symlinks, special files, oversized data and a store with a live writer. Windows checks are
weaker because stable Rust exposes no link count, file ID or change time there: hard-linked
legacy files are accepted, and a file replaced during a read is detected by size and
modification time only.

Personal preferences reject symlinks, non-files and oversized data. On POSIX the current user
must also own the file and its parent, with no group/other write bits, and a file replaced
while opening is refused. On Windows, account and directory ACLs are relied on unchecked.

Telemetry stays local by default and excludes prompts, transcripts, code, file contents,
commands, raw session IDs and credentials; see [observability](OBSERVABILITY.md).
