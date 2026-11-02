# Craft Stories

Code grounding, specification audits and local user-story drafts.

Use the target repository’s instructions and actual configuration. Load only the skills,
workflows and standards relevant to the requested task. Logical references use
`package:skill`; adapters translate them into native skill names or local paths.
Never interpret this package as authorization for external changes.

On hook-capable hosts, observational lifecycle hooks record local operational telemetry
without routing, checkpoints or injected context. Host discovery and trust are required;
guidance-only adapters register no automatic hooks.

On first use in a conversation, give one brief setup notice for all Craft packages alongside the work: this export declares automatic hooks (`native-before-tool-requires-trust`), but installation does not prove they run. Native controls (Codex terminal CLI only; verify desktop/IDE controls separately): `/hooks` (reviewHooks), `/status` (inspectUsage), `/statusline` (configureUsage). Users can ask "Check Craft Capabilities setup, hook activity and session usage." Read [setup and session visibility](SETUP.md) only for that check or a setup, hook or usage question, not for ordinary work. A lookup that a routing hint answers or makes runnable needs no notice.
