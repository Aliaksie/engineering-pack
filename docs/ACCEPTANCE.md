# Portable acceptance contracts

The engine exposes opt-in JSON validation and criterion coverage. Task commands can
persist structured contracts and evidence; required gaps prevent structured completion.
Existing tasks remain usable with agent-declared completion and unknown criterion coverage.

| Operation | Input | Result |
|---|---|---|
| `task-context-validate` | `context` | Validated task context |
| `engineering-contract-validate` | `contract` | Validated engineering contract |
| `engineering-contract-digest` | `contract` | SHA-256 of recursively key-sorted JSON |
| `acceptance-evidence-validate` | `evidence` | Validated evidence ledger |
| `acceptance-coverage` | `contract`, `evidence`, `current`, optional boolean `blockers` | Applicability and per-criterion coverage |
| `qa-projection` | `contract`, `evidence`, `current`, `probes`, optional boolean `blockers` | Read-only charter/probe view of the same coverage |
| `product-projection` | `contract`, `evidence`, `current`, optional `focus` and boolean `blockers` | Acceptance gaps, unresolved choices and cited dependencies |
| `architecture-projection` | Same as product projection | Boundaries, dependencies, consequences, ADR sources and evidence gaps |

All use the existing version-1 engine envelope. Each payload independently uses
`formatVersion: 1`; this is unrelated to package release versions. Unknown fields and
unsupported payload versions fail. Contract arrays preserve order in the digest.
Source-checkout examples live in `tests/fixtures/acceptance/contract.json` (two
distinct required criteria) and `tests/acceptance-contract.test.mjs` (evidence cases).
Tests are not included in installed bundles.

## Shapes and limits

A context contains canonical relative `scope` paths, `sources`, `constraints`,
`checks`, `questions` and `coverage` (`complete`, `incomplete`, `unknown`). Each source
has an ID, kind, reference, content/version identity (or explicit null), UTC observation
time (`YYYY-MM-DDTHH:MM:SS[.mmm]Z`, a real calendar date), scope, provenance (`declared`, `observed`, `inferred`) and decision status
(`accepted`, `proposed`, `superseded`, `unknown`). Full source text stays outside the
packet. References and check descriptions are data; validation executes nothing.

A constraint carries its ID, description, source IDs, authority (`request`, `project`,
`observation`), status, scope, required flag and conflicting constraint IDs. Conflicts
are explicit interpretations; the engine does not discover contradictions in prose.
An engineering contract contains intent, exclusions, context, criteria, decisions and
attributable derivation. Each criterion carries an ID, description, required flag,
scope, source IDs, check IDs and expected evidence kind (`check`, `manual`). At least
one source supplies its basis. Decisions retain blocking/resolved flags.

Contexts and contracts each permit at most 32 KiB of UTF-8 JSON, including nested
context in a contract. Collections permit 32 entries, including sources, constraints
and criteria. Evidence permits 64 records within a separate 32 KiB envelope. IDs are
at most 128 characters, descriptions 512, references/artifacts 1024. Text is nonempty
and excludes control characters. Duplicate IDs, dangling source/conflict references,
unknown fields and overflow fail explicitly; nothing is silently truncated or evicted.
The existing 128 KiB session budget is unchanged; these limits do not establish that
every maximum-sized payload can be persisted together.

An evidence envelope contains `records`. Each record has an ID, criterion IDs, scope,
contract digest, `binding: {kind, identity}`, check/probe ID, evidence kind, artifact
reference, observation time, reporter, expected basis, support and execution result.
Binding kind is `repository` or `environment`; identity is an explicitly supplied
snapshot/build-and-environment identity or null. Support is `verified`, `observed`,
`inferred`, `unknown`; execution is `passed`, `failed`, `not-run`, `not-available`.
Freshness is computed rather than accepted from the evidence producer.

## Coverage and its limits

