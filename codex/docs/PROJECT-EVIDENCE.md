# Project evidence (preview)

An opt-in local index of syntax facts that agents can cite instead of searching again:
JavaScript declarations and literal imports (`.js`, `.mjs`, `.jsx`, `.cjs`), and Java
declarations, imports and method-invocation syntax (`.java`). It records syntax, not
resolved calls: runtime dispatch, package exports, overloads and receiver types are not
resolved, and direct search remains useful.

## Use

```sh
craft project-evidence refresh --enable --root .
craft project-evidence query   --enable --root . --value stateFile --verify
craft project-evidence status  --enable --root . --json
```

Without `--enable`, nothing is read or written. No hook runs it automatically. Other
operations are `version`, `backup` (a consistent snapshot beside the database; never
overwrites an existing backup) and `rebuild` (clears derived evidence; run `refresh` next).

- **Query** matches an exact `id`, `name`, `path` or syntax `target` (`--field`, `--value`),
  30 rows per page (up to 200) with an `--after` continuation. `--source <path>` restricts to
  one file. `--verify` rehashes the selected sources and marks each row verified or stale.
- **Refresh** reuses unchanged extractions and retires deleted sources. Optional
  `--max-files`, `--max-bytes`, `--max-output-bytes` and `--max-duration-ms` limit one run;
  deferred sources stay pending (and hidden) until a later refresh. Check `pendingSources`
  and `failedSources`: `inventoryComplete` describes discovery only.
- **Workspaces**: repeat `--member id=/absolute/root` to index several repositories. Each
  keeps its own database and generation. Literal relative imports between selected members
  get a resolution record with the target's current hash; that is a declared path, not a
  proven call.

Java invocation rows carry the spelled method name and, when inside a method or constructor,
its `container` ID. Query that ID to see the caller. Calls in lambdas keep their outer
container; that does not mean they run when the outer method does.

## Storage and limits

The database is `.craft/state/intelligence/project.sqlite`, guarded by an
OS lock; `backup` writes `project.sqlite.backup` beside it. On POSIX all three are created
owner-readable only. Unknown schema versions and corrupt stores refuse writes; preserve them for
diagnosis rather than rebuilding blindly. Per source: 2 MiB, 20,000 records and a two-second
extraction deadline. Generated state, build output and dependency directories are excluded.
Occurrence IDs are stable across relocation and repeated extraction.

## Building

Development needs Rust and a C compiler (bundled SQLite and Tree-sitter):

```sh
cargo build --workspace --locked
CRAFT_PROJECT_EVIDENCE_BINARY="$PWD/target/debug/craft" node --test tests/project-evidence.test.mjs
```

The native tests are skipped unless the binary is supplied; `CRAFT_EVIDENCE_LARGE_SMOKE=1`
adds a 10,001-file inventory case. `npm run build:native` builds a relocatable bundle for the
current platform under `dist/project-evidence/<target>/`, with dependency notices and
per-file checksums ([local builds](INSTALL.md#local-builds-and-project-overlays)).
Dependencies are pinned in `Cargo.lock`: [rusqlite](https://docs.rs/crate/rusqlite/0.40.2),
[Tree-sitter](https://docs.rs/crate/tree-sitter/0.27.1) with the
[JavaScript](https://docs.rs/crate/tree-sitter-javascript/0.25.0) and
[Java](https://docs.rs/crate/tree-sitter-java/0.23.5) grammars.
