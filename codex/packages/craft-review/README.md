# Craft Review

Load `review-change` in an interactive agent or a PR/MR bot. The package takes an immutable
diff snapshot and produces revision-bound findings; it does not assume GitHub, a tracker,
MCP or a personal inbox. GitHub/GitLab bindings can supply remote metadata separately.

Use optional `craft-engineering` lookup and risk-specific guidance and `craft-delivery`
decision/service skills when installed; direct inspection remains the fallback. Supplied
or explicitly linked Jira/task criteria, relevant contracts, ADRs, head-specific CI and prior
findings can support the review through the consumer's authorized reads.

Reports classify findings as high/medium/low and recommend `approve`, `request-changes` or
`needs-verification`. A clean review can explicitly say the change looks good for approval;
missing required evidence prevents that conclusion. Recommendations do not post approvals.

`bin/prepare-review.mjs` snapshots a local Git revision range without running project code.
`bin/check-review.mjs` validates the returned findings against that snapshot. Publication
belongs to a separately authorized job, which must check that the remote base and head still match.
See [review integration](https://github.com/Aliaksie/engineering-pack/blob/distribution/codex/docs/REVIEW.md) and [service bindings](https://github.com/Aliaksie/engineering-pack/blob/distribution/codex/docs/INTEGRATIONS.md).
