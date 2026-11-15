# Security

Capability code runs with the host's permissions. Package metadata grants no approval,
installs nothing and sandboxes nothing: review tools, MCP declarations and hooks as code that
runs with a developer's access. Specifications, repository files, PR text, model responses and
retrieved content are untrusted data. Packages cannot authorize remote writes, deployments or
publication, and must not contain credentials, private endpoints or production transcripts.

## Trust boundaries

- Consumer configuration and bot runner policy are trusted; an untrusted PR or MR head must
  not replace them or grant itself publication rights.
- Invocation plans keep argv and structured arguments but neither execute nor authorize them.
  Read/write policy, tool availability, credentials and user or job authorization are separate
  checks. Publication requires matching base/head revisions and explicit authorization.
- `craft check` starts the commands the project configuration declares, without a shell and
  with the caller's permissions. It runs only when called, never from an agent host's hook,
  and nothing it reports authorizes a commit, push or publication. Run it on untrusted
  configuration only where running that project's own build would be acceptable. A Git
  pre-commit hook runs it only after the user wrote one with `craft check --install-hook
  --apply`. The CI job `--ci-snippet` prints runs the pull request's own declared checks with
  a read-only token and no persisted credentials, as building that pull request would.
- The organization and user configuration layers are files outside the repository. On POSIX
  they are read only when no other account can write them; Windows has no default
  organization file and checks no ownership, so the file named there must be writable by
  administrators only. Layers set defaults a project can override, so they are not a control
  over a repository's own configuration, and they hold no credentials. A check command a
  layer declares runs under `craft check` like the project's own. While a layer file cannot be
  used, a hook that guards shell commands refuses them, as with an unusable project file.
  A committed `.craft/organization.json` (written by `craft config --sync --apply`) is
  repository content, read like the project file and only where the machine has no
  organization file: whoever can change the project file can change it too.
- Building untrusted PR code can run arbitrary code even in a disposable checkout.
- For agent changes, test tool authorization, idempotency, isolation and output validation
  independently of model behavior; a prompt or content filter does not replace them.

## Read-only review guard

