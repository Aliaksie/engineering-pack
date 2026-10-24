# Project evidence development preview

The opt-in native prototype extracts JavaScript declarations and literal import/re-export
specifiers from Git-listed `.js`, `.mjs`, `.jsx` and `.cjs` sources, plus Java declarations,
imports and method-invocation syntax from `.java` files. It records dynamic JavaScript
imports as unresolved syntax. It does not resolve runtime dispatch, package exports,
Java overloads or symbol references. Direct source search remains useful.

## Build and run

Development requires Rust/Cargo and a C compiler for bundled SQLite and Tree-sitter.
The current native implementation has been exercised on Apple Silicon macOS only.
Run from the checkout:

```sh
cargo build --workspace --locked
node cli/project-evidence.mjs refresh --enable --root . --binary "$PWD/target/debug/craft-project-evidence"
node cli/project-evidence.mjs query --enable --root . --binary "$PWD/target/debug/craft-project-evidence" --value stateFile --verify
node cli/project-evidence.mjs status --enable --root . --binary "$PWD/target/debug/craft-project-evidence" --json
```

Omitting `--enable` disables native invocation and state access. The wrapper never
builds or downloads a binary. No automatic host hook is installed.
`version --enable --binary <absolute-path>` reports binary, schema and active `providers`.
The legacy `provider` field continues to identify the JavaScript provider.

Queries match an exact occurrence `id`, `name`, source `path`, or syntax `target` using `--field`
and `--value`. Pages default to 30 rows, with a maximum of 200 and an explicit `--after`
continuation. Add `--source <relative-path>` (native `sourcePath`) to restrict any
query to one exact file before pagination. Paths are relative to each selected member;
select one member when the same relative path exists in several roots. This avoids
paging through unrelated languages or tests for common method names. Continuations
are for the same query, source filter and stored generation; restart after
a refresh. `--json` exposes hashes, byte ranges, byte columns, provider and observation time
(Unix milliseconds). Text output shows source citations.

`backup` creates a consistent SQLite snapshot beside the database with suffix
`.backup`; an existing backup is never replaced. Preserve or remove it explicitly
before requesting another. `rebuild` transactionally clears derived evidence and
marks coverage incomplete; run `refresh` next. It preserves source files, unrelated
state and consumer configuration. Both operations use the normal exclusive lock.
Unknown schema versions or corrupt stores still refuse writes; preserve those files
for diagnosis before manually replacing them with a compatible snapshot while no
command is running. Rebuild does not silently repair an incompatible store.

## Relocatable local bundle

Run `npm run build:native` to build an offline, locked release for the current Rust
host target. Dependencies must already be cached; missing dependencies fail rather
than downloading during packaging. `cargo` and `rustc` must be on PATH, or supplied
through `CARGO` and `RUSTC` respectively. The command writes
`dist/project-evidence/<target>/` and only replaces a previously generated bundle.
This developer build is separate from `npm run build`. CI and release packaging include
the checked prebuilts in each complete host bundle and independently cached plugin.

Move the entire bundle directory to its intended local location. With Node 22+ and
Git on PATH, run `node <bundle>/cli/project-evidence.mjs version --enable` or the same
refresh/query commands shown above without `--binary`. No Rust toolchain is needed
at runtime. The bundle contains the release binary, the small wrapper, dependency
notices and a manifest with protocol/schema/provider versions, target, source revision,
dirty-state indicator and per-file SHA-256 checksums. The manifest itself is excluded
from its checksum list. Checksums detect changed content; they are not signatures.