Different contract digests or known source/environment identities are stale. Missing
identity is unknown; a matching string with a different binding kind is stale.
Changing source identities in the context changes the contract digest. Collectors
must reobserve referenced sources and preserve the identity actually checked; these
pure operations cannot detect a file edit or unavailable artifact themselves.

A criterion requires current evidence naming its ID, one of its declared check IDs,
its expected evidence kind and all its exact scopes. It needs verified support and
passed execution. Any applicable failure prevents a passing record from hiding it.
An empty check list cannot be verified. Optional gaps remain visible. Completion also
requires no unresolved required constraint conflicts/unknown status, no blocking open
decisions and no supplied attempt blockers. Superseded constraints are historical.
Resolving a failure requires an explicitly amended evidence ledger; this operation
does not silently choose the latest pass or implement a persistence/recovery policy.

`assurance: attributed` means coverage follows the supplied reporter's assessments;
it does not independently validate assertions, artifact availability or semantic truth.
Manual assessment requires the same attributable basis and explicit environment binding.
No-contract projection returns `assurance: declared`, unknown coverage and no verified
completion. None of these outputs grant authorization or execute a check.

Errors retain the engine's `INVALID_INPUT` code. Payload diagnostics use the prefix
`acceptance:` and reasons `invalid-fields`, `invalid-text`, `invalid-value`, `capacity`,
`duplicate-id`, `invalid-scope`, `missing-scope`, `invalid-time`, `invalid-digest`,
`unsupported-version`, `unknown-reference`, `missing-basis`, `invalid-conflict`,
`missing-criteria`. Generic JSON/path validation can reject inputs before these checks.

## QA projection

`qa-projection` uses the same contract, criterion IDs, evidence and freshness calculation.
Call it through `evaluate({formatVersion: 1, operation: 'qa-projection', input})` on the
public engine boundary. It executes nothing and creates no state or new session format.
Non-Git QA supplies `current: {kind: 'environment', identity: BUILD_AND_ENVIRONMENT}`;
include configuration/test-data version when relevant. Null identity cannot verify freshness.
This explicit environment path does not change the task CLI's repository binding policy.

`probes` is a transient array of up to 32 entries within 32 KiB. Each entry has `id`
(the check/probe ID), `criterionIds`, `risk`, `flows`, `preconditions`, `actions`,
`expectedBasis` and `evidenceIds`. Collections contain at most 32 unique strings; strings
are nonempty, control-free and at most 1024 characters. Preconditions and evidence IDs
may be empty. Every linked record must name that probe and only its declared criteria.
Observations, test-data detail and reproduction artifacts live at the existing ledger's
artifact references. The engine cannot inspect or assess them. It rejects dangling links,
unknown criteria, duplicate probes, extra fields and oversized detail.

The result retains intent, exclusions, unanswered questions, declared checks, criteria,
probes and the underlying `coverage`. A probe without records is `not-run`; a probe with
records is `recorded`, which is not a pass. Inspect each record's execution, support and
freshness. `unexploredCriterionIds` names criteria without a current executed linked probe;
explored does not mean verified. Check descriptions are declarations, not proof that an
automation suite asserts the criterion.

Each criterion also exposes `conflict` when current evidence contains both passed and
failed results, including disagreement between automation and manual QA. This is a
conservative disagreement signal, even if one check is inadequate for acceptance. The
original coverage rules still require the declared kind/check/scope; the QA view neither
upgrades weaker evidence nor hides a failure. Both reporters and artifacts remain visible.
Stale or unknown records stay in the view without becoming a current disagreement.

The source checkout contains executable fixtures and a requirements-only charter at
`examples/evals/manual-qa/README.md` (excluded from installed bundles). Deterministic checks
establish mechanics; usefulness and factual accuracy require a separate human assessment.

## Product and architecture projections

