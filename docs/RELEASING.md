# Sharing and releasing

The source branch is `main`; `packages/` remains the authoritative engineering core.
The frozen `distribution` branch remains the stable GitHub installation channel.
Routine CI and manual releases publish candidates only to `distribution-next`, including
Claude, Codex, Gemini and the downloadable JetBrains overlay. Stable promotion requires
the explicit `0.0.3` release after comparison and stabilization; neither publisher updates
`distribution`. npm publication is unnecessary.
The Claude marketplace identifier stays `craft-capabilities`; Codex uses `engineering-pack`.
The canonical pack version is `package.json.version`; the initial versioned release is `0.0.2`.
Capability manifests and the bundled runtime version are mechanically synchronized
projections. `npm run version:check` rejects drift, and every build checks consistency.

## Activate GitHub installation

Commit and push the source changes, including `.github/workflows/ci.yml`, to `main`.
After Linux, macOS and Windows checks and the packaging job pass, CI creates or updates
`distribution-next` using the job's GitHub token. No personal access token or npm secret is needed.
Alternatively run **CI → Run workflow** on `main` to retry publication. Repository rules must
allow GitHub Actions to write the `distribution-next` branch; organization rules may restrict it.

Check that the **Publish candidate installation branch** job succeeded and the branch contains:

```text
.claude-plugin/marketplace.json  → claude/plugins/<package>/
.agents/plugins/marketplace.json → codex/packages/<package>/
gemini-extension.json            → skills/ + packages/
jetbrains/                      → project rule and portable resources
```

To test candidates, select `distribution-next` explicitly in the
[GitHub installation commands](PLATFORMS.md) from a disposable consumer project.
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
workflow files or `.git` data. It preserves candidate distribution history, uses normal fast-forward
pushes and refuses a source revision that is no longer `main`. Write permission is limited
to the main-branch publish job; pull requests and forks cannot publish. Distribution commits
contain no workflow, and CI ignores pushes to that branch.

## Updates and releases

Every successful eligible CI run refreshes `distribution-next`. Users update with native
platform commands; the [installation guide](PLATFORMS.md#updates-and-verification) explains
rolling-update caching limits. CI records the source revision in each distribution commit.
Keep `dist/`, local eval results and plans out of source commits. Do not edit the generated
branch manually. The build contains the [MIT license](../LICENSE).

## Manual versioned release

After merging the PR, choose **Release → Run workflow** on `main` to publish the
already-reviewed version, initially `v0.0.2`. Releases remain manual, independent of
ordinary PR merges. The workflow does not bump versions, create source commits or
push changes to main. Source changes reach main only through the repository's PR
merge process using its squash/fast-forward policy; tags identify the resulting merged SHA without depending on individual
feature-branch commits or merge-commit ancestry.

For later releases, prepare the next version and its `CHANGELOG.md` entry in a PR.
`npm run version:sync -- patch` (or `minor`, `major`, or an explicit stable version)
synchronizes required projections locally; do not edit copies independently.
Format/schema versions are separate from the pack release version.

The workflow validates the selected main SHA, checks version consistency, requires one
nonempty matching changelog section, checks tag absence, and runs `npm run check` again
before publication. Nested consumer telemetry is ignored so these checks do not leave
generated state that would violate the clean-checkout guard.
It archives all eight adapter bundles plus the combined distribution, preserving hidden
files, and records source/release SHAs and SHA-256 checksums. A normal Git push publishes
only the `vX.Y.Z` tag at the reviewed main revision. An advanced main detected by the
pre-publication guard, existing tag, version mismatch, failed check or rejected push
blocks publication; refs are never forced.

The release and rolling distribution publisher share a concurrency group. The release
explicitly refreshes the candidate installation branch at the validated release revision because
tag publication does not replace installation-branch verification. A draft GitHub Release
uses the selected changelog section plus source provenance as its notes. All eleven
assets (nine archives, provenance and checksums) are uploaded and
counted, and only then is the release published. Archive names include the pack version;
build reports, native manifests, distribution marker and runtime metadata carry it too.
No npm publication, hosted analytics setup or semantic model certification occurs.

The repository must allow GitHub Actions to write release tags, releases and the
installation branch using the workflow token. Main retains its PR-only update policy;
the release workflow does not require permission to update it. Never bypass branch rules.
If those rules forbid the workflow push, arrange an approved repository policy before
running a real release. Local tests do not establish these remote permissions.

### Partial failure and recovery

Before the tag push, failed validation leaves remote refs unchanged. After the push,
a failed installation update, asset upload or release publication does not roll back the
tag. Recovery artifacts, including generated release notes, are retained for 14 days.
Do not change the version to recover the same
release, delete tags or move an existing tag. Verify that the remote tag resolves to the
recorded release revision and that the version and checksums agree with retained artifacts.
Then retry only the missing installation/publication step against that revision, using
an explicitly authorized workflow or GitHub CLI action. For a draft release, upload only
missing assets, verify all eleven assets/checksums, and publish the draft. A workflow
rerun from preparation deliberately refuses the existing tag. Recovery is a deliberate
external action; this documentation grants no agent permission to execute it.

The official Claude validator can additionally be run with `npm run validate:claude` when
its CLI is installed. Local Git tests cover version guards and ref behavior without a
GitHub account. Live host loading, repository permission enforcement and model behavior
require separate evidence.
