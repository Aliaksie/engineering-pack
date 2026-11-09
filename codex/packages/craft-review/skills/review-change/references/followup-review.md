# Follow up on an earlier review

To decide what a GitHub pull request you reviewed needs next, fetch it read-only with the
consumer's authorized access, for example `gh api graphql -F owner=<owner> -F name=<repo>
-F number=<n> -F query=@<skill-dir>/references/followup-pull-request.graphql > <file>` (or
[followup-open.graphql](followup-open.graphql) for recently updated open PRs). Then
run `bin/craft review-followup --observation <file> --reviewer <login>`. Each result carries a
`status`: `new` (no contribution from you), `fixed-pending-verify` (a commit after your
contribution, a head other than the one you last reviewed, or a participating thread resolved
since), `answered`, `clean` (a later approval and none of your threads open), `stale`,
`awaiting-author` or `closed`, plus the signals behind it. Once you have verified a resolved
thread, pass its ID in `--acknowledged-threads`. Exit status 1 means some evidence was
truncated or unreadable; fetch it again rather than assume silence. The status summarizes
observable events; it does not show that a concern was fixed or answered well.