The build does not install or enable a host integration. Keep the previous bundle
directory when replacing an installed copy; rollback selects that directory again.
Omitting `--enable` disables access without removing data. Removing a bundle leaves
repository source and private evidence state intact. The native CI gate exercises Linux
x86-64, Apple Silicon macOS and Windows x86-64; see the
[supported candidate platforms](PLATFORMS.md#native-runtime-in-candidate-bundles).

## Explicit workspaces

Repeat `--member client=/absolute/client --member server=/absolute/server` on
refresh, status or query to select named members. Only selected roots participate;
missing members are reported as unavailable. Duplicate IDs, duplicate canonical roots
and overlapping roots are rejected before refresh. Member IDs are portable citation
names; physical roots remain separate. Each member keeps its own database, generation,
query page and continuation. The response does not represent an atomic workspace snapshot.
For paging one member, select it and pass its returned continuation with `--after`.

Workspace query rows include the member and a member-qualified `workspaceId`. Literal
relative imports into existing eligible files in selected members receive a bounded
resolution record with target member, path and current hash. This is a declared import
path, not proof of a runtime call. Package exports, omitted extensions, directory indexes
and dynamic resolution remain unchecked. Target content is checked during resolution;
source freshness remains independent and requires `--verify`. A stale source never
becomes verified merely because its target exists. Missing or unselected targets stay
unresolved. Workspace responses currently use JSON, including in the default display.

## Ownership and evidence lifecycle

Java records distinguish `java:declares-type`, `java:declares-method`,
`java:declares-constructor`, `java:imports`, `java:imports-static` and
`java:invokes-syntax`. Invocation `target` is the spelled method name, not a callee ID.
An optional `container` is the same-source ID of the lexically enclosing method or
constructor; query it with `--field id --value <container>` to inspect the caller's
citation. Overloads retain distinct declaration IDs. Calls in lambdas retain their
lexical container; this does not mean the lambda executes when its outer method does.
Field/initializer calls have no method container, and anonymous/local type boundaries
do not inherit the outer method's container. Comments and strings do not create calls.

Receiver types, inheritance, overload selection, constructors called with `new`, method
references, reflection, schema references and template relationships remain unchecked.
Matching method names across files or members does not establish a resolved edge.
Java imports retain their spelled qualified names and wildcard suffixes without
claiming that a selected member supplies the imported type.

`engine/src` owns portable records and identities; `platform/src` owns SQLite,
Git inventory and the Tree-sitter provider; `application/src` orchestrates operations.
The Node platform bridge invokes a versioned JSON operation through `application/src/main.rs`.
The existing JavaScript engine remains in place for unaffected operations under
[ADR 0001](adr/0001-rust-core-runtime.md).

SQLite state lives in `.craft/state/<worktree-id>/shared/project-evidence.sqlite`.
The native runtime owns its canonical worktree identity; consumers use paths returned
by the API (for example `backup`) rather than reconstructing the directory hash.
An OS lock serializes operations and releases on process exit. A successful extraction
atomically replaces that source/provider's records, including empty success. Parse/read
failure records a diagnostic and retires outdated evidence. Only successful
inventory completion retires deleted/excluded sources. Restarting refresh reuses successful
unchanged extractions. Refresh retires contributions from providers no longer active,
including replaced provider versions, before acquiring new evidence. Unsupported schema
versions and corrupt databases refuse writes without automatic rebuilding.

Hashes are SHA-256 over raw source bytes. Occurrence IDs hash a UTF-8 JSON string array
of relative path, provider version, predicate, name and start byte. They replay across
relocation and repeated extraction; an occurrence moved within a source can receive a
new ID. Unknown predicate namespaces are preserved by storage.

Query results describe stored snapshots. `--verify` hashes only selected sources and
labels each row verified or stale/unavailable; it does not establish complete references
or freeze a changing worktree. Inventory completion is not semantic completeness.
Symlink sources and database paths are rejected. Path checks assume the local worktree
is not being adversarially replaced between validation and file access.

The initial bounds are 2 MiB per source, 20,000 extracted records per source, a two-second
parser/extraction deadline and a 120-second wrapper operation deadline. The repository
has no file-count limit. Inventory streams NUL-delimited Git output; generated state,
build output, private plans and dependency directories are excluded. Cancellation can
leave partial coverage; the next explicit refresh restarts inventory and reuses stored work.

Resolved callee relationships, richer diagnostics and host installation remain under
development. The local bundle is a development preview, not a completed migration or
a verified release for all platforms.

Refresh accepts optional positive integer limits, each applied independently per member:

| CLI flag | Native input | Bound |
|---|---|---|
| `--max-files N` | `maxFiles` | Extraction attempts, including failures and rejected output |
| `--max-bytes N` | `maxBytes` | Source bytes passed to extraction, including failed attempts |
| `--max-output-bytes N` | `maxOutputBytes` | Compact UTF-8 JSON bytes of accepted complete extraction records, including source metadata |
| `--max-duration-ms N` | `maxDurationMs` | Elapsed refresh time after store opening, checked before starting each extraction |

Omitted limits are unrestricted within the fixed per-source bounds above. Time is a
cooperative scheduling limit: an in-flight extraction and write may finish after it.
Inventory, hashing, reuse and deletion detection still scan all eligible files even
after a limit is reached. These controls do not bound total operation time, scanned
bytes, SQLite size, parser memory or query/wrapper response size.

Refresh reports `extractionAttempts`, `extractionBytes`, `outputBytes` and counts in
`deferredByLimit`; `bytesScanned` includes reused and deferred sources. Output size is
counted after extraction, before storage, without a second serialized copy. A source
is never partially published to fit a limit. A source larger than the entire byte or
output allowance stays pending until that limit is raised or omitted; smaller later
sources can still advance. Deferred sources persist as
`pendingSources`, and their older evidence is hidden from queries. Repeat refresh
to advance: unchanged successful and failed extractions are reused, so a syntax
error cannot starve later files. Changing the source retries extraction; rebuild
also clears cached failures. `inventoryComplete` describes discovery only; inspect
`pendingSources` and `failedSources` for extraction coverage.

The provider revision advances to v2 for failed-content caching; the next refresh
retires v1 contributions and reacquires them rather than reusing old failure hashes.

## Verification

```sh
cargo test --workspace --locked
cargo clippy --workspace --locked --all-targets -- -D warnings
CRAFT_PROJECT_EVIDENCE_BINARY="$PWD/target/debug/craft-project-evidence" node --test tests/project-evidence.test.mjs
CRAFT_PROJECT_EVIDENCE_BINARY="$PWD/target/debug/craft-project-evidence" CRAFT_EVIDENCE_LARGE_SMOKE=1 node --test tests/project-evidence.test.mjs
```

The large smoke creates 10,001 eligible files and a pathname listing larger than 2 MiB.
It checks the tail query and unchanged reuse, without making comparative performance claims.
The default Node test suite checks disabled behavior and skips native cases unless the
binary is explicitly supplied. Native checks are therefore separate from `npm run check`.

Storage uses [rusqlite](https://docs.rs/crate/rusqlite/0.40.2) with bundled SQLite;
syntax extraction uses [Tree-sitter](https://docs.rs/crate/tree-sitter/0.27.1) and its
[JavaScript grammar](https://docs.rs/crate/tree-sitter-javascript/0.25.0) and
[Java grammar](https://docs.rs/crate/tree-sitter-java/0.23.5).
`Cargo.lock` pins the dependency graph. These libraries use permissive licenses;
native distribution must include the applicable dependency notices.

### Native invocation contract

The `craft` executable and the compatibility name `craft-project-evidence` accept
one UTF-8 JSON request on standard input, bounded to 3 MiB, and write one JSON
response on standard output. Both executables use the same application library.
The request envelope is `{ "formatVersion": 1, "operation": "version", "input": {} }`.
Operations are `version`, `extract`, `refresh`, `status`, `query`, `backup` and
`rebuild`; operation inputs retain their existing Project Intelligence contracts.
Successful responses contain `formatVersion`, `ok: true` and `result`. Failures
contain `formatVersion`, `ok: false` and `error` with `code` and `message`, and exit
with status 1. Invalid JSON or oversized input returns `INVALID_INPUT`, an
unsupported envelope version returns `UNSUPPORTED_VERSION`, and an unknown
operation returns `UNKNOWN_OPERATION`.

The existing Node bridge continues to select `craft-project-evidence`, checks its
version before invoking an operation, and reports native failures without replay.
Disabled access bypasses the executable. Missing executables and deadlines remain
explicit availability errors; signal interruption reports possible partial effects.
