---
name: pr-loop
description: Select watch-list refresh, exact-revision local review or consumer-owned comment writing for the optional GitHub inbox.
---

Choose the requested result: `pr-loop-watch` provides an agenda, `pr-loop-review` prepares
artifacts for one PR, and `pr-loop-voice` works with local writing evidence. Resolve missing
identity from the consumer configuration; ask only when the requested repository or PR
remains ambiguous.

Invoke `node <inbox>/bin/inbox.mjs` by its installed absolute path from the consumer
project directory. Otherwise supply `--config <absolute-config-file>` on every command. Report
incomplete remote reads before relying on prior state. End at the requested local result.
A watch does not authorize source execution, and a draft does not authorize publication.
