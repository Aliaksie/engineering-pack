# Sharing and releasing

The source project is `craft-capabilities`; its unit of reuse is an AI Capability Package.
The Claude marketplace export also uses `craft-capabilities`. A checkout directory may have
any name. Nothing is published by a local build or check.

Distribute the [MIT license](../LICENSE) with source and generated bundles. Review source files
for private data. Keep dist output, local eval results, IDE settings, credentials, plans and
PR review state out of source commits.

Run `npm run check` and, when available, the official Claude validators. Review each adapter’s
feature report. Package files and payload fixtures are checked locally; live installation,
MCP connections and model behavior need their own validation and should be reported separately.

Keep this initial iteration at 0.0.1. For an explicitly requested release/version change,
update source manifests and regenerate native manifests rather than editing them. For release
artifacts, distribute a selected generated directory with its resources, license and adapter
report, including hidden files. Follow [installation by host](ADAPTERS.md#installation-by-host)
for marketplace, local plugin and project-overlay layouts.

A source repo URL is not currently a Claude marketplace: its generated `dist/claude` directory
is the marketplace. Publishing direct marketplace installation from GitHub would require a
release branch or artifact layout that includes those generated files.

GitHub Actions builds all six exports and uploads a commit-labeled workflow artifact after
the cross-platform checks succeed. Download that artifact from the workflow run; it retains
hidden platform manifests. This is build packaging, not a GitHub Release or registry publish.
No repository write token or publish credential is required. See [contributing](CONTRIBUTING.md)
for triggers, runner coverage and retention.
