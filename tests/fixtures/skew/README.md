# Version skew fixtures

`<version>/` holds what a released `craft` binary wrote in one short Claude session, so that
`cargo test -p craft-application --test skew_replay` can replay it with the current build:
the user's `preferences.json`, the project's `.craft/project.json` and its `.craft/state`
(stored as `state/`, since `.craft/state` is ignored by Git). The session ran hooks at
startup, prompt, edit, test run, compaction and turn end, saved a checkpoint and ran a
commit-stage check.

`capture.sh <released-craft-binary> <new-directory>` records it: an isolated `HOME`, a
repository committed with a fixed author and date (the replay recreates the same commit),
then the hook sequence. Copy `home/.config/craft/preferences.json`,
`project/.craft/project.json` and `project/.craft/state` without lock files.

State formats are compatibility contracts: a replay failure is a regression to fix in the
reader, not a reason to recapture.

| Fixture | Binary |
|---|---|
| `0.0.6/` | `v0.0.6` release asset, `aarch64-apple-darwin` |
