# Craft Delivery

Plan, implement, verify and maintain project context. Use `route-work` for module/skill/worker
routing and `resolve-service` for Git, codehost or task-tracker tool selection. Pasted tasks
and local files work without a tracker; remote operations need explicit service bindings.

The shared runtime is copied into exported packages at `support/{engine,application,platform,integrations,hosts,cli}/`. It produces
routing/tool plans without executing work. Optional lifecycle hooks provide routing hints
and default milestone checkpoint snapshots; they do not force skills to load on every host.
Edit hints and hint deduplication default to enabled and can be disabled. Service metadata
and bounded lookup results can be reused across turns.

Use `task-start` once for a multi-step task and `task-step` at semantic acceptance boundaries;
ordinary discovery/status calls reuse the session binding without per-feature task JSON.
Use the bundled CLI's `session` command for context occupancy, selected-guidance estimates,
pack activity and next-step advice; `session-attempt` records explicit acceptance outcomes.
Missing host telemetry stays unknown. Neither command compacts a session or switches models.
See [session status](https://github.com/Aliaksie/engineering-pack/blob/distribution/claude/docs/SESSION.md) for receipts, bounded continuation and reuse.

Code review routes to `craft-review` when available. Implementation uses the relevant
parts of `craft-engineering`. The target project still owns architecture and permissions.
See [integrations](https://github.com/Aliaksie/engineering-pack/blob/distribution/claude/docs/INTEGRATIONS.md) and [adapters](https://github.com/Aliaksie/engineering-pack/blob/distribution/claude/docs/ADAPTERS.md).

`configure-service` sets up and checks a real authorized CLI/MCP read. `ship-change` covers
independently usable commit, push, installation, deployment, publication and license phases.
Existing consumer authorization applies; these procedures do not activate services on install.


## Claude Code install

After adding the generated marketplace:

```text
/plugin install craft-delivery@craft-capabilities
```
