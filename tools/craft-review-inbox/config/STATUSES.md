# Reading the local inbox

A scan calculates a recommendation for each non-draft pull request using the configured
reviewer identity. The first applicable row below determines its status.

| Status | Condition | Suggested next action |
|---|---|---|
| `RE_REVIEW_NEEDED` | An unpublished local draft predates a commit, or its reviewed head differs | Check the new changes before using that draft |
| `DRAFT_READY` | A local draft is newer than the recorded posting event | Inspect the proposed wording |
| `NEW` | No dated contribution from this reviewer is present | Decide whether a review is needed |
| `FIXED_PENDING_VERIFY` | A commit follows the reviewer's contribution, or a participating thread became resolved | Verify the affected concern |
| `ANSWERED` | Another participant added substantive text afterward | Read the response |
| `CLEAN` | A later approval exists and no participating thread remains open | No follow-up suggested by these signals |
| `STALE_MINE` | The latest contribution is older than the configured silence interval | Decide whether follow-up is useful |
| `AWAITING_AUTHOR` | None of the conditions above applies | Wait or inspect the underlying evidence |

An empty approval can support `CLEAN`; it does not count as a textual answer. Earlier
approvals do not settle later concerns. These recommendations describe observable events,
not proof that an implementation is correct or that a question was satisfactorily answered.

A complete scan labels formerly tracked PRs absent from the open list `CLOSED_OR_MERGED`.
A failed or truncated scan retains earlier data and reports incomplete freshness separately.
The title of a status never overrides that freshness warning.

`reviewed_head_sha` records an explicitly supplied reviewed revision. Use `mark ... reviewed
--head <sha>` after inspecting that exact head. Local verdict and review timestamps survive
subsequent scans. `record-posted` archives text the user supplies and records the posting
locally; it does not send that text to GitHub. Scans do not infer a new posting event from
comment timestamps. A `posted_inferred` field denotes uncertain evidence and never establishes
the wording of an actual posted sample.

Detailed records include contributing timestamps, participating thread summaries, resolution
changes and status history. Use those fields to explain a recommendation; do not silently
edit generated state to change the result.
