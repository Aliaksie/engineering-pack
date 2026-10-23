import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import { createHash } from 'node:crypto';
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

test('explicit members retain identities, independent coverage and cross-member import evidence', { skip: !binary }, t => {
  const left = fixture(t), right = fixture(t);
  const relative = path.relative(left.root, path.join(right.root, 'same.mjs')).split(path.sep).join('/');
  fs.writeFileSync(path.join(left.root, 'same.mjs'), `import '${relative}';\nexport const shared = 1;`);
  fs.writeFileSync(path.join(right.root, 'same.mjs'), 'export const shared = 2;');
  const members = [{ id: 'left', root: left.root }, { id: 'right', root: right.root }, { id: 'absent', root: path.join(left.root, 'missing') }];
  const refreshed = left.run('refresh', { members });
  assert.deepEqual(refreshed.members.map(m => m.state), ['available', 'available', 'unavailable']);
  const found = left.run('query', { members, value: 'shared', verify: true });
  const rows = found.members.slice(0, 2).map(m => m.result.results[0]);
  assert.equal(rows[0].path, rows[1].path);
  assert.notEqual(rows[0].workspaceId, rows[1].workspaceId);
  assert.deepEqual(rows.map(r => r.member), ['left', 'right']);
  const query = selection => left.run('query', { members: selection, field: 'target', value: relative, verify: true }).members[0].result.results[0];
  assert.equal(query(members).resolution.member, 'right');
  assert.equal(query(members).resolution.sourceFreshness, 'verified-selected-source');
  assert.equal(query(members.slice(0, 1)).resolution.state, 'unresolved');
  fs.writeFileSync(path.join(left.root, 'same.mjs'), 'export const changed = 1;');
  assert.equal(query(members).resolution.sourceFreshness, 'stale-or-unavailable');
  fs.unlinkSync(path.join(right.root, 'same.mjs'));
  assert.equal(query(members).resolution.state, 'unresolved');
  assert.throws(() => left.run('refresh', { members: [members[0], members[0]] }), /unique/);
  assert.throws(() => left.run('refresh', { members: [members[0], { id: 'alias', root: left.root }] }), /duplicate member root/);
});

test('native state creation rejects symlinked private directories', { skip: !binary || process.platform === 'win32' }, t => {
  const left = fixture(t), right = fixture(t);
  fs.symlinkSync(right.root, path.join(left.root, '.craft'));
  assert.throws(() => left.run('refresh'), /symlink/);
  assert.equal(fs.existsSync(path.join(right.root, 'state')), false);
});


test('corrupt databases fail without rebuilding or overwriting derived state', { skip: !binary }, t => {
  const { root, run } = fixture(t);
  run('status');
  const id = createHash('sha256').update(root).digest('hex').slice(0, 24);
  const database = path.join(root, '.craft', 'state', id, 'shared', 'project-evidence.sqlite');
  const corrupt = Buffer.from('not a SQLite database');
  fs.writeFileSync(database, corrupt);
  assert.throws(() => run('refresh'), /not a database/);
  assert.deepEqual(fs.readFileSync(database), corrupt);
});


test('backup is a consistent snapshot and rebuild only clears derived evidence', { skip: !binary }, t => {
  const { root, run } = fixture(t);
  fs.writeFileSync(path.join(root, 'a.js'), 'export const saved = 1;');
  run('refresh');
  const before = run('status');
  const snapshot = run('backup');
  assert.equal(snapshot.coverage.generation, before.generation);
  const database = snapshot.backup.slice(0, -'.backup'.length);
  const unrelated = path.join(path.dirname(database), 'session.json');
  fs.writeFileSync(unrelated, 'preserved');
  assert.throws(() => run('backup'), /already exists/);
  assert.equal(run('rebuild').coverage.sources, 0);
  assert.equal(run('query', { value: 'saved' }).results.length, 0);
  assert.equal(fs.readFileSync(unrelated, 'utf8'), 'preserved');
  assert.equal(fs.readFileSync(path.join(root, 'a.js'), 'utf8'), 'export const saved = 1;');
  assert.ok(run('status').generation > before.generation);
  fs.copyFileSync(snapshot.backup, database);
  assert.equal(run('query', { value: 'saved' }).results.length, 1);
  assert.equal(run('status').generation, before.generation);
  assert.equal(run('refresh').reused, 1);
});
