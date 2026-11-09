# Observed host payloads

Hook payloads a released host sent to Craft's hooks in a live session, recorded unchanged
except for local paths (`/work/shop` is the project, `/home/user/.codex` the Codex home).
Each file names the host version and the date it was observed. Unlike the frozen runtime
contracts, these record what a host sends, not what Craft answered; `expect` holds only the
facts Craft must read from each payload.

Add a payload when a host release changes what it sends; keep the old one while that release
is still supported.
