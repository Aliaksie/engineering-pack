import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';
import assert from 'node:assert/strict';
import test from 'node:test';
import * as source from '../platform/node/state-files.mjs';

const repository = fileURLToPath(new URL('../', import.meta.url));
const baseline = JSON.parse(fs.readFileSync(new URL('./fixtures/runtime/state-contracts.json', import.meta.url), 'utf8'));

for (const boundary of ['source', 'relocated']) {
  test(`frozen state contracts: ${boundary}`, async t => {
    const scratch = fs.realpathSync(fs.mkdtempSync(path.join(os.tmpdir(), 'craft-state-replay-')));
    t.after(() => fs.rmSync(scratch, { recursive: true, force: true }));
    let adapter = source;
    if (boundary === 'relocated') {
      const bundle = path.join(scratch, 'bundle with spaces λ');
      fs.cpSync(path.join(repository, 'dist/agents'), bundle, { recursive: true });
      adapter = await import(pathToFileURL(path.join(bundle, 'platform/node/state-files.mjs')).href);
    }
    function record(id) {
      const root = path.join(scratch, id);
      fs.mkdirSync(root);
      return { root, file: adapter.stateFile(root, 'record.json', true, 'session λ') };
    }
    for (const fixture of baseline.writes) {
      await t.test(`bytes and exact UTF-8 bound: ${fixture.id}`, () => {
        const { file } = record(fixture.id);
        adapter.writeState(file, fixture.value, fixture.bytes, fixture.pretty);
        assert.equal(fs.readFileSync(file, 'utf8'), fixture.text);
        assert.deepEqual(adapter.readState(file, fixture.bytes), fixture.value);
        assert.throws(() => adapter.readState(file, fixture.bytes - 1), /bounded state/);
        assert.throws(() => adapter.writeState(file, fixture.value, fixture.bytes - 1, fixture.pretty), /size limit/);
        assert.equal(fs.readFileSync(file, 'utf8'), fixture.text);
      });
    }
    await t.test('missing, malformed and oversized records preserve existing bytes', () => {
      const { file } = record('invalid');
      assert.equal(adapter.readState(file, 64), null);
      for (const text of ['{broken', ' '.repeat(65)]) {
        fs.writeFileSync(file, text);
        assert.throws(() => adapter.writeState(file, { replacement: true }, 64));
        assert.equal(fs.readFileSync(file, 'utf8'), text);
        assert.deepEqual(fs.readdirSync(path.dirname(file)), ['record.json']);
      }
    });
    await t.test('exclusive lock refuses overlap and unresolved stale locks', () => {
      const { file } = record('lock');
      const ticket = adapter.lockState(file);
      try { assert.equal(adapter.lockState(file), null); }
      finally { adapter.releaseState(ticket); }
      assert.equal(fs.existsSync(`${file}.lock`), false);
      fs.writeFileSync(`${file}.lock`, 'unresolved writer');
      assert.equal(adapter.lockState(file), null);
      assert.equal(fs.readFileSync(`${file}.lock`, 'utf8'), 'unresolved writer');
    });
    await t.test('failed atomic publication preserves old bytes and removes temporary files', () => {
      const { file } = record('interruption');
      adapter.writeState(file, { old: true }, 64);
      const rename = fs.renameSync;
      const mock = t.mock.method(fs, 'renameSync', (from, to) => {
        if (to === file) throw new Error('injected publication failure');
        return rename(from, to);
      });
      try { assert.throws(() => adapter.writeState(file, { next: true }, 64), /injected/); }
      finally { mock.mock.restore(); }
      assert.equal(fs.readFileSync(file, 'utf8'), '{"old":true}\n');
      assert.deepEqual(fs.readdirSync(path.dirname(file)), ['record.json']);
      adapter.writeState(file, { next: true }, 64);
      assert.deepEqual(adapter.readState(file, 64), { next: true });
    });
    await t.test('routing generation migrates with its record and remains source-bound', () => {
      const { root } = record('routing');
      const legacy = path.join(root, '.capabilities', 'state');
      fs.mkdirSync(legacy, { recursive: true });
      fs.writeFileSync(path.join(legacy, 'routing-demo.json'), '{"generation":2}\n');
      fs.writeFileSync(path.join(legacy, 'routing-demo.json.generation'), '2\n');
      const file = adapter.stateFile(root, 'routing-demo.json', false, 'session λ');
      assert.equal(fs.readFileSync(file, 'utf8'), '{"generation":2}\n');
      assert.equal(fs.readFileSync(`${file}.generation`, 'utf8'), '2\n');
      const receipts = fs.readdirSync(path.dirname(file)).filter(name => name.startsWith('migration-'));
      assert.equal(receipts.length, 1);
      const receipt = adapter.readState(path.join(path.dirname(file), receipts[0]), 4096);
      assert.equal(receipt.formatVersion, 1);
      assert.match(receipt.source, /^[a-f0-9]{24}$/);
      fs.writeFileSync(path.join(legacy, 'routing-demo.json.generation'), '3\n');
      assert.throws(() => adapter.stateFile(root, 'routing-demo.json', false, 'session λ'), /changed after migration/);
      assert.equal(fs.readFileSync(`${file}.generation`, 'utf8'), '2\n');
    });
  });
}
