# Engineering concepts and specialization

Shared guidance holds engineering decisions that apply across languages. Language/runtime
references hold concrete syntax, compiler behavior and runtime mechanics. Framework/provider
guidance applies only when that framework/provider is present. A language example does not
make its implementation pattern a universal rule.

| Concept | Shared guidance |
|---|---|
| Design, responsibility, abstraction, coupling and reuse | [Design principles](../packages/craft-engineering/skills/craft-engineering-foundations/references/design-principles.md) |
| Concurrency, synchronization, cancellation and duplicate delivery | [Concurrency](../packages/craft-engineering/skills/craft-engineering-backend-engineering/references/async-patterns.md) |
| Failure contracts, deadlines, retries and unknown outcomes | [Error handling](../packages/craft-engineering/skills/craft-engineering-backend-engineering/references/error-handling.md) |
| Consistency, conflicting writes and cross-resource recovery | [Distributed systems](../packages/craft-engineering/skills/craft-engineering-backend-engineering/references/distributed-systems.md) |
| Signals, correlation, sensitive data and audit semantics | [Observability](../packages/craft-engineering/skills/craft-engineering-backend-engineering/references/observability-logging.md) |
| Desired state, resource lifecycle, rollout and recovery | [Infrastructure](../packages/craft-engineering/skills/craft-engineering-foundations/references/infrastructure.md) |
| Contracts, dependency direction, ownership and resource lifetimes | [Service boundaries](../packages/craft-engineering/skills/craft-engineering-backend-engineering/references/service-layers.md) |
| Capacity, backpressure, caching and performance evidence | [Performance](../packages/craft-engineering/skills/craft-engineering-backend-engineering/references/performance-efficiency.md) |
| Context reuse, prompt/tool caches and logical model profiles | [Execution optimization](../packages/craft-engineering/skills/craft-engineering-foundations/references/execution-optimization.md) |
| Meaningful verification, test reliability and investigation | [Testing](../packages/craft-engineering/skills/craft-engineering-testing-engineering/SKILL.md) |
| Trust boundaries, authorization and safe interpretation | [Security](../packages/craft-engineering/skills/craft-engineering-security-engineering/SKILL.md) |

Load only the concept relevant to the changed behavior, then the affected
[language/runtime reference](../packages/craft-engineering/skills/craft-engineering-foundations/SKILL.md#choose-detail).
Shared examples state assumptions, the protected invariant and observable outcomes. Use
labeled pseudocode for algorithms and ordered timelines for rollout or crash behavior.
Concrete examples belong in the matching discipline and explain what they do and do not prove.
Consumer architecture, access policy and operational targets remain the grounding source.
