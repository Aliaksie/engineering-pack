import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { spawnSync } from 'node:child_process';
import assert from 'node:assert/strict';
import test from 'node:test';

const repository = fileURLToPath(new URL('../', import.meta.url));
const baseline = JSON.parse(fs.readFileSync(new URL('./fixtures/runtime/cli-contracts.json', import.meta.url), 'utf8'));

// Exercise executable boundaries, including a relocated export, rather than run()
// alone: exit status, stdout/stderr separation and imports are public contracts.
for (const boundary of ['source', 'relocated']) {
  test(`frozen CLI contracts: ${boundary}`, async t => {
    const scratch = fs.mkdtempSync(path.join(os.tmpdir(), 'craft-cli-replay-'));
    t.after(() => fs.rmSync(scratch, { recursive: true, force: true }));
    const bundle = boundary === 'source' ? repository : path.join(scratch, 'bundle with spaces');
    if (boundary === 'relocated') fs.cpSync(path.join(repository, 'dist/agents'), bundle, { recursive: true });
    for (const fixture of baseline.cases) {
      await t.test(fixture.id, () => {
        const root = path.join(scratch, fixture.id);
        fs.mkdirSync(root);
        const args = [...fixture.args];
        if (fixture.entry === 'cli/main.mjs' && args.length) args.push('--preferences', path.join(root, 'missing-preferences.json'));
        const child = spawnSync(process.execPath, [path.join(bundle, fixture.entry), ...args], {
          cwd: root, encoding: 'utf8', timeout: 10000, maxBuffer: 1024 * 1024
        });
        assert.ifError(child.error);
        assert.equal(child.signal, null);
        assert.equal(child.status, fixture.status, child.stderr);
        if (fixture.mode === 'failure') {
          assert.equal(child.stdout, '');
          assert.ok(child.stderr.trim(), 'failure must explain itself on stderr');
          if (fixture.errorCode) assert.equal(child.stderr.split(':')[0], fixture.errorCode);
        } else {
          assert.equal(child.stderr, '');
          assert.ok(child.stdout.endsWith('\n'));
          const result = JSON.parse(child.stdout);
          const actual = fixture.mode === 'catalog' ? { commands: result.commands }
            : fixture.mode === 'usage' ? { usage: result.usage } : result;
          assert.deepEqual(actual, fixture.response);
        }
      });
    }
  });
}
