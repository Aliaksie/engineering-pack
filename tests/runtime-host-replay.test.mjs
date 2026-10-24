import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';
import assert from 'node:assert/strict';
import test from 'node:test';
import * as source from '../hosts/hooks.mjs';

const repository = fileURLToPath(new URL('../', import.meta.url));
const baseline = JSON.parse(fs.readFileSync(new URL('./fixtures/runtime/host-contracts.json', import.meta.url), 'utf8'));

// Keep host wire translation independent from the runtime migration. The frozen
// inputs use explicit cwd values so replay never depends on the developer's tree.
for (const boundary of ['source', 'relocated']) {
  test(`frozen host contracts: ${boundary}`, async t => {
    let adapter = source;
    if (boundary === 'relocated') {
      const scratch = fs.mkdtempSync(path.join(os.tmpdir(), 'craft-host-replay-'));
      t.after(() => fs.rmSync(scratch, { recursive: true, force: true }));
      const bundle = path.join(scratch, 'bundle with spaces');
      fs.cpSync(path.join(repository, 'dist/agents'), bundle, { recursive: true });
      adapter = await import(pathToFileURL(path.join(bundle, 'hosts/hooks.mjs')).href);
    }
    for (const fixture of baseline.cases) {
      await t.test(fixture.id, () => {
        const input = structuredClone(fixture.input);
        const result = adapter[fixture.operation](input, fixture.host, fixture.definition);
        // Normalized events contain optional undefined values; the portable native
        // boundary represents these as absent JSON fields, never null.
        assert.deepEqual(JSON.parse(JSON.stringify(result)), fixture.response);
        assert.deepEqual(input, fixture.input, 'host translation must not mutate its input');
      });
    }
  });
}
