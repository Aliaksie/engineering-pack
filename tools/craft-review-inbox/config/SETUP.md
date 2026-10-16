# Review inbox setup

Use Node 20+, Git and `gh` on Windows, macOS or Linux. Configure GitHub CLI credentials using
your existing project/account policy. The tool never prompts for credentials or reads a
shell token file. Noninteractive credentials may be supplied through gh's supported process
environment; keep them out of config, logs and committed artifacts.

Create `<project>/.pr-loop/config.json` using the fields in
[the template](../templates/config.json). Required: `formatVersion: 1`, explicit `host`,
`owner`, `me`, and a `repos` object mapping stable short names to remote repository names.
Optional `home`, `artifacts`, `scratch` paths resolve against the consumer project root;
defaults are `.pr-loop`, `.pr-loop/artifacts` and `.pr-loop/scratch`. `staleDays` defaults to
5; `tickHours` defaults to 6. No environment-variable or shell expansion occurs in paths.

Run the CLI with `node <inbox>/bin/inbox.mjs <action>`. Configuration is discovered by walking
up from the working directory for `.pr-loop/config.json`, or selected with `--config <file>`.
Quote paths with spaces as required by your host; programmatic callers should use argv arrays.
Do not rely on POSIX executable bits or a particular shell.

| Action | Arguments | Effect |
|---|---|---|
| `config` | none | Print resolved nonsecret directories and repository bindings |
| `doctor` | none | Check Git/gh, configured authentication and one repository read |
| `scan` | optional `--repo <short>` | Read GitHub and update local state, ledger and inbox |
| `tick` | optional `--force`, `--hours <number>` | Scan only when a complete full scan is stale |
| `render` | none | Rebuild local ledger/inbox from stored state |
| `checkout` | `<short> <number>` | Fetch the scanned PR into a fresh owned scratch checkout; reject a moved head |
| `mark` | `<short> <number> reviewed --head <sha> [--verdict V] [--note text]` | Pin the head actually reviewed and snapshot a draft |
| `mark` | `<short> <number> posted` or `note --note text` | Record a declared local event; never post remotely |
| `record-posted` | `<short> <number> --file <actual-text-file>` or `--stdin` | Archive text the user actually posted, retain draft/posted differences and mark locally |

`config` reports state/artifact locations instead of requiring exported shell variables.
Posted-text input comes from stdin or a file. The caller must confirm the supplied
text is what was posted; the tool does not fetch or infer that assertion. Local mark events
are labeled pending remote reconciliation until the next scan.

## Failure and state behavior

Root PR pagination completes before missing PRs are marked closed. If a nested review,
comment or thread connection exceeds its requested window, the scan reports incomplete and
keeps that repository's prior state. It does not infer a clean review from truncated data.
A failed/partial scan returns nonzero and never advances full-scan freshness. A `--repo` scan
does not claim other repositories are fresh. Existing draft PRs are not newly reviewed.

State files are atomically replaced and writer actions use a local exclusive lock. A crash
can leave `.writer.lock`; after establishing no writer remains, remove that lock manually.
Multi-file updates are not a filesystem transaction: a failed write may leave partial local
updates, reported as an incomplete scan requiring retry. Render failures also remain failures.
Do not hand-edit generated state to conceal them. Back up personal state before changing
its configuration or storage location.

Checkout never resets/reuses an existing working directory or fetches into a source checkout.
It does not install dependencies or run project tests. Review of untrusted code still needs
the project's isolation and execution policy. Scratch cleanup is explicit caller scope.

Use the CLI with the host/project's scheduler if scheduled reads are desired. Scheduling
is configured by the consumer; the tool does not install a background job.

No live host credentials or remote calls were needed by automated fixture tests. Those tests
exercise the same Node implementation on the current host; they do not certify unrun OSes.
