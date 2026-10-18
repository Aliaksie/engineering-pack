# Optional personal review inbox

A local GitHub watch list, review ledger and posted-text corpus. Excluded from default
capability exports; run it from this repository checkout so shared engine and integration
modules are available. Headless review uses `packages/craft-review` instead.

Node 20+, Git and GitHub CLI (`gh`) are the only runtime prerequisites. No Bash, jq,
Unix pipelines, OS clipboard or platform scheduler is required. Run:

```text
node tools/craft-review-inbox/bin/inbox.mjs config --config <project>/.pr-loop/config.json
node tools/craft-review-inbox/bin/inbox.mjs doctor --config <project>/.pr-loop/config.json
node tools/craft-review-inbox/bin/inbox.mjs tick --config <project>/.pr-loop/config.json --force
```

Copy the fields from [config.json](templates/config.json) into your consumer configuration.
Use [setup](config/SETUP.md) for subcommands and limitations, and
[statuses](config/STATUSES.md) for the state machine. No command posts, approves,
merges or writes to a remote. Reads require the configured account's normal access.

Configuration is secret-free JSON. There is no automatic shell evaluation or token-file sourcing.