The optional review hook recognizes direct publishing commands (PR/MR and issue creation,
`api` calls whose flags imply a write), quoting, command separators, Git global options,
`-c` scripts of supported shells and active `$()`/backtick substitutions, and denies nesting
deeper than 16 levels. The text of a here-document is data, not commands, unless a supported
shell is named on the line that opens it; substitutions in it are read when its delimiter is
unquoted. It is a guardrail, not a security boundary: aliases, other
interpreters and executable paths, and variable expansion are outside it. Enforce with host
tool permissions, read-only credentials and isolation. `npm run hooks:report` measures it
against a labeled corpus ([contributing](CONTRIBUTING.md#hook-diagnostics)).

## Checks read from the project's files

While a project declares no standards, the lint, format and type-check commands its own
files state (`npm run lint`, `./gradlew spotlessCheck`, `npx tsc --noEmit` and the like) are
checks `craft check` runs when it is called. A tool that only comes with the toolchain
(`go vet`, `dotnet format`) is advised and not run. No hook starts them, but `craft check` in a
repository you do not trust runs that repository's scripts and build wrappers, as running its
build would. `craft standards --propose` lists them first, `craft check` names each command
it starts, and `routing.conventionProposals: false` in the user or organization file turns
detection off everywhere. The commit gate's refusal asks the agent to run `craft check` before
it commits, detected checks included; `routing.commitGate: false` stops that.

## Safety policy

Craft refuses destructive shell commands and reads of secret files before they run, on hosts
where a hook can refuse a tool call (Claude Code, Codex, Cursor, Copilot CLI, Gemini CLI). The policy is on with no
configuration. `safety` in `.craft/project.json`, or in an organization or user file, leaves
rules out, adds the project's own or turns the policy off with `"enabled": false`:

```json
"safety": {
  "disables": ["system-administration"],
  "deny": [{ "id": "infra-destroy", "command": ["terraform", "destroy"],
             "reason": "tearing down shared infrastructure" }]
}
```

| Rule | Refuses |
|---|---|
| `delete-outside-project` | `rm -r`, `find -delete` or `find -exec rm -r` whose target is outside the project root, under `~`, an unexpanded variable or substitution, or read from input as with `xargs rm -r`; a target strictly below a temporary directory (`$TMPDIR`, `/tmp`, `/var/tmp`) passes |
| `delete-project-root` | `rm -r` of the project root, of everything in it (`*`, `.`) or of its `.git`; `find` from the project root deleting with no name, path, time or size test |
| `force-push` | `git push` with `--force`, `-f`, `--mirror` or a `+refspec`; `--force-with-lease` passes |
| `delete-remote-branch` | `git push` with `--delete`, `-d`, `--prune` or an empty source (`:branch`) |
| `discard-changes` | `git reset --hard`, `git checkout`/`git restore` of the whole tree, `git checkout -f`, `git clean -f` (not a dry run), `git stash drop` and `clear` |
| `system-administration` | `sudo`, `doas`, shutdown and reboot, stopping or disabling services, mounting, formatting and writing to devices |
| `read-secret-file` | Reading a file that commonly holds secrets through any tool the hook sees: a file read, a search whose path or file filter names one (`.env*`, `*.key`), an edit of an existing one, or a shell command that prints, searches, copies or sends it (`cat`, `grep`, `base64`, `cp`, `curl -d @file`, `git show HEAD:.env`, `< .env`). Inside or outside the project, in any repository of a workspace. Claude Code asks the user, who sees why; other hosts refuse |

### Secret files

`read-secret-file` judges a file by its name and the directories above it: `.env` and
`.env.*`, `*.env`, `.npmrc`, `.pypirc`, `.netrc`, `.pgpass`, `.git-credentials`,
`.htpasswd`, `credentials` and `credentials.json`/`secrets.yaml` and similar data files,
private keys (`*.key`, `*.p12`, `*.pfx`, `*.jks`, `*.ppk`, a `.pem` named as a key such as
`privkey.pem`, `id_rsa`/`id_ed25519` and anything else in `~/.ssh` but public keys,
`known_hosts`, `config` and `authorized_keys`), `~/.gnupg`, Terraform state, and token files
of `gh`, `kubectl`, Docker, Azure and gcloud. Templates (`.env.example`, `.sample`,
`.template`, `.dist`), public keys and certificates, files named `example-…` or `sample-…`
and files below `testdata`, `fixtures`, `examples` or `samples` are not secrets. Listing a
file by name and writing a new one are not reads. Loading a file into the shell
(`source .env`) shows the model nothing and passes.

It is a guardrail for the common direct reads, not a boundary. A read that does not name the
file is outside it: a script (`python -c "open('.env')"`), `env` or `printenv`, a test or
build that prints its configuration, a search of a whole directory, and tools the host does
not pass to a hook. For a boundary use the host's own controls: Claude Code
`permissions.deny` rules such as `Read(./.env)` and its sandbox, Codex's sandbox, Cursor's
ignore files. Leave the rule out with `"disables": ["read-secret-file"]`.

Craft never reads such a file's content itself, whether the rule is in force or not: `craft
locate` skips it (counted under `skipped.secret`), Project Intelligence does not index it,
and standards checks and the stop review do not read it, so no answer or finding quotes a
line of it. Its name can still appear where Craft lists changed files.

`disables` leaves built-in rules out. `deny` adds the project's own: a command whose leading
words match is refused, with the executable matched by name and variable assignments or
wrappers such as `env` skipped. `craft settings` lists the rules in force and where each
comes from; refused commands are counted in the session summary. Every installed Craft
package enforces the policy before a tool call, so it holds with any one of them.

The policy only refuses, or on Claude Code hands a secret read to the user. It never
approves a tool call, so the host's prompts, sandbox and
deny rules apply as before, and what the host already blocks stays blocked: Claude Code
refuses deletes of critical paths in every mode and has more rules in auto mode; Codex
confines writes with its sandbox. The policy adds one command-level rule set that is the same
on every host and still holds where the host has none, such as Claude Code's bypass mode or
Codex with full access.

Targets are judged by name, from the directory the command runs in, against the project
root as the host names it and as it resolves on disk; a Windows drive or network path is
judged as absolute. Nothing else is read from disk, so a link inside the project that leads
outside is not seen. Commands are read like the review guard reads them, with the same limits; a command
nested deeper than 16 levels is refused. Personal preferences and task overrides cannot
change the policy. Use host permissions, sandboxing and credentials for enforcement.

## Builds and state

Builds reject symlinked or escaping package paths, stage output, replace only managed
directories, and never touch host configuration or contact services. MCP declarations reference
credentials through host environment variables; no package ships a live server.

Runtime state lives in `.craft/state/sessions/<session-id>/`, with project-wide records in
sibling folders (`telemetry/`, `intelligence/`, `workspaces/`, `notices/`); ignore `.craft/state/` in Git, which Craft never edits. Worker worktrees live in
`.craft/worktrees/` (ignore it too); `plan-integrate` makes local merge commits and never
pushes. State paths
reject symlinks and use bounded, locked, atomic writes. On POSIX, state directories are
created owner-only (0700) and files, including the evidence database, its lock and backup,
owner-readable only (0600). Checkpoints hold explicit task data
and Git state, not transcripts or tool output, and are evidence, never authorization.

State left by earlier plugin versions is never read, written or removed.

Personal preferences reject symlinks, non-files and oversized data. On POSIX the current user
must also own the file and its parent, with no group/other write bits, and a file replaced
while opening is refused. On Windows, account and directory ACLs are relied on unchecked.

Telemetry stays local by default and excludes prompts, transcripts, code, file contents,
commands, raw session IDs and credentials; see [observability](OBSERVABILITY.md).