Both operations read the same contract and ledger through the public `evaluate` boundary.
They retain identical source, criterion, decision and evidence IDs and use the existing
coverage/freshness calculation. No role state, discovery, execution or publication occurs.
Product readers receive `deliveryGaps` and `unresolvedDecisions`; architects receive
`evidenceGaps` and `adrSources`, including proposed and superseded status. Neither a source
status nor a successful check resolves a product choice or accepts an ADR.

Optional `focus` selects existing constraint IDs in `boundaries`, `dependencies` and
`consequences` arrays (at most 32 unique IDs each). Unknown fields, dangling IDs and duplicate
IDs fail. These are reader selections, not new facts. Each selected constraint includes its
cited `sources` with their original provenance and status: declared dependencies, observed
coupling and inference remain distinguishable, even when one constraint cites several kinds.
The engine does not classify prose, infer dependencies or discover operational consequences.
Unselected constraints remain visible in `constraints`; selection does not change coverage.

`discoveryCoverage` retains incomplete/unknown discovery separately from criterion coverage.
Empty dependencies or ADR sources mean no supplied facts, not absence in the project.
Questions, explicit constraint conflicts and unresolved decisions remain visible with no
code or runtime access. Existing coverage does not judge source completeness or truth;
readers must assess those limits before reporting readiness or architectural confidence.
Guidance-only installations carry these same distinctions manually in the existing packet.

The synthetic example at `examples/evals/product-views/README.md` demonstrates the two
reader questions and their common factual basis. It is not real product validation or
model-quality evidence.

## Compatibility boundary

| Existing reader/writer | Behavior to preserve |
|---|---|
| `engine/session/task.mjs` | Strict start/step/binding inputs; existing scope digest and declared receipts unchanged |
| `engine/session/activity.mjs` | Version-1 session, bounded 24-attempt history; strict receipt fields |
| `engine/checkpoint.mjs` | Strict metadata fields; existing version-1 checkpoint and continuation source binding |
| `application/session/{task,activity,status}.mjs` | Locked updates, checkpoint orchestration and task projection through the public engine |
| `application/{checkpoint,discovery/discover,workspace}.mjs` | Consume existing bindings, snapshots and selected repositories |
| `engine/session/{status,recommendations,features}.mjs` | Recovery, action and operational feature facts retain their separate meanings |
| CLI and host entry points | Continue through the existing application task/session operations |

Legacy task/attempt/checkpoint fixtures retain their declared semantics and original
scope digest. Opting into a structured contract upgrades that session/task state to
`formatVersion: 2`; current readers accept versions 1 and 2, and validate the nested
contract and evidence in version 2. Old version-1 validators reject version 2 before
writing. Stop incompatible writers before upgrading; do not downgrade a structured
state file or remove its contract to regain legacy completion. Checkpoint format stays
version 1: its existing task-contract binding now includes the engineering-contract
digest. Evidence remains in the locked session state rather than duplicated in a checkpoint.

`checkpoint-read` and supported session-start/after-compaction recovery reload the
active task's current coverage as `taskRecovery`. Its task/contract identity and pending
criteria come from session state, with freshness recomputed against current sources.
`checkpointTaskMatches` reports whether the historical checkpoint names that task and
contract; a matching repository snapshot alone cannot certify acceptance. Missing or
unreadable task state returns `taskRecovery.status: unavailable` while retaining the
historical checkpoint for inspection. Recovery neither rewrites evidence nor grants
authorization. These task details are returned to the caller, never telemetry attributes.

## Task command persistence

Use existing commands with an actual host session ID and private JSON input files:

```sh
node cli/main.mjs task-start --root PROJECT --session SESSION --task TASK --goal "Outcome" --contract contract.json --json
node cli/main.mjs session --root PROJECT --session SESSION --json
node cli/main.mjs task-step --root PROJECT --session SESSION --stage verify --target acceptance:criterion --approach assessed-check --evidence retained-output --outcome passed --result "Observed behavior" --next "Report remaining gaps" --acceptance-evidence ledger.json --expected-task-contract TASK_DIGEST --expected-evidence-digest LEDGER_DIGEST --complete --json
```

