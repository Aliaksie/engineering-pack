# Portable acceptance contracts

The engine exposes opt-in JSON validation and coverage operations. These do not yet
write session state, collect sources or change task command completion. Existing tasks
remain usable with agent-declared completion and unassessed criterion coverage.

| Operation | Input | Result |
|---|---|---|
| `task-context-validate` | `context` | Validated task context |
| `engineering-contract-validate` | `contract` | Validated engineering contract |
| `engineering-contract-digest` | `contract` | SHA-256 of recursively key-sorted JSON |
| `acceptance-evidence-validate` | `evidence` | Validated evidence ledger |
| `acceptance-coverage` | `contract`, `evidence`, `current`, optional boolean `blockers` | Applicability and per-criterion coverage |

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
time, scope, provenance (`declared`, `observed`, `inferred`) and decision status
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

Legacy task/attempt/checkpoint input and completed-state fixtures freeze these paths.
Strict legacy command and metadata readers reject new structured fields. Existing
session validation is permissive about unknown task fields and checkpoint validation
does not deeply validate arbitrary additions: acceptance by an old reader does not
prove preservation by its writer. No structured fields are persisted in this increment.
Before enabling writers, introduce explicit compatible state validation and mixed-version
write rejection; never assume old clients preserve new evidence or convert an old
passing receipt into verified criteria. State storage remains behind platform ports.

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
