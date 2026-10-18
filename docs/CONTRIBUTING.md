# Contributing

Run `npm run check` with Node 20+. No npm install is needed. `npm test` builds exports first.
Review skill changes for useful decision criteria, selective references and clear scope.
Keep examples portable and verify any executable behavior affected by the change.

Keep canonical content in `packages/` and native host behavior in `hosts/`. Generated output is
ignored and reproducible; never repair it by hand. Build refuses to overwrite unmanaged output.

Read [package format](PACKAGE-FORMAT.md), [adapters](ADAPTERS.md) and [security](SECURITY.md)
before adding a component. Use common principles, language discipline, application-layer
advice and provider references independently. A stack example does not impose that stack.

Keep initial source versions at 0.0.1 until a version change is requested; adapter manifests are generated.
Change the engineering craft bar in `packages/craft-engineering/standards/craft-bar.md`,
then run `npm run sync:craft-bar`. It applies to reference-index craft skills, not every
standalone method. Add focused behavioral cases for changes with material risk.

Script lint rejects shell executables and checks Node ESM syntax. Use `npm run test:unit`
only after building; `npm test` and `npm run check` build automatically.
Use `npm run validate:claude` after building when Claude Code is available. Native validation
complements isolated build tests; neither proves actual model behavior. Use [eval cases](EVALS.md)
for workflow and instruction changes, and record any live checks honestly.

Portable decisions belong in `engine/`, orchestration in `application/`, Node I/O behind
`platform/index.mjs`, wire behavior in `hosts/`, and workflow guidance in `packages/`.
External system mappings belong in `integrations/`; build, export, validation and release
code belongs in `tooling/`. Keep folders feature-oriented and the engine JSON boundary
independent of Node so another implementation can replace it without migrating use cases. Exercise real Git snapshots and hostile/partial hook payloads with isolated
fixtures. Service planning tests must never contact a real tracker or codehost.

## GitHub Actions

CI runs on push, pull request and manual dispatch. It validates manifests, catalog and source targets,
checks generated guidance, lints portable scripts, builds eight host exports and the GitHub distribution, validates exports
and runs tests on Linux/macOS/Windows with Node 24, plus Linux with the Node 20 minimum.
No dependency installation, cloud credentials or model runner is required.

After every matrix check succeeds, the packaging job rebuilds and uploads `dist/` as
`craft-capabilities-<commit-sha>` for 14 days. Hidden generated manifests are included because
the host layouts require them. The upload contains generated bundles only, not local plans
or personal state. On eligible main-branch runs, a separate job publishes the generated GitHub installation branch; see [publishing](RELEASING.md). It does not publish npm packages or create versioned releases.

The workflow defaults to read-only repository permissions and SHA-pinned official actions. Action
runtime versions are separate from the Node versions tested by this project. Local checks and hosted CI results are reported separately.

Pinned action releases: [checkout v7.0.1](https://github.com/actions/checkout/releases/tag/v7.0.1),
[setup-node v7.1.0](https://github.com/actions/setup-node/releases/tag/v7.1.0),
and [upload-artifact v7.0.2](https://github.com/actions/upload-artifact/releases/tag/v7.0.2).

Each successful matrix job also runs `npm run hooks:report` and uploads a separate source-
identified hook diagnostic artifact. Its hand-labeled corpus can report false/missed blocks;
collection success is not a policy-enforcement pass. See the
[tool/hook audit](authoring/TOOLS.md) for the measured scope and current matcher limitations.
