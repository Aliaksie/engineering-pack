import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { execFileSync } from 'node:child_process';
import { projectEvidence } from '../application/project-evidence.mjs';

test('disabled evidence never requires a repository or native binary', () => {
  assert.equal(projectEvidence({ enabled: false, binary: '/missing', root: '/missing' }).enabled, false);
});

const binary = process.env.CRAFT_PROJECT_EVIDENCE_BINARY;
function fixture(t) {
  const root = fs.realpathSync(fs.mkdtempSync(path.join(os.tmpdir(), 'craft-evidence-')));
  t.after(() => fs.rmSync(root, { recursive: true, force: true }));
  execFileSync('git', ['init', '--quiet', root]);
  const run = (operation, input = {}) => projectEvidence({ enabled: true, root, binary, operation, input });
  return { root, run };
}

test('native refresh, reuse, failure, deletion and selected-source verification', { skip: !binary }, t => {
  const { root, run } = fixture(t);
  fs.writeFileSync(path.join(root, 'a.mjs'), "import './b.mjs';\nexport const before = 1;\n");
  fs.writeFileSync(path.join(root, 'b.mjs'), 'export const other = 2;');
  assert.equal(run('refresh').changed, 2);
  const found = run('query', { field: 'name', value: 'before', verify: true });
  assert.equal(found.results.length, 1);
  assert.equal(found.results[0].verification, 'verified-selected-source');
  assert.equal(found.results[0].evidence.line, 2);
  assert.equal(run('refresh').reused, 2);
  fs.writeFileSync(path.join(root, 'a.mjs'), 'export const after = 1;');
  assert.equal(run('query', { value: 'before', verify: true }).results[0].verification, 'stale-or-unavailable');
  assert.equal(run('refresh').changed, 1);
  assert.equal(run('query', { value: 'before' }).results.length, 0);
  fs.writeFileSync(path.join(root, 'a.mjs'), 'export const {');
  assert.equal(run('refresh').failed, 1);
  assert.equal(run('query', { value: 'after' }).results.length, 0);
  fs.writeFileSync(path.join(root, 'a.mjs'), '// empty');
  assert.equal(run('refresh').coverage.failedSources, 0);
  fs.unlinkSync(path.join(root, 'b.mjs'));
  assert.equal(run('refresh').removed, 1);
  assert.equal(run('query', { value: 'other' }).results.length, 0);
  assert.equal(run('status').sources, 1);
});

test('native inventory reaches beyond ten thousand files and a two MiB listing', { skip: !binary || !process.env.CRAFT_EVIDENCE_LARGE_SMOKE }, t => {
  const { root, run } = fixture(t);
  const prefix = 'source-'.padEnd(210, 'x');
  for (let index = 0; index < 10001; index++) fs.writeFileSync(path.join(root, `${prefix}-${String(index).padStart(5, '0')}.mjs`), `export const symbol${index} = ${index};`);
  const result = run('refresh');
  assert.equal(result.files, 10001);
  assert.equal(result.changed, 10001);
  assert.equal(result.coverage.inventoryComplete, true);
  assert.equal(run('query', { value: 'symbol10000' }).results.length, 1);
  assert.equal(run('refresh').reused, 10001);
});
