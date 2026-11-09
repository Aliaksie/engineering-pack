# Project evidence (preview)

A local index of syntax facts that agents can cite instead of searching again: declarations,
call sites with their enclosing function, and imports in source code; sections, links,
code-span mentions and decision records in Markdown (`.md`, `.markdown`); and keys in
configuration files (`.json`, `.jsonc`, `.toml`, `.yaml`, `.yml`). It records syntax, not resolved calls: runtime dispatch,
package exports, overloads and receiver types are not resolved, and direct search remains
useful.

## Languages

One extractor reads every language with a [Tree-sitter](https://tree-sitter.github.io/)
grammar and a query that names its declarations, calls and imports. Languages differ only in
that query.

| Level | Languages | Answers |
|---|---|---|
| Full | TypeScript (`.ts`, `.mts`, `.cts`, `.tsx`), JavaScript (`.js`, `.mjs`, `.cjs`, `.jsx`), Python (`.py`, `.pyi`), Java, Go, Rust | `--declaration`, `--callers` with the enclosing function or method, `--imports` by module and by imported name |
| Basic | C (`.c`, `.h`), C++ (`.cc`, `.cpp`, `.cxx`, `.hh`, `.hpp`, `.hxx`), C#, PHP, Ruby | `--declaration` and `--callers` by name with the enclosing function or method; C and C++ `#include` as imports |

What each level does not do:

- No language resolves a call to its declaration: rows are every call spelled with the name.
  A method call `a.f()` is cited by `f`; `new F()` (JavaScript, TypeScript) by `F`.
- JavaScript and TypeScript: a function passed as a value (`app.use(handler)`) is not a call.
  Functions are declared by `function`, by a variable, property or object key bound to an
  arrow or function expression, and by class methods and fields.
- Python: calls in a class body outside a method have no enclosing function. Decorators are
  not calls.
- Go: a function value passed as an argument is not a call. Top-level `var` and `const` are
  declared; local ones are not.
- Rust macro arguments are not parsed, so a call there is recorded from its `name(` spelling.
- Basic languages record no imports (except C and C++ includes) and no functions passed as
  values. C# and PHP declarations are those of the grammar's own tags query (classes,
  interfaces, methods, namespaces; PHP functions and fields), so C# structs, records and enums
  are not declared.
- Ruby: a call is recorded when it has a receiver, parentheses or arguments (`cart.total`,
  `apply(x)`). A bare `name` reads the same as a local variable, so it is not a call.
- A grammar can reject valid source (tree-sitter-typescript 0.23.2 rejects overload call
  signatures, `export type *` and generic arrow functions in `.ts` files). Facts outside the
  syntax error are kept and the source is `partial`: it answers lookups and is also searched
  literally, so an empty answer there is not an absence.

## Use

```sh
craft project-evidence refresh --enable --root .
craft project-evidence query   --enable --root . --value stateFile --verify
craft project-evidence status  --enable --root . --json
```

Without `--enable`, these commands read and write nothing. Other operations are `version`, `backup` (a
consistent snapshot beside the database; never overwrites an existing backup), `rebuild`
(clears derived evidence; run `refresh` next) and `warm` (refreshes in chunks of about 250 ms
of extraction until nothing is deferred, releasing the store between chunks; at most a minute).

When a session starts in a Git worktree with evidence enabled, the session-start hook runs
`warm` as a detached background process and returns at once, so the session's first lookup
usually finds a warm index instead of indexing within its own budget. A lookup waits up to
400 ms for a chunk to release the store. `CRAFT_EVIDENCE_WARM=0` in the host's environment
turns the background start off, for example for checkouts removed right after a session. On Windows the hook does not start it: a detached
child there would inherit the host's output pipe, so the first lookup indexes as before.

## Routed lookups

`craft locate` answers structural questions from this index, building it in the project's
`.craft/state/intelligence/` on first use. Turn this off per project with
`"evidence": {"enabled": false}` in `.craft/project.json`; lookups then search literally
(`route.reason` `index-disabled`):

```sh
craft locate --root . --callers processOpts --path modules/core
craft locate --root . --outline docs/guide.md
```

| Question | Matches | Answered from |
|---|---|---|
| `--declaration <name>` | declared name | every language above |
| `--callers <name>` | called function or method name, or a function passed as a value (a Java method reference, a lowercase Rust path such as `prompt::request` used as a value), with its enclosing declaration; a bare identifier passed as a value is not matched | every language above |
| `--imports <target>` | an imported name (`import { X } from`, `from m import X`), or a module by its whole trailing path or module segments: `utils/buffer` matches `../utils/buffer.js`, `Gson` matches `com.google.gson.Gson`, `b` matches `crate::a::b` (one full path per Rust `use` leaf) | full languages, C and C++ includes |
| `--section <heading>` | heading text | Markdown |
| `--outline <document>` | every section of one document | Markdown |
| `--mentions <name-or-path>` | a code span spelled as the value, or qualified before it: `Evidence` matches `` `Evidence` `` and `` `craft_engine::Evidence` `` (also `Type.member`, `Type#member`), while `craft_engine::Evidence` matches only that spelling; or a local link whose destination resolves to the value as a root-relative path (`src/hono.ts`). Each row names its section | Markdown |
| `--decisions <title-word>` | decision records whose title contains the value, case-insensitively; `*` matches every record. Rows carry the status as written, links resolved (`superseded by docs/adr/0002-x.md`) | Markdown under an `adr`, `adrs`, `decisions`, `decision-records` or `architecture-decisions` directory, or named `adr-*` |
| `--key <dotted.key>` | a configuration key by its dotted path from the document root, or ending in it at a segment (`strict` matches `compilerOptions.strict`); rows carry the scalar value | JSON (also with comments and trailing commas), TOML, YAML |

