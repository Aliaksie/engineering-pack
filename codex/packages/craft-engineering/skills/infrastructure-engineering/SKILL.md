---
name: infrastructure-engineering
description: "Design and verify infrastructure changes, including CI/CD pipelines, using the consumer's environment and execution boundaries."
---

<!-- Generated from packages/; edit the portable source and rebuild. -->

Read the shared [package instructions](../../AGENTS.md) when using this capability.

# Infrastructure engineering

Identify the affected environment, configuration owner and intended operational outcome.
Separate validation and planning from actions that change resources, publish artifacts or
deploy software. Use the consumer's existing tools and permissions; opening this skill
grants no authority to run a release or change a remote system.

Read only the references needed for the affected behavior:

- [CI/CD](references/ci-cd.md): Pipeline validation, meaningful tests, release gates,
  reproducible inputs and failure recovery across providers.
- [GitHub Actions](references/github-actions.md): Read alongside CI/CD when affected
  workflow files or explicit task evidence establish GitHub Actions. A GitHub remote
  alone does not establish the pipeline provider.
- [Resource lifecycle](../foundations/references/infrastructure.md): Desired state,
  replacement, rollout compatibility and recovery when actual infrastructure changes.

For another pipeline provider, retain the shared CI/CD principles and consult that
provider's official documentation for the configured version. Do not transplant GitHub
syntax, permissions or event semantics. Add another reference only when concrete work
needs maintained specialization; do not create a separate skill per Git vendor.

Return the scoped change or decision, checks actually run and remaining operational
limits. Distinguish local validation, observed hosted runs and unexecuted deployment.
