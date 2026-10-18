# Sharing and releasing

The source branch is `main`; `packages/` remains the authoritative engineering core.
The generated `distribution` branch is the GitHub installation channel for Claude, Codex
and Gemini, and includes a downloadable JetBrains overlay. npm publication is unnecessary.
The Claude marketplace identifier stays `craft-capabilities`; Codex uses `engineering-pack`.
All initial source/native manifest versions stay at `0.0.1` until a version change is requested.

## Activate GitHub installation

Commit and push the source changes, including `.github/workflows/ci.yml`, to `main`.
After Linux, macOS and Windows checks and the packaging job pass, CI creates or updates
`distribution` using the job's GitHub token. No personal access token or npm secret is needed.
Alternatively run **CI → Run workflow** on `main` to retry publication. Repository rules must
allow GitHub Actions to write the `distribution` branch; organization rules may restrict it.

Check that the **Publish GitHub installation branch** job succeeded and the branch contains:

```text
.claude-plugin/marketplace.json  → claude/plugins/<package>/
.agents/plugins/marketplace.json → codex/packages/<package>/
gemini-extension.json            → skills/ + packages/
jetbrains/                      → project rule and portable resources
```

Then use the [GitHub installation commands](PLATFORMS.md) from a disposable consumer project.
The source branch itself is not the installable extension; preserve the documented branch
selector in commands. No public marketplace approval or gallery listing is implied.

## Build and verify locally

```sh
npm run check
npm run build:distribution
claude plugin validate ./dist/distribution --strict
```

The assembly script preserves relative links, includes hidden manifests and licenses,
validates the Claude marketplace and refuses unmanaged/symlinked output. Tests check
reproducibility, discovery roots and resource paths. Git transport tests use a local bare
remote to exercise branch creation, no-op updates, history preservation, deletions and
stale-source rejection without publishing anything.

The publisher commits only assembled output, never the checkout, plans, credentials,
workflow files or `.git` data. It preserves distribution history, uses normal fast-forward
pushes and refuses a source revision that is no longer `main`. Write permission is limited
to the main-branch publish job; pull requests and forks cannot publish. Distribution commits
contain no workflow, and CI ignores pushes to that branch.

## Updates and releases

Every successful eligible CI run refreshes the generated branch. Users update with native
platform commands; the [installation guide](PLATFORMS.md#updates-and-verification) explains
initial-version caching limits. CI records the source revision in each distribution commit.
Keep `dist/`, local eval results and plans out of source commits. Do not edit the generated
branch manually. The build contains the [MIT license](../LICENSE).

For a requested versioned release, update canonical manifests and regenerate adapters.
Release tags, npm publication, a public gallery listing and automatic semantic model
certification are intentionally outside this workflow. Live host loading and model behavior
require separate evidence. Existing CI artifacts retain all eight development exports.
