# Engineering concepts and specialization

Shared guidance holds engineering decisions that apply across languages. Language/runtime
references hold concrete syntax, compiler behavior and runtime mechanics. Framework/provider
guidance applies only when that framework/provider is present. A language example does not
make its implementation pattern a universal rule.

| Concept | Shared guidance |
|---|---|
| Design, responsibility, abstraction, coupling and reuse | [Design principles](../plugins/craft-engineering/skills/foundations/references/design-principles.md) |
| Concurrency, synchronization, cancellation and duplicate delivery | [Concurrency](../plugins/craft-engineering/skills/backend-engineering/references/async-patterns.md) |
| Failure contracts, deadlines, retries and unknown outcomes | [Error handling](../plugins/craft-engineering/skills/backend-engineering/references/error-handling.md) |
| Consistency, conflicting writes and cross-resource recovery | [Distributed systems](../plugins/craft-engineering/skills/backend-engineering/references/distributed-systems.md) |
| Signals, correlation, sensitive data and audit semantics | [Observability](../plugins/craft-engineering/skills/backend-engineering/references/observability-logging.md) |
| Desired state, resource lifecycle, rollout and recovery | [Infrastructure](../plugins/craft-engineering/skills/foundations/references/infrastructure.md) |
| Pipeline validation, release gates and automation recovery | [Infrastructure engineering](../plugins/craft-engineering/skills/infrastructure-engineering/SKILL.md), with shared [CI/CD](../plugins/craft-engineering/skills/infrastructure-engineering/references/ci-cd.md) and conditional [GitHub Actions](../plugins/craft-engineering/skills/infrastructure-engineering/references/github-actions.md) detail |
| Contracts, dependency direction, ownership and resource lifetimes | [Service boundaries](../plugins/craft-engineering/skills/backend-engineering/references/service-layers.md) |
| Capacity, backpressure, caching and performance evidence | [Performance](../plugins/craft-engineering/skills/backend-engineering/references/performance-efficiency.md) |
| Context reuse, prompt/tool caches and logical model profiles | [Execution optimization](../plugins/craft-engineering/skills/foundations/references/execution-optimization.md) |
| Meaningful verification, test reliability and investigation | [Testing](../plugins/craft-engineering/skills/testing-engineering/SKILL.md) |
| Trust boundaries, authorization and safe interpretation | [Security](../plugins/craft-engineering/skills/security-engineering/SKILL.md) |

Load only the concept relevant to the changed behavior, then the affected
[language/runtime reference](../plugins/craft-engineering/skills/foundations/SKILL.md#choose-detail).
Shared examples state assumptions, the protected invariant and observable outcomes. Use
labeled pseudocode for algorithms and ordered timelines for rollout or crash behavior.
Concrete examples belong in the matching discipline and explain what they do and do not prove.
Consumer architecture, access policy and operational targets remain the grounding source.
