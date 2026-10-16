import { test } from 'node:test';
import assert from 'node:assert/strict';
import { mkdtempSync, mkdirSync, writeFileSync, readdirSync, readFileSync, existsSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const checkout = fileURLToPath(new URL('../', import.meta.url));
function inspectLinks(directory) {
  const pending = [directory];
  const failures = [];
  while (pending.length) {
    const folder = pending.pop();
    for (const entry of readdirSync(folder, { withFileTypes: true })) {
      const filename = path.join(folder, entry.name);
      if (entry.isDirectory()) { pending.push(filename); continue; }
      if (!entry.isFile() || !entry.name.endsWith('.md')) continue;
      let fence;
      for (const [index, line] of readFileSync(filename, 'utf8').split(/\r?\n/).entries()) {
        const marker = /^ {0,3}(`{3,}|~{3,})/.exec(line)?.[1];
        if (marker) {
          if (!fence) fence = marker;
          else if (marker[0] === fence[0] && marker.length >= fence.length) fence = undefined;
          continue;
        }
        if (fence) continue;
        for (const link of line.matchAll(/\]\((?:<([^>]+)>|([^\s)]+))(?:\s+"[^"]*")?\)/g)) {
          const address = link[1] ?? link[2];
          if (/^(?:[a-z][a-z0-9+.-]*:|#)/i.test(address)) continue;
          const pathname = decodeURIComponent(address.split('#')[0]);
          if (pathname && !existsSync(path.resolve(folder, pathname))) failures.push({ file: path.relative(directory, filename), line: index + 1, target: address });
        }
      }
    }
  }
  return failures;
}

test('source documentation and six distributable layouts contain reachable local links', () => {
  for (const relative of ['docs', 'packages', ...['agents', 'claude', 'codex', 'copilot', 'cursor', 'gemini'].map(host => `dist/${host}`)]) {
    assert.deepEqual(inspectLinks(path.join(checkout, relative)), [], relative);
  }
});

test('link inspection distinguishes missing resources from external addresses and quoted examples', t => {
  const fixture = mkdtempSync(path.join(tmpdir(), 'resource-links-'));
  t.after(() => rmSync(fixture, { recursive: true, force: true }));
  mkdirSync(path.join(fixture, 'nested'));
  writeFileSync(path.join(fixture, 'asset name.md'), '# Resource\n');
  writeFileSync(path.join(fixture, 'nested', 'reader.md'), [
    '[asset](../asset%20name.md#usage)',
    '[missing](../removed.md)',
    '[remote](https://example.org/resource)',
    '~~~md',
    '[quoted](unavailable.md)',
    '~~~',
    '````text',
    '```',
    '[also quoted](unavailable.md)',
    '````',
    '[space](<../asset name.md>)'
  ].join('\r\n'));
  assert.deepEqual(inspectLinks(fixture), [{ file: path.join('nested', 'reader.md'), line: 2, target: '../removed.md' }]);
});