`--contract` reads the engineering-contract shape above. `--acceptance-evidence` reads
the complete versioned evidence envelope. Use `taskContract`, `evidenceDigest` and
`binding` returned by start/session; evidence records name the engineering-contract
digest separately. Preserve the binding actually checked: copying a new source identity
onto old evidence does not verify it. Non-Git roots return null repository identity and
cannot obtain verified completion through this developer path. Explicit environment
coverage remains available through the pure operation for QA producers.

Ledger replacement is explicit and conditional on both expected digests under the
existing session lock. Reload after a conflict; reconcile the records rather than retry
an old replacement with new digests. An assessment correcting a failure must retain its
basis in the revised ledger/retained artifact. No last-pass-wins rule hides a failure.
Evidence persists independently of the 24-entry attempt history and is not evicted.
The 32 KiB ledger, 32 KiB contract and combined 128 KiB session limits all apply; an
oversized write fails atomically, preserving the prior state. Busy locks and unavailable
storage fail the command. Checkpoint/workspace failures remain separately reported.

Contract, scope, risk, repository selection and checkout amendments clear structured
coverage and completion. Source edits make retained evidence stale; evidence with a
null source remains unknown. Stage changes through `task-start` also reset evidence;
normal `task-step` progression preserves it. Evidence from another contract/source is
never rebound automatically. Required failures, gaps, conflicts, open decisions or
attempt blockers reject `--complete`; optional gaps remain visible. Record partial
coverage without `--complete` before resolving a gap. A rejected completion does not
persist its proposed ledger or receipt.

Artifact references and `verified` support are attributable reporter assessments, not
automatic file retrieval or semantic checking. The reporter must establish artifact
availability and adequacy before declaring support; unavailable output uses
`execution: not-available`, unexecuted work `not-run`, and uncertain support `unknown`.
Reobserve cited instructions/configuration and authorized external sources after changes;
external freshness cannot be independently established by a repository fingerprint.
No source prose, criterion descriptions or artifact contents enter operational telemetry.
Direct commands require no hooks; guidance-only installations keep this same discipline
manually without claiming runtime persistence or enforcement.

## Grounded context composition

`application/session/grounding.mjs` exposes the opt-in, read-only
`groundedContext(root, input, { preferencesFile })` use case. `input.scope` contains
1–32 project-relative paths. Optional `sources`, `constraints`, `checks` and `questions`
use the context shapes above; `discovery` may carry an existing bounded lookup result.
The public engine operations are `grounding-request` and `task-context-compose`.
Task command persistence is separate; composition does not change task state.

The application hashes root and ancestor `AGENTS.md` files, the project configuration
and explicitly cited local sources, checking observations again after configuration
loading. It reuses routing's module selection for declared ownership, dependencies and
check commands. Nested instructions retain their directory scope. Files are bounded
regular files; missing explicit citations, symlinks and oversized reads remain visible.
No instruction text or declared command is executed. Module checks remain declarations,
not passing evidence. A source hash identifies bytes, not acceptance of its contents.

Supply interpreted constraints with their citations and explicit status. Conflicts
survive composition; unrelated module declarations and source interpretations are
excluded. Request sources and authorized external observations retain caller-declared provenance without
connecting a service or independently verifying their identity. Existing lookup output
is returned unchanged without another search; truncation/skips make coverage incomplete,
and literal matches never become constraints automatically.

Composition never certifies complete semantic coverage: it returns `unknown` or
`incomplete`. Changed/deleted citations lose identity and accepted status. A non-Git
root has a null repository binding even when file hashes are available. These observations
are a point-in-time packet; callers must recompose after source/configuration changes.
Raw lookup results remain outside the 32 KiB validated context and must retain their
original bounded discovery contract. No CLI writer or automatic completion behavior is
introduced by this API.
