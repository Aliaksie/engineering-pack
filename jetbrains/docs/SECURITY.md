# Security

Capability code runs under the host’s permissions. Authoring metadata does not grant approval,
install dependencies or sandbox a command. Review tools, MCP declarations and hook changes
as code executed with a developer’s access.

Treat specifications, repository files, PR text, model responses and retrieved content as
untrusted data. Packages cannot authorize remote writes, deployments or publication. Keep
credentials, private endpoints and saved production transcripts out of reusable packages.

Portable policies are pure engine decisions. Application use cases collect observations;
`hosts/` normalizes native inputs and serializes decisions. Filesystem, Git and state access
live under `platform/`; build/export code stays in `tooling/`. No native enforcement is
installed by the generic AGENTS export. The review command policies use
heuristic matching, not a complete shell parser, so they are guardrails rather than a security
boundary. Host policy and tool-side authorization remain necessary. Hook trust, event coverage
and failure behavior differ by platform; inspect the generated adapter report.

Builds validate package paths, reject symlinks, stage output and replace only managed generated
directories. They do not modify host configuration or contact services. MCP env declarations
reference credentials from the host environment; declared network effects describe tools and
do not enforce access. No live server is configured in the shipped packages.

PR review uses the consumer’s explicit GitHub configuration and gh credentials; drafts and
state stay local. Building untrusted PR code can execute arbitrary code even in a disposable
checkout. Follow the review skill’s validation limits and project authorization.

For agent changes, test tool authorization, idempotency, isolation and output validation
independently of model behavior. Content filtering or a prompt does not replace these controls.


Consumer configuration and bot runner policy are trusted inputs. An untrusted PR/MR head must
not replace them or grant itself publication rights. Invocation plans preserve argv/structured
arguments but do not execute or authorize them. Read/write policy, tool availability, credentials
and user/job authorization are distinct checks. Publication plans require matching current
base/head revisions and explicit authorization; comment API line applicability still needs checking.

Checkpoint metadata stores explicit task/authorization/next-step data and Git state, not full
transcripts or tool output. It is historical evidence, not a new authorization source. State paths
reject symlinks and use bounded, locked atomic writes. Runtime state uses `.craft/state/<worktree-id>/<session-id>/` for Git and non-Git roots,
with `shared/` for project-wide records. Ignore `.craft/state/` in Git; Craft never edits
consumer ignore rules or writes Git metadata. Session activity, retry receipts and lookup packets share this private storage
and are not provider telemetry. Context occupancy requires fresh session-bound
host observations; file reads and billing totals cannot establish it. Automatic snapshots
do not refresh the source binding of older continuation notes.

## Usage telemetry

Engineering Pack does not send telemetry remotely by default. Local metadata excludes
prompts, transcripts, code, file contents, commands, raw session IDs and credentials.
See [observability and privacy](OBSERVABILITY.md) for exact fields, scoped correlation,
retention, failure behavior, opt-out and optional OTLP export.

Personal preferences reject symlinks, non-files, oversized data and replacement during opening
on every platform. POSIX additionally requires the current user to own both the file and its
parent, with no group/other write bits. On Windows, preferences rely on the consumer's account
and directory ACLs; Node mode bits do not validate ACL privacy or ownership.