Markdown links count when written inline (links and images) or as reference
definitions; autolinks, HTML links and links or code spans that wrap across lines are not
read. URLs and fragment-only links are not files. A decision record's title is its first
heading; its status is front matter `status`, a `Status:` line (also a list item or bold)
before the second heading, or the first line of a `Status` section; none is a record without
status.

Configuration keys are read by scanners, not full parsers: keys inside arrays and YAML
sequences are not recorded, inline tables and flow collections are values, and a key segment
that contains `.` is joined as spelled. A lockfile (`package-lock.json`, `pnpm-lock.yaml`) or a
JSON, TOML or YAML file over 64 KiB is data (fixtures, schemas): a refresh does not read it, and
a `--key` lookup searches it literally as an unindexed candidate. Structurally broken JSON fails
extraction and is searched literally too. `.ini`, `.properties` and `.conf` files are candidates no provider
reads: a `--key` lookup searches them literally.

A qualified `--declaration` or `--callers` value (`Type.method`, `Type::method`, `module::fn`,
`Type#method`; a trailing `()` is dropped) matches its last segment: the index stores call sites
by the called name, not by the resolved receiver, so rows are every match of that name and
`route.name` and `route.qualifier` say the qualifier was not resolved. A qualified value that does
not end in a name is refused.

Every row states its `basis`. `parsed`: the parse itself establishes the row (a declaration,
an import statement, a Markdown section, link or decision record, a configuration key). `name`:
the row only spells the asked name (a call or reference token, a code-span mention), so it may
mean another declaration of that name. Text output marks name rows `[by name]`. A hook answer
states the `basis` of its rows (`parsed`, `name` or `mixed`, with each name row then marked)
and, when any row rests on its name, says so in its `use` line. The basis follows from the row's
kind, so stored evidence needs no new field.

Each lookup first refreshes the index for up to two seconds of extraction, then answers from
verified evidence. In-scope sources the index cannot answer for are its gaps: pending, failed
or partial extraction, a source changed since it was cited, or a candidate in a language the
question's providers do not read (a `.kt` file for `--callers`, an `.rst` file for
`--section`).
Up to 200 gap files are searched literally for the spelled value beside the index rows
(`route.answeredBy` `intelligence+locate`, `route.reason` `gaps-searched`, `route.detail`
counting them). The whole scope is searched literally instead (`locate`) when the reason is
`index-disabled`, `index-unavailable`, `scope-excluded`, `inventory-incomplete` or
`scope-gaps` (more than 200 gap files); an outline then searches its document for `#`.
A `--path` scope is a directory or one source path. An exclusion inside the scope is named in
`route.detail.excluded`; its sources are neither indexed nor searched. `route.complete` is true
when an empty answer is an absence within that coverage: no exclusion inside the scope, and no
literal search or a gap search that was neither truncated nor skipped a file. A whole-scope
literal search never is.

Output is text: the route line, rows grouped by source with their enclosing declaration,
line span and first line, then literal matches. `--json` prints the structure. Pages hold
`--max-matches` rows (30, up to 200); `--after` continues the index rows, without repeating
the gap search.

- **Query** matches an exact `id`, `name`, `path` or syntax `target`, or an `import` as
  `--imports` matches it (`--field`, `--value`),
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
  there. Check `pendingSources`, `failedSources` and `partialSources`: `inventoryComplete`
  describes discovery only.
- **Scope**: list generated or vendored paths in `.craft/project.json` as
  `"evidence": {"exclude": ["samples/", "vendor/bundles"]}` (project-relative prefixes,
  matched by whole path components). Refresh skips them and retires facts already stored
  under them; coverage reports the applied `exclusions` and `excludedSources`, so an empty
  answer from an excluded path is not mistaken for absence. Excluded directories are not
  walked for untracked files, nor nested worktrees inside them listed, so `excludedSources`
  counts only sources Git tracks outside such worktrees.
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

Call rows carry the spelled name and, when inside a function, method or constructor, their
`container`: the enclosing declaration's ID, kind, name and line. Calls in lambdas, closures
and nested anonymous functions keep their outer container; that does not mean they run when the
outer function does. Code directly in a class or impl body has no container. Kinds are
`<language>:<role>`, for example `ts:declares-class`, `python:invokes-syntax`,
`go:imports`, `js:imports-name`.

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
owner-readable only. Writes go through a write-ahead log (`project.sqlite-wal`, with its
`-shm` index, same mode) that is synced at checkpoints rather than at every commit: a power
failure may lose the last commits, which the next `refresh` extracts again. Evidence is
derived, so a store written by another schema version is discarded and the next `refresh`
rebuilds it. Corrupt stores refuse writes; preserve them for
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
[JavaScript](https://docs.rs/crate/tree-sitter-javascript/0.25.0),
[TypeScript](https://docs.rs/crate/tree-sitter-typescript/0.23.2),
[Python](https://docs.rs/crate/tree-sitter-python/0.25.0),
[Java](https://docs.rs/crate/tree-sitter-java/0.23.5),
[Go](https://docs.rs/crate/tree-sitter-go/0.25.0),
[Rust](https://docs.rs/crate/tree-sitter-rust/0.24.2),
[C](https://docs.rs/crate/tree-sitter-c/0.24.2),
[C++](https://docs.rs/crate/tree-sitter-cpp/0.23.4),
[C#](https://docs.rs/crate/tree-sitter-c-sharp/0.23.5),
[PHP](https://docs.rs/crate/tree-sitter-php/0.25.1) and
[Ruby](https://docs.rs/crate/tree-sitter-ruby/0.23.1) grammars. Queries for each language are
in `platform/queries/`; C#, PHP and Ruby use their grammar's tags query with a few added call
patterns (Ruby without its bare-identifier rule, which needs local-variable tracking).
