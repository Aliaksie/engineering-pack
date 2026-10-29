# Project evidence (preview)

An opt-in local index of syntax facts that agents can cite instead of searching again:
JavaScript declarations and literal imports (`.js`, `.mjs`, `.jsx`, `.cjs`), Java
declarations, imports and method-invocation syntax (`.java`), and Markdown sections
(`.md`, `.markdown`). It records syntax, not
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
  30 rows per page (up to 200) in path and line order, with an `--after` continuation that
  is refused once its row is no longer stored. Narrow before paging with `--kind <kind>`,
  `--under <directory>` (whole path components) or `--source <path>`. A truncated page
  reports the total matches by kind and by directory. Each source's hash and provider are
  listed once under `sources`. `--verify` rehashes the cited sources and marks each verified
  or stale; a verified row that spans several lines (a declaration, a section) gains its
  `endLine`. `--excerpt` also verifies and adds the row's trimmed first line.
- **Refresh** reuses unchanged extractions and retires deleted sources. Optional
  `--max-files`, `--max-bytes`, `--max-output-bytes` and `--max-duration-ms` limit one run;
  deferred sources stay pending (and hidden) until a later refresh. Writes commit in batches,
  so an interrupted refresh keeps every committed batch and the next refresh continues from
  there. Check `pendingSources` and `failedSources`: `inventoryComplete` describes discovery
  only.
- **Scope**: list generated or vendored paths in `.craft/project.json` as
  `"evidence": {"exclude": ["samples/", "vendor/bundles"]}` (project-relative prefixes,
  matched by whole path components). Refresh skips them and retires facts already stored
  under them; coverage reports the applied `exclusions` and `excludedSources`, so an empty
  answer from an excluded path is not mistaken for absence.
- **Roots**: inside a Git worktree, Git lists the sources (tracked and unignored). Any other
  root, such as a folder of repositories or a plain folder, is walked without following
  symlinks and reads no ignore files; each nested Git worktree found there is listed by its
  own Git, so its ignore rules apply. Paths stay root-relative (`api/src/Api.java`), and
  coverage reports `inventory` as `git` or `filesystem` with the nested `repositories`. A walk
  stops after 1,000,000 directory entries and keeps earlier evidence; select a narrower root.
- **Workspaces**: repeat `--member id=/absolute/root` to index several repositories. Each
  keeps its own database and generation. Literal relative imports between selected members
  get a resolution record with the target's current hash; that is a declared path, not a
  proven call.

Java invocation rows carry the spelled method name and, when inside a method or constructor,
their `container`: the enclosing declaration's ID, kind, name and line. Calls in lambdas keep
their outer container; that does not mean they run when the outer method does.

Markdown `md:section` rows are named by their heading text as spelled (inline markup kept,
closing `#`s removed). A section runs from its heading to the next heading of the same or a
higher level, and its `container` is the enclosing section, so `--field path --value
docs/guide.md --verify` lists a document's outline with line spans to read. Headings in
fenced or indented code, HTML comments and front matter are not sections. Inside list items
and block quotes only a `#` heading opening the line is recognized, and HTML blocks other than
comments are not.

## Storage and limits

The database is `.craft/state/intelligence/project.sqlite`, guarded by an
OS lock; `backup` writes `project.sqlite.backup` beside it. On POSIX all three are created
owner-readable only. Evidence is derived, so a store written by another schema version is
discarded and the next `refresh` rebuilds it. Corrupt stores refuse writes; preserve them for
diagnosis rather than rebuilding blindly. Per source: 2 MiB, 20,000 records and a two-second
extraction deadline. Generated state, build output and dependency directories are always excluded.
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
