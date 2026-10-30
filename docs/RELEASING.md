# Releasing

`packages/` on `main` is the source. Two generated branches serve installation:
`distribution` (stable) and `distribution-next` (candidate). CI and manual releases publish
only to `distribution-next`; promoting a candidate to stable is a separate, explicit
decision. Nothing is published to npm.

`package.json` holds the pack version. Capability manifests and runtime metadata are
synchronized projections: `npm run version:sync -- patch|minor|major|<x.y.z>` updates them
and `npm run version:check` rejects drift. Marketplace identifiers stay `craft-capabilities`
(Claude) and `engineering-pack` (Codex, Gemini).

## Candidate branch

Every successful push to `main` (or **CI → Run workflow** on `main`) runs the **Publish**
job. It downloads the packaged bundles from the same run and commits them to
`distribution-next` with the source revision in the commit. Only that job has write
permission; pull requests and forks cannot publish. The branch must contain:

```text
.claude-plugin/marketplace.json  → claude/plugins/<package>/
.agents/plugins/marketplace.json → codex/packages/<package>/
gemini-extension.json            → skills/ + packages/
jetbrains/                       → project rule and portable resources
```

Never edit the generated branch by hand. To reproduce it locally:

```sh
npm run check
npm run build:distribution
claude plugin validate ./dist/distribution --strict
```

## Versioned release

1. In a PR, run `npm run version:sync -- <bump>` and add the matching `CHANGELOG.md` section.
2. After merging, run **Release → Run workflow** on `main`.

The workflow validates the version, changelog and tag absence, runs workflow linting, dependency
security checks, CodeQL and `npm run check`. It builds and tests release `craft` on Linux
x86-64, macOS Apple Silicon and Windows x86-64. It
then archives the eight adapter bundles, the distribution and the three native bundles with
`native-release.json`, provenance and `SHA256SUMS`. Before publication it creates GitHub
artifact attestations for all fifteen assets. It pushes only the `vX.Y.Z` tag, refreshes
`distribution-next` at that revision and publishes the GitHub Release once all fifteen assets
are uploaded. It never bumps versions, commits to `main` or forces a ref. The repository must
allow the workflow token to write tags, releases and the installation branch.

Consumers can verify a downloaded asset against this repository's build identity:

```sh
gh attestation verify <downloaded-asset> --repo Aliaksie/engineering-pack
```

The workflow uses GitHub OIDC and artifact attestations for this public repository; it needs
no signing secret or paid service.

### Recovery

A failure before the tag push changes nothing remote. After it, the tag stays: never move,
delete or re-version it. Recovery artifacts are kept for 14 days. Verify that the tag resolves
to the recorded revision and the checksums match, then retry only the missing step (branch
update, asset upload or publishing the draft) as an explicitly authorized action. Rerunning
the workflow refuses the existing tag.

## Benchmarking a native candidate

The **Performance** workflow measures every pull request and `main` on all three platforms and
tracks results in Bencher; see [CONTRIBUTING](CONTRIBUTING.md#performance). To measure a
candidate export locally, run `.github/benchmarks/suite.mjs` against it.
