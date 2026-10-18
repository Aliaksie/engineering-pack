# Keep changes reviewable and repository actions explicit

Inspect the branch, index and working tree before changing Git state. Identify pre-existing
work and the paths belonging to the task. Follow the consumer's branching, commit and merge
policy. Use the user's existing authorization: completing an implementation does not by
itself request a commit, push or history rewrite.

## Commit a coherent outcome

When commits are requested, aim for one coherent, verified change per completed task. Include
its implementation, meaningful tests and necessary documentation together. A large task may
need several independently reviewable commits; several tightly coupled subtasks may belong
in one. Do not create a commit per file, tool call or worker, or package unrelated tasks
merely because they finished together. Follow the consumer's squash policy where applicable.

Stage only the intended paths or hunks and inspect the actual staged diff. Preserve unrelated
staged work; never include it just to obtain a clean status. Check generated files, secrets,
private evidence and accidental deletions. Verify the version being committed, and distinguish
pre-existing failures from regressions. Do not amend or squash another person's work without
the applicable authorization.

Follow the repository's message convention. If none is established, use
[Conventional Commits](https://www.conventionalcommits.org/en/v1.0.0/): a type, optional scope
and concise description of the resulting behavior.

```text
fix(reservations): preserve operation identity during recovery

Reconcile a lost response before issuing another reservation.
Verify recovery of a committed result and rejection of duplicate effects.
```

Use `feat` for a feature and `fix` for a defect; types such as `docs`, `test`, `refactor`,
`perf`, `build` and `ci` distinguish other work. Describe impact and rationale in the body
when useful. Mark an actual incompatible contract change with `!` or a `BREAKING CHANGE:`
footer, explaining the consumer impact. Only claim checks that ran. Message conventions do
not authorize a release, version bump or automatic publication.

## Parallel changes and worktrees

Use tools or bounded read-only workers for retrieval; a read-only assignment normally needs
no branch or worktree. Workers must not change the shared checkout, index or branch while
performing lookup. A working tree can change underneath a reader: retain revision/scope
identity and refresh affected evidence after edits.

For concurrent writing, prefer a separate branch and Git worktree for each independent
assignment. First establish the base revision, task-owned paths, acceptance conditions and
integration owner. Inspect existing worktrees and choose unused paths and branch names.
Do not force Git to check out an already-used branch. Overlapping edits or dependent tasks
need sequencing or one owner even when separate worktrees are available.

Example commands, after confirming that current `HEAD` is the intended committed base and
the sample branch/path are unused:

```text
git worktree list --porcelain
git worktree add -b task/reservation-recovery ../reservation-recovery HEAD
git -C ../reservation-recovery status --short
```

Uncommitted changes in the original checkout are not copied into the new worktree. Supply
needed task inputs deliberately; do not commit or copy unrelated work to make them appear.
Worktrees separate checked-out files and indexes, but share repository objects, refs and
some configuration. They do not isolate ports, external services, credentials or processes.
Give tests/builds independent resources where needed, and use real isolation for untrusted code.
See the [Git worktree manual](https://git-scm.com/docs/git-worktree).

The integration owner inspects each returned diff, resolves conflicts against applicable
contracts and runs affected checks on the combined state. A worker's passing check does not
prove the integrated change. Use the consumer's authorized patch/merge/cherry-pick process;
workers commit only if their assignment includes it. Remove only task-owned worktrees after
changes are safely integrated or preserved and the worktree is clean. Never use forced
removal or broad pruning to hide unfinished work.

## Remote actions

Confirm destination, branch and included revisions before a remote action. Do not force-push,
reset, clean or discard changes as a routine repair. Resolve conflicts with affected contracts
in mind and rerun checks justified by the resolution. Report the resulting commit identifiers,
actual verification and remaining work; distinguish local checks from hosted CI.
