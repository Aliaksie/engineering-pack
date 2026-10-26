# Playbook catalog

`engine/routing/catalog.json` holds the selection rules; `craft` embeds and validates it, and
`npm run validate` resolves every target against registered skills and files. The schema
is [playbook-catalog.schema.json](../schemas/playbook-catalog.schema.json).

## Entries

Logical identities are `<kind>:<slug>` with kinds `workflow`, `practice`, `language`,
`runtime`, `framework`, `provider` and `layer`, independent of package boundaries. Each
entry has load targets (`skills` as `package:skill`, `references` as
`package:skill/references/file.md`), `when` selectors and a `value` record justifying it:
problem, contribution, inputs, result, boundaries, consumer and required evidence. The value
record is authoring rationale and is not returned by `route`. `contextOnly: true` marks an
identity used only for further selection. A `documentationQuestion` names something to check
in official documentation when the change needs it.

## Selection

- Selectors are AND across dimensions and OR within one. Stack selectors match per affected
  module, so evidence from different modules never combines.
- `when.paths` matches an exact file or a directory and its descendants; there are no globs.
- Stacks need declarations: Java/Kotlin imply JVM but never Spring; TypeScript does not imply
  Node; frameworks and agent providers must be declared (providers also need an agent layer).
- Risk tags pick shared practices independently of stack: `debugging`, `regression`,
  `flaky-test`, `test-data`, `concurrency`, `idempotency`, `observability`, `audit-trail`,
  `service-boundary`, `performance`, `cache`, `distributed-system`, `consistency`,
  `infrastructure`, `deployment`, `rollout`, `ci-cd`, `pipeline`. Affected `.github/workflows`
  paths or the `github-actions` risk add the GitHub Actions reference; a GitHub remote alone
  does not.
- Story authoring loads no implementation or provider guidance; `locate` loads only the
  lookup skill; planning adds the design-principles reference.
- `fallbackSkills` substitutes an alternative only when the host reports the primary missing
  and the alternative installed. For example, `craft-delivery:review-code` stands in for
  `craft-review:review-change`, without claiming snapshot tooling.

The `loading` output lists `workflow`, `practices`, `specializations`, per-selection
`selections` with their evidence, `unavailable` identities, `documentationQuestions` and
`missingContext`.

## Writing a playbook

Justify its trigger, problem, contribution, inputs, result, boundaries, consumer and
evidence. Keep the skill entry short (decisions, evidence to collect, failure modes,
verification) and link detail as conditionally loaded references. Keep repository
behavior, accepted policy, documented external behavior and recommendations apart, and
surface contradictions instead of treating existing code as policy. Language guidance
(JavaScript, TypeScript, Python, JVM, Go, C#) lives in foundations references, selected by
the actual language. Measure effects with the [evaluation protocol](EVALUATION.md).

## Shared engineering concepts

Shared guidance holds engineering decisions that apply across languages. Language/runtime
references hold concrete syntax, compiler behavior and runtime mechanics. Framework/provider
guidance applies only when that framework/provider is present. A language example does not
make its implementation pattern a universal rule.

| Concept | Shared guidance |
|---|---|
| Design, responsibility, abstraction, coupling and reuse | [Design principles](../packages/craft-engineering/skills/foundations/references/design-principles.md) |
| Concurrency, synchronization, cancellation and duplicate delivery | [Concurrency](../packages/craft-engineering/skills/backend-engineering/references/async-patterns.md) |
| Failure contracts, deadlines, retries and unknown outcomes | [Error handling](../packages/craft-engineering/skills/backend-engineering/references/error-handling.md) |
| Consistency, conflicting writes and cross-resource recovery | [Distributed systems](../packages/craft-engineering/skills/backend-engineering/references/distributed-systems.md) |
| Signals, correlation, sensitive data and audit semantics | [Observability](../packages/craft-engineering/skills/backend-engineering/references/observability-logging.md) |
| Desired state, resource lifecycle, rollout and recovery | [Infrastructure](../packages/craft-engineering/skills/foundations/references/infrastructure.md) |
| Pipeline validation, release gates and automation recovery | [Infrastructure engineering](../packages/craft-engineering/skills/infrastructure-engineering/SKILL.md), with shared [CI/CD](../packages/craft-engineering/skills/infrastructure-engineering/references/ci-cd.md) and conditional [GitHub Actions](../packages/craft-engineering/skills/infrastructure-engineering/references/github-actions.md) detail |
| Contracts, dependency direction, ownership and resource lifetimes | [Service boundaries](../packages/craft-engineering/skills/backend-engineering/references/service-layers.md) |
| Capacity, backpressure, caching and performance evidence | [Performance](../packages/craft-engineering/skills/backend-engineering/references/performance-efficiency.md) |
| Context reuse, prompt/tool caches and logical model profiles | [Execution optimization](../packages/craft-engineering/skills/foundations/references/execution-optimization.md) |
| Meaningful verification, test reliability and investigation | [Testing](../packages/craft-engineering/skills/testing-engineering/SKILL.md) |
| Trust boundaries, authorization and safe interpretation | [Security](../packages/craft-engineering/skills/security-engineering/SKILL.md) |

Load only the concept relevant to the changed behavior, then the affected
[language/runtime reference](../packages/craft-engineering/skills/foundations/SKILL.md#choose-detail).
Shared examples state assumptions, the protected invariant and observable outcomes. Use
labeled pseudocode for algorithms and ordered timelines for rollout or crash behavior.
Concrete examples belong in the matching discipline and explain what they do and do not prove.
Consumer architecture, access policy and operational targets remain the grounding source.
