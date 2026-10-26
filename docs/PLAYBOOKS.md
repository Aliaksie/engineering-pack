# Portable playbook catalog

`engine/routing/catalog.json` owns engineering selection rules; `craft` embeds it. Exports
and host adapters neither select practices nor infer a stack. Package skill IDs identify
loadable content.

Logical identities use `<kind>:<slug>` with lowercase hyphenated slugs. Supported kinds are
`workflow`, `practice`, `language`, `runtime`, `framework`, `provider`, and `layer`.
They describe behavior independently of installation package boundaries.

Each catalog entry has an ID, skill/reference load targets, `when` selectors and a `value`
record covering problem, contribution, inputs, result, boundaries, consumer and required evidence.
Selectors are the trigger. `contextOnly: true` explicitly marks an identity used for further
selection without independently loadable guidance. These authoring records justify inclusion;
they are not model evaluation results and are omitted from route output to limit context.

The published contract is `schemas/playbook-catalog.schema.json`. `craft` validates the
embedded catalog on use: shape, logical IDs, selector dimensions/values, duplicate identities,
intent boundaries, value fields, target syntax and runtime implications. `npm run validate`
also resolves every target against registered package skills and existing source files.
Missing optional packages at installation time are still reported through host availability
rather than treated as permissions.

Selectors are AND across dimensions and OR within a dimension. Only workflow/practice entries with
explicit intent selectors run outside `stackIntents`. Stack matches are evaluated per affected
module so unrelated module evidence cannot combine. Empty module arrays override project
defaults; absent fields inherit project defaults. Consumers should explicitly declare module
stack dimensions in mixed repositories.

`when.paths` selects by canonical project-relative file or directory scope: an exact file
or a descendant across a directory boundary matches. It has no glob syntax. Matching uses
the affected paths within each module and records that path evidence; a matching path
cannot borrow a provider or framework from another module. Paths select guidance to
verify against the repository, not proof of a configured service or permission to execute it.

`skills` uses existing `package:skill` IDs; `references` uses `package:skill/references/file.md`.
An identity can report `unavailable` rather than inventing guidance. A
`documentationQuestion` describes a question to resolve only if the changed behavior needs it,
using the pinned dependency version and official documentation. Routing does not fetch it.

`fallbackSkills` maps a primary skill to an explicit alternative. The router selects it only
when the host supplies an availability list, the primary is absent and the alternative is
installed. The selection records the substitution and its reason. Without that evidence,
the primary remains selected and missing targets stay visible. Review can therefore use
`craft-delivery:review-code` when `craft-review:review-change` is unavailable, without
claiming the fallback has committed-snapshot tooling or granting publication authority.

The `loading` field contains:

- `workflow`, `practices`, `specializations`: logical selections.
- `selections`: targets, matching selectors, evidence and limitations.
- `evidence`: declared source or explicitly marked inference.
- `unavailable`: selected identities without standalone guidance.
- `documentationQuestions`: conditional external evidence needs.
- `missingContext`: unresolved affected scope.

The `languages` output recognizes JVM declarations. Precise Java/Kotlin
identities appear under `loading`. Configure `runtimes` independently at project/module level;
TypeScript and backend layers do not infer Node. Java/Kotlin imply JVM but never Spring.
Framework and agent-provider specializations require explicit declarations. Agent providers
additionally require an agent layer. CI/CD references use their own task/path selectors.
Story authoring loads no implementation specializations or provider references.

The `locate` intent selects only `craft-engineering:locate-code`; execution stays in tools
and declared stack or risk tags do not load engineering disciplines for lookup. Planning
selects the focused design-principles reference alongside its workflow, making boundary and
abstraction criteria available without loading the full foundations index.

For example, a consumer may declare:

```json
{
  "formatVersion": 1,
  "modules": [{
    "name": "worker",
    "path": "worker",
    "languages": ["typescript"],
    "runtimes": ["node"],
    "layers": ["agent"],
    "providers": ["bedrock"]
  }]
}
```

Save that configuration as `<project>/.craft/project.json`. From this source checkout,
run `npm run route -- --root <project> --intent implement --file worker/main.ts`, or run an
exported package's `bin/craft route` with `--root` pointing to the same consumer project.

A playbook must justify its trigger, problem, contribution, inputs, result, boundaries,
consumer, and evidence. Keep the skill entry concise: decisions, evidence to collect,
failure modes, verification and links to conditionally loaded references. Repository behavior,
accepted policy, external documented behavior and recommendations are separate evidence
classes. Surface contradictions rather than treating existing code as policy.

Risk tags select deeper references independently of stack: `debugging`/`regression` load the
investigation procedure; `flaky-test`/`test-data` load reliability guidance. They do not claim
a defect exists or authorize extra agents, remote writes or cleanup.

Shared practices are selected through risk evidence (`concurrency`/`queue`/`idempotency`,
`observability`/`logging`/`audit-trail`, `service-boundary`/`resource-lifetime`, and
`performance`/`cache`/`resource-budget`). These practices apply independently of Node;
a Node declaration alone does not load them. The backend-engineering index is also available
for explicit invocation or configured module skills. `distributed-system`/`consistency`/
`replication`/`partition` select distributed-state guidance; `infrastructure`/`deployment`/
`rollout`/`disaster-recovery` select infrastructure lifecycle guidance. These concepts do not
require a particular language, backend framework or cloud. See the
[shared engineering concepts](#shared-engineering-concepts).

`ci-cd`/`pipeline`/`build-pipeline`/`release-pipeline` select infrastructure engineering and
its shared CI/CD reference. Affected `.github/workflows` paths or explicit `github-actions`
risk additionally select the GitHub Actions reference. A GitHub remote or codehost binding
alone selects neither. Other providers use the shared guidance and their actual tooling;
specialization belongs in references under the same skill, not a skill per Git vendor.

Guidance uses consumer policy and available evidence. UI/security/TypeScript references
do not prescribe a universal styling, state or test stack, or claim conformance and safety
from a pattern alone. No logical identity requires a separate physical package.

JavaScript, TypeScript, Python, JVM, Go and C# guidance lives in selectively loaded
foundations references. Select those references by the actual language and task, independently
of runtime, application layer and provider. Use the [paired evaluation protocol](EVALUATION.md)
when measuring the effect of guidance on model outcomes.

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
