# Naming

The repository and generated marketplace are `craft-capabilities` (Craft Capabilities).
Installation packages use `craft-<domain>`:

| Package | Domain |
|---|---|
| `craft-engineering` | Shared engineering concepts and concrete disciplines |
| `craft-delivery` | Planning, implementation, service setup and shipping |
| `craft-review` | Scoped code review and revision-bound artifacts |
| `craft-stories` | Grounded specifications and readable user stories |

Use lowercase kebab-case for public identifiers, files and directories. Procedure skills use
clear action names, such as `plan-change`, `inspect-architecture`, `write-adr`, `resolve-service`
and `maintain-context`. Discipline indexes use descriptive names such as `testing-engineering`
or `backend-foundations`. References name the concept they explain. Avoid adding a redundant
`craft-` prefix inside a package.

Canonical citations use `package:skill`. Logical catalog IDs use `kind:topic` independently
of file placement. Adapters supply native package prefixes where a host needs them; authoring
content must not maintain separate host-specific name copies.

Workflows use verb-based names such as `deliver-change`, `ship-change` and `prepare-stories`.
Their generated skill names are adapter-owned. Internal JavaScript symbols
follow existing code conventions. Consumer identities, service operation names and vendor
event names retain their actual contracts rather than being renamed for branding.

Top-level directories name ownership rather than generic adapters or scripts. Follow the
[architecture map](ARCHITECTURE.md#ownership). Within each layer, group related responsibilities
by feature at shallow depth; keep a single small responsibility as a file until a folder
has useful components. Internal plans and local evaluation artifacts stay ignored.

Consumer configuration lives in `.craft/project.json` and `.craft/telemetry.json`.
Git worktrees and non-Git coordination roots both keep generated operational data in
`.craft/state/<worktree-id>/<session-id>/`, with a reserved `shared/` scope for project-wide
records. Ignore `.craft/state/` in Git. See [session storage and migration](SESSION.md)
for identities and legacy-copy behavior. To upgrade configuration, move `.capabilities/`
configuration to `.craft/`; runtime migration never overwrites consumer configuration.
Old `.capabilities/state/` remains excluded from source lookup, review and exports.
