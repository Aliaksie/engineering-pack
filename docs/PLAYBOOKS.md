# Portable playbook catalog

`engine/routing/catalog.json` owns engineering selection rules. `tooling/export/` bundles
the canonical runtime; `hosts/` translates native formats. Neither selects practices or
infers a stack. Package skill IDs identify loadable content.
Versions remain `0.0.1`.

Logical identities use `<kind>:<slug>` with lowercase hyphenated slugs. Supported kinds are
`workflow`, `practice`, `language`, `runtime`, `framework`, `provider`, and `layer`.
They describe behavior independently of installation package boundaries.

Each catalog entry has an ID, skill/reference load targets, `when` selectors and a `value`
record covering problem, contribution, inputs, result, boundaries, consumer and required evidence.
Selectors are the trigger. `contextOnly: true` explicitly marks an identity used for further
selection without independently loadable guidance. These authoring records justify inclusion;
they are not model evaluation results and are omitted from route output to limit context.

The published contract is `schemas/playbook-catalog.schema.json`. The dependency-free runtime
validator checks shape, logical IDs, selector dimensions/values, duplicate identities,
intent boundaries, value fields, target syntax and runtime implications. `npm run validate`
also resolves every target against registered package skills and existing source files.
Builds validate catalog shape before replacing generated output. Missing optional packages at
installation time are still reported through host availability rather than treated as permissions.

Selectors are AND across dimensions and OR within a dimension. Only workflow/practice entries with
explicit intent selectors run outside `stackIntents`. Stack matches are evaluated per affected
module so unrelated module evidence cannot combine. Empty module arrays override project
defaults; absent fields inherit project defaults. Consumers should explicitly declare module
stack dimensions in mixed repositories.

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
Frameworks and providers require explicit declarations. Providers additionally require an agent
layer. Story authoring loads no implementation specializations or provider references.

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

Save that configuration as `<project>/.capabilities/project.json`. From this source checkout,
run `npm run route -- --root <project> --intent implement --file worker/main.ts`, or invoke
the exported `cli/main.mjs` with `--root` pointing to the same consumer project.

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
[engineering guidance map](ENGINEERING-GUIDANCE.md).

Guidance uses consumer policy and available evidence. UI/security/TypeScript references
do not prescribe a universal styling, state or test stack, or claim conformance and safety
from a pattern alone. No logical identity requires a separate physical package.

JavaScript, TypeScript, Python, JVM, Go and C# guidance lives in selectively loaded
foundations references. Select those references by the actual language and task, independently
of runtime, application layer and provider. Use the [paired evaluation protocol](EVALUATION.md)
when measuring the effect of guidance on model outcomes.
