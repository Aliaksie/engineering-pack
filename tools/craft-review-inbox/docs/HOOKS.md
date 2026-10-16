# Optional command advice

The inbox offers two pure policies, each accepting `{command}` text or a normalized `{argv}`
array and returning an allow/deny recommendation with an optional reason. Default capability
exports do not install these hooks. A consumer must supply host registration and serialize
the result using that host's actual event protocol.

`gh-guard.mjs` recognizes ordinary direct GitHub CLI operations. Administrative destruction
is rejected; posting and other recognized mutations require a deliberate per-invocation
`PR_LOOP_ALLOW_WRITE=1` marker. That marker records an exception to this advisory policy and
grants no user or host authorization. `gh pr create` remains outside the inbox restriction.
Git pushes from recognizable inbox scratch paths are also rejected.

Visible GraphQL query documents can be recognized as reads. A mutation, file indirection or
unknown document cannot establish a read and requires the same explicit exception. REST
fields imply a write unless an explicit read method is present. Prefer structured argv when
available; unsupported flags, wrappers, aliases, shell expansion and indirect execution are
not exhaustively interpreted. These policies are not a sandbox or an authorization boundary.

`gh-trim.mjs` suggests projected fields, failure-only logs or saved artifacts for selected
broad reads. `PR_LOOP_RAW=1` marks an intentional full-output request. Recognized downstream
filters and redirection are accepted as output-handling hints, not evidence of a byte or
token limit. These recommendations do not establish cost savings without measurement.

The policies execute no commands and make no remote calls. Their unit tests cover the
specified decisions, quoted literal mentions and invocation-scoped markers. Live host event
compatibility, comprehensive command coverage and performance remain unverified. The inbox
CLI itself exposes local recording and remote reads, with no posting command.
