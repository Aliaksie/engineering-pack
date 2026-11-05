# JavaScript decisions

Determine whether execution is in a browser, server, worker or embedded engine and inspect the
module/build configuration. ESM and CommonJS have different loading contracts; preserve public
exports and initialization order when changing boundaries. Do not infer Node APIs from `.js`.

Treat external values as unknown runtime data. Distinguish missing, null, empty and zero where
the contract does; avoid truthiness defaults that erase valid input. Check numeric precision and
units, especially identifiers, money and timestamps. Use the platform's available primitives
rather than assuming a polyfill or recent API exists everywhere.

Async functions return promises; an ignored rejection is not completed background work. Await
owned operations, bound parallel work and make abort/cleanup behavior explicit. Event listeners,
timers and streams need lifecycle cleanup. A single event loop does not prevent logical races
between asynchronous reads and writes.

```javascript
const limit = suppliedLimit ?? defaultLimit;
if (!Number.isSafeInteger(limit) || limit < 0 || limit > 1000) {
  throw new TypeError('Limit must be a whole number from 0 to 1000');
}
```

Here 1000 is an illustrative page-size ceiling; use the actual domain budget.

Verify the actual runtime and packaging path. A type checker or transpiler cannot prove a
browser global exists in a server process.

## Bounded literal search in Node

This Node ESM example requires a trusted `rg` (ripgrep) installation. The caller supplies an
approved directory; this function does not establish filesystem containment. It is a synchronous
CLI helper, not a request-handler pattern: it blocks the event loop. It illustrates
argv separation, ripgrep's literal/terminator options, budgets and distinct no-match/failure
outcomes. Browser code needs a different API.

```javascript
import { spawnSync } from 'node:child_process';

export function searchLiteral(term, approvedDirectory) {
  if (typeof term !== 'string' || !term || Buffer.byteLength(term, 'utf8') > 256) {
    throw new TypeError('Search term must contain 1 to 256 UTF-8 bytes');
  }
  const result = spawnSync('rg', [
    '--no-config', '--fixed-strings', '--line-number', '--with-filename', '--color', 'never',
    '--', term, approvedDirectory
  ], {
    shell: false, encoding: 'utf8', timeout: 2000, maxBuffer: 64 * 1024
  });
  if (result.error) throw result.error; // Includes startup, timeout and output-budget failure.
  if (result.status === 1) return ''; // ripgrep: search completed, no matching lines.
  if (result.status !== 0) throw new Error(`Search failed: exit ${result.status}`);
  return result.stdout;
}
```

The executable comes from the trusted environment, not user input. A term beginning with
`--` remains data after the terminator, and punctuation is literal because of `--fixed-strings`.
A missing directory, failed process or exhausted output budget is an error, not evidence of
absence. Choose budgets for the consumer; these sample limits are not universal defaults.

API details: [Node child processes](https://nodejs.org/api/child_process.html#child_processspawnsynccommand-args-options)
and [ripgrep option definitions](https://github.com/BurntSushi/ripgrep/blob/master/crates/core/flags/defs.rs).
The timeout requests termination; it is not a universal hard wall-clock guarantee.
