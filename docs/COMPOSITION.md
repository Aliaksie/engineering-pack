# Capability composition

A package groups content that has a useful shared consumer, ownership and installation
lifecycle. A skill is an independently useful procedure; a reference holds selectively
loaded detail. Logical practices and specializations do not each require a package or skill.

When a collection needs independent installation or ownership, extract it into a self-contained
package under `packages/`. Give it a manifest, instructions and only its own resources.
Keep one canonical source for shared runtime; the builder supplies an isolated runtime copy
to each export. Installed packages must not import authoring files or sibling packages.

Update catalog load targets, references, optional dependencies and documentation together.
Use `package:skill` identities so adapters can generate native names. Missing optional content
must remain visible with an appropriate direct/project-guidance fallback. A procedure should
pass scope, evidence, resulting state and unresolved decisions to the next procedure without
requiring an extra document for every handoff.

Run the source/build checks for an extraction and inspect the affected export paths. Verify
standalone imports and selective loading; a split should not force unrelated content to load.
Keep collections together when splitting adds installation coupling without a useful consumer
boundary. This is a supported composition technique, not a target package count.

## Decisions before implementation

`inspect-architecture` supplies a bounded map of applicable decisions and their status,
scope and constraints. `apply-change` reads those records and the relevant selected engineering
detail before changing the governed behavior. Reuse current evidence rather than loading all
ADRs or every reference. Resolve material policy conflicts through the consumer's decision
process; an absent decision record does not require writing one for routine work.

Decision retrieval starts with the consumer's index and batched literal lookup. An authorized
economical worker can find candidates by meaning when terminology differs; the main task reads
the candidate records and decides applicability. `write-adr` checks human readability and uses
the consumer's acceptance/supersession conventions. Hooks provide context where supported;
they do not prove that a model read a document or understood a decision.
