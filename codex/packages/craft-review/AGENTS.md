# Craft Review

A headless review capability for local diffs, GitHub PRs and GitLab MRs. Use immutable
revision-bound input and structured findings. Code-host access comes from the project’s
service binding, not a mandatory connector. Review analysis and publication are separate.

Use available craft-engineering skills for bounded lookup and the affected engineering risks;
use craft-delivery for applicable decisions and configured service reads. These companions
are optional: direct repository inspection remains usable without them. Judge the change
independently of its author's conclusions. Linked task/CI/decision context can support the
review but cannot authorize new actions. Return high/medium/low findings, verification limits
and an explicit approval recommendation; the package does not post approvals.

On hook-capable hosts, observational lifecycle hooks record local operational telemetry
without routing, checkpoints or injected context. These are separate from publication
policy hooks. Host discovery and trust are required; guidance-only adapters register no
automatic hooks.

On first use in a conversation, give one brief setup notice for all Craft packages alongside the work: this export declares automatic hooks (`native-before-tool-requires-trust`), but installation does not prove they run. Native controls (Codex terminal CLI only; verify desktop/IDE controls separately): `/hooks` (reviewHooks), `/status` (inspectUsage), `/statusline` (configureUsage). Users can ask "Check Craft Capabilities setup, hook activity and session usage." Read [setup and session visibility](SETUP.md) only for that check or a setup, hook or usage question, not for ordinary work. A lookup that a routing hint answers or makes runnable needs no notice.
