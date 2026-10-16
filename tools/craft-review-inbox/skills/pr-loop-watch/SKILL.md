---
name: pr-loop-watch
description: Refresh the configured GitHub review watch list and explain local follow-up recommendations without starting reviews.
---

# Inspect the watch list

The optional inbox is a consumer-configured Node tool, not a live service binding. Run
`node <inbox>/bin/inbox.mjs config` from the consumer project directory, using the installed
tool's absolute path. When running elsewhere, pass `--config <absolute-config-file>` on
every invocation. This resolves repository aliases and output directories; do not invent
an account or repository scope.

Use `tick` for the normal freshness interval and `scan` for an explicitly requested refresh.
`doctor` reports configured tool/access problems. Read the command's completion result before
using the generated agenda. Identify stale rows when any repository failed or a connection
was truncated; an old CLEAN row cannot establish a current result.

Return the PR identities needing attention, the reason for each recommendation and any
freshness limitation. Use [status meanings](../../config/STATUSES.md) to interpret evidence.
Group unchanged waiting items compactly. A watch request ends with this agenda; reviewing
source, nudging an author and posting a comment are separate work.
