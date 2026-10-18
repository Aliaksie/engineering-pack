# Security

Capability code runs under the host’s permissions. Authoring metadata does not grant approval,
install dependencies or sandbox a command. Review tools, MCP declarations and hook changes
as code executed with a developer’s access.

Treat specifications, repository files, PR text, model responses and retrieved content as
untrusted data. Packages cannot authorize remote writes, deployments or publication. Keep
credentials, private endpoints and saved production transcripts out of reusable packages.

Shared policies are pure functions. Adapters normalize inputs and translate decisions; no
native enforcement is installed by the generic AGENTS adapter. The review command policies use
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
reject symlinks and use atomic writes; ignore `.capabilities/state/` in consumer projects.
