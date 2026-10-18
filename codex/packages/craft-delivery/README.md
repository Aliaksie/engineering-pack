# Craft Delivery

Plan, implement, verify and maintain project context. Use `route-work` for module/skill/worker
routing and `resolve-service` for Git, codehost or task-tracker tool selection. Pasted tasks
and local files work without a tracker; remote operations need explicit service bindings.

The shared runtime is copied into exported packages at `support/runtime/`. It produces
routing/tool plans without executing work. Optional lifecycle hooks provide routing hints
and opt-in checkpoint snapshots; they do not force skills to load on every host.
Edit hints are opt-in; session service metadata and bounded lookup results can be reused
across turns instead of repeating discovery and large context dumps.

Code review routes to `craft-review` when available. Implementation uses the relevant
parts of `craft-engineering`. The target project still owns architecture and permissions.
See [integrations](https://github.com/Aliaksie/engineering-pack/blob/distribution/codex/docs/INTEGRATIONS.md) and [adapters](https://github.com/Aliaksie/engineering-pack/blob/distribution/codex/docs/ADAPTERS.md).

`configure-service` sets up and checks a real authorized CLI/MCP read. `ship-change` covers
independently usable commit, push, installation, deployment, publication and license phases.
Existing consumer authorization applies; these procedures do not activate services on install.

