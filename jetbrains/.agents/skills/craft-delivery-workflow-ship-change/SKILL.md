---
name: craft-delivery-workflow-ship-change
description: "Execute selected release, installation or repository actions against explicit consumer targets."
---

<!-- Generated from packages/; edit the portable source and rebuild. -->

Read the shared [package instructions](../../../packages/craft-delivery/AGENTS.md) when using this capability.

# Complete the requested shipping actions

Select only the actions in the user's scope. Installation, committing, pushing, deployment
and registry publication can be independent; one does not imply permission for the others.
Read the consumer's established commands, destinations and release rules before preparing an
invocation. Identify the source revision, artifact, target account/repository/environment and
verification that will establish success.

Prepare a concrete result before requesting any genuinely missing approval. Reuse authorization
already given. An ambiguous target or unspecified license decision blocks its dependent action,
not all preparation. Keep credentials in the consumer's supported mechanisms and prefer its
existing tooling over inventing a deployment or packaging system.

Apply the relevant action checks:

- **License selection:** preserve the established choice and inspect redistribution obligations.
  If a responsible person's license decision is missing, present that decision without choosing
  one on their behalf. Keep package metadata and distributed notices consistent.
- **Commit:** inspect the working tree and staged diff, stage only the intended scope, preserve
  unrelated staged work, and run the required checks before the authorized commit.
  Aim for a coherent completed task with its tests/docs; split independently reviewable
  outcomes where useful rather than making a commit per file or worker. Follow the consumer's
  message policy, using Conventional Commits when no policy exists. Review integrated worker
  changes before staging, and preserve another worker's branch/worktree and unfinished work.
- **Push:** verify the remote, ref and included commits against the intended destination. Do not
  infer force-push or history replacement permission. Record the resulting remote revision.
- **Install:** inspect the consumer's existing setup and use the supported installer without
  overwriting configuration or user data. Verify discovery and a meaningful operation, and
  identify removal or recovery steps.
- **Deploy:** use the selected artifact and environment through the consumer's rollout process.
  Observe relevant health and behavior checks, then follow the agreed failure procedure.
  A code rollback may not undo stored-data changes or external effects.
- **Publish:** check version, changelog and the actual packaged inventory, including notices and
  secrets exclusion. Publish only the authorized artifact/version, then verify its identity and
  availability. Replacement, deletion and announcements need their own applicable authorization.

Report what executed, its resulting identifier and the verification outcome. Distinguish a
prepared command, a submitted action and a verified result; successful submission alone does
not establish a usable installation or healthy deployment.

Package tools and shared assets: `../../../packages/craft-delivery` relative to this skill directory.
