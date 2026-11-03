# Craft Stories

Code grounding, specification audits and local user-story drafts.

Start with `workflow-prepare-stories` when you have a feature or product outcome to turn
into reviewable stories. It composes three methods:

| Skill | Input and result |
|---|---|
| `ground-spec` | Explicit source locations and product claims → cited implementation evidence and access limits |
| `audit-specs` | A specification and available evidence → contradictions, gaps and consequential questions |
| `write-stories` | A feature and confirmed requirements → outcome-based stories with observable acceptance criteria |

Supply the feature text or file and identify source repositories only when implementation
grounding is needed. The workflow keeps assumptions and proposed solution choices separate
from confirmed requirements. It returns local drafts and unresolved questions; tracker access
and implementation guidance are optional. It never assumes a particular tracker or design system.

The canonical contract is [capability.json](source-capability.json); [AGENTS.md](AGENTS.md)
gives shared instructions. Build platform exports with `npm run build` from the source checkout's
root. See [adapter support](https://github.com/Aliaksie/engineering-pack/blob/distribution-next/codex/docs/ADAPTERS.md) and [package format](https://github.com/Aliaksie/engineering-pack/blob/distribution-next/codex/docs/PACKAGE-FORMAT.md).
