import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { execFileSync, spawn } from 'node:child_process';
import { once } from 'node:events';
import { setTimeout as delay } from 'node:timers/promises';
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
  // The native API owns canonical storage paths (including Windows path prefixes).
  const database = run('backup').backup.slice(0, -'.backup'.length);
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


test('busy refresh refuses concurrent queries and killed acquisition restarts safely', { skip: !binary }, async t => {
  const { root, run } = fixture(t);
  for (let index = 0; index < 1000; index++) fs.writeFileSync(path.join(root, `${index}.js`), `export const item${index} = ${index};`);
  const database = run('backup').backup.slice(0, -'.backup'.length);
  const child = spawn(binary, [], { stdio: ['pipe', 'pipe', 'pipe'] });
  const exited = once(child, 'exit');
  t.after(() => { if (child.exitCode === null) child.kill('SIGKILL'); });
  child.stdin.end(JSON.stringify({ formatVersion: 1, operation: 'refresh', input: { root } }));
  let busy = false;
  for (let attempt = 0; attempt < 200; attempt++) {
    if (fs.existsSync(database)) {
      try { run('status'); } catch (error) {
        if (/busy/.test(error.message)) { busy = true; break; }
        throw error;
      }
    }
    if (child.exitCode !== null) break;
    await delay(5);
  }
  assert.equal(busy, true, 'observe the native exclusive lock before interrupting');
  child.kill('SIGKILL');
  await exited;
  const partial = run('status');
  assert.equal(partial.inventoryComplete, false);
  const resumed = run('refresh');
  assert.equal(resumed.files, 1000);
  assert.equal(resumed.coverage.inventoryComplete, true);
  assert.equal(run('query', { value: 'item999' }).results.length, 1);
});

test('bounded extraction persists pending work, hides stale facts and advances past failures', { skip: !binary }, t => {
  const { root, run } = fixture(t);
  for (const name of ['a', 'b', 'c']) fs.writeFileSync(path.join(root, `${name}.mjs`), `export const ${name} = 1;`);
  run('refresh');
  fs.writeFileSync(path.join(root, 'a.mjs'), 'export const {');
  fs.writeFileSync(path.join(root, 'b.mjs'), 'export const replacement = 2;');
  fs.writeFileSync(path.join(root, 'c.mjs'), 'export const tail = 3;');
  const first = run('refresh', { maxFiles: 1 });
  assert.equal(first.failed, 1);
  assert.equal(first.deferred, 2);
  assert.equal(first.coverage.inventoryComplete, true);
  assert.equal(run('status').pendingSources, 2);
  assert.equal(run('query', { value: 'b' }).results.length, 0);
  const second = run('refresh', { maxFiles: 1 });
  assert.equal(second.changed, 1);
  assert.equal(second.deferred, 1);
  assert.equal(second.coverage.failedSources, 1);
  fs.unlinkSync(path.join(root, 'b.mjs'));
  const third = run('refresh', { maxFiles: 1 });
  assert.equal(third.changed, 1);
  assert.equal(third.removed, 1);
  assert.equal(third.coverage.pendingSources, 0);
  assert.equal(run('query', { value: 'tail' }).results.length, 1);
  assert.equal(run('query', { value: 'replacement' }).results.length, 0);
  for (const maxFiles of [0, -1, 1.5, '2']) assert.throws(() => run('refresh', { maxFiles }), /positive integer/);
});

test('byte batches skip oversized sources, bound failed attempts and resume without stale facts', { skip: !binary }, t => {
  const { root, run } = fixture(t);
  fs.writeFileSync(path.join(root, 'a.mjs'), 'export const old = 1;');
  run('refresh');
  const large = 'export const large = 1;' + ' '.repeat(100);
  const small = 'export const small = 1;';
  fs.writeFileSync(path.join(root, 'a.mjs'), large);
  fs.writeFileSync(path.join(root, 'b.mjs'), small);
  const first = run('refresh', { maxBytes: Buffer.byteLength(small) });
  assert.equal(first.changed, 1);
  assert.equal(first.extractionBytes, Buffer.byteLength(small));
  assert.deepEqual(first.deferredByLimit, { maxBytes: 1 });
  assert.equal(first.bytesScanned, Buffer.byteLength(large + small));
  assert.equal(run('query', { value: 'old' }).results.length, 0);
  assert.equal(run('query', { value: 'small' }).results.length, 1);
  assert.equal(run('refresh', { maxBytes: Buffer.byteLength(large) }).coverage.pendingSources, 0);
  fs.writeFileSync(path.join(root, 'a.mjs'), 'export {');
  fs.writeFileSync(path.join(root, 'b.mjs'), 'export const next = 2;');
  const failed = run('refresh', { maxBytes: 8 });
  assert.equal(failed.failed, 1);
  assert.equal(failed.extractionBytes, 8);
  assert.equal(failed.extractionAttempts, 1);
  assert.equal(failed.deferred, 1);
  assert.equal(run('refresh').changed, 1);
});

test('output batches publish complete sources, hide rejected stale facts and retry with larger limits', { skip: !binary }, t => {
  const { root, run } = fixture(t);
  fs.writeFileSync(path.join(root, 'a.mjs'), 'export const old = 1;');
  run('refresh');
  const source = 'export const first = 1; export const second = 2;';
  fs.writeFileSync(path.join(root, 'a.mjs'), source);
  const extracted = JSON.parse(execFileSync(binary, [], { input: JSON.stringify({ formatVersion: 1, operation: 'extract', input: { path: 'a.mjs', source } }), encoding: 'utf8' })).result;
  const size = Buffer.byteLength(JSON.stringify(extracted));
  const rejected = run('refresh', { maxOutputBytes: size - 1 });
  assert.equal(rejected.changed, 0);
  assert.equal(rejected.outputBytes, 0);
  assert.equal(rejected.extractionAttempts, 1);
  assert.deepEqual(rejected.deferredByLimit, { maxOutputBytes: 1 });
  for (const value of ['old', 'first', 'second']) assert.equal(run('query', { value }).results.length, 0);
  const accepted = run('refresh', { maxOutputBytes: size });
  assert.equal(accepted.changed, 1);
  assert.equal(accepted.outputBytes, size);
  assert.equal(accepted.coverage.pendingSources, 0);
  for (const value of ['first', 'second']) assert.equal(run('query', { value }).results.length, 1);
  const reused = run('refresh', { maxBytes: 1, maxOutputBytes: 1 });
  assert.equal(reused.reused, 1);
  assert.equal(reused.extractionAttempts, 0);
  assert.equal(reused.outputBytes, 0);
  fs.writeFileSync(path.join(root, 'b.mjs'), source);
  run('rebuild');
  const batch = run('refresh', { maxOutputBytes: size });
  assert.equal(batch.changed, 1);
  assert.equal(batch.deferred, 1);
  assert.equal(batch.outputBytes, size);
  assert.equal(run('refresh', { maxOutputBytes: size }).coverage.pendingSources, 0);
});

test('budget validation precedes state creation and limits are per member through CLI', { skip: !binary }, t => {
  const left = fixture(t), right = fixture(t);
  const members = [{ id: 'left', root: left.root }, { id: 'right', root: right.root }];
  for (const field of ['maxFiles', 'maxBytes', 'maxOutputBytes', 'maxDurationMs']) {
    for (const value of [0, -1, 1.5, '2', null]) {
      assert.throws(() => left.run('refresh', { [field]: value }), /positive integer/);
      assert.throws(() => left.run('refresh', { members, [field]: value }), /positive integer/);
    }
  }
  assert.equal(fs.existsSync(path.join(left.root, '.craft')), false);
  const source = 'export const item = 1;';
  const bytes = Buffer.byteLength(source);
  for (const { root } of [left, right]) fs.writeFileSync(path.join(root, 'a.js'), source);
  const result = JSON.parse(execFileSync(process.execPath, ['cli/project-evidence.mjs', 'refresh', '--enable', '--binary', binary,
    '--member', `left=${left.root}`, '--member', `right=${right.root}`, '--max-files', '1', '--max-bytes', String(bytes),
    '--max-output-bytes', '1024', '--max-duration-ms', '60000', '--json'], { encoding: 'utf8' }));
  for (const member of result.members) {
    assert.equal(member.result.changed, 1);
    assert.equal(member.result.extractionBytes, bytes);
    assert.equal(member.result.coverage.pendingSources, 0);
  }
});

test('Java syntax callers are cited independently of callee resolution and coexist with JS', { skip: !binary }, t => {
  const { root, run } = fixture(t);
  fs.writeFileSync(path.join(root, 'keep.js'), 'export const kept = 1;');
  const source = 'import example.Schema;\nclass Generator {\n String type(Schema s) { return schema(s); }\n String schema(Schema s) { return "string"; }\n}';
  fs.writeFileSync(path.join(root, 'Generator.java'), source);
  const first = run('refresh');
  assert.equal(first.changed, 2);
  assert.equal(first.coverage.sources, 2);
  assert.equal(run('version').providers.length, 2);
  const results = run('query', { value: 'schema', verify: true }).results;
  const call = results.find(row => row.evidence.kind === 'java:invokes-syntax');
  const declaration = results.find(row => row.evidence.kind === 'java:declares-method');
  assert.ok(call && declaration);
  assert.equal(call.verification, 'verified-selected-source');
  assert.equal(call.evidence.line, 3);
  assert.equal(call.evidence.target, 'schema');
  assert.notEqual(call.evidence.container, declaration.id);
  const caller = run('query', { field: 'id', value: call.evidence.container, verify: true }).results[0];
  assert.equal(caller.evidence.name, 'type');
  assert.equal(caller.evidence.kind, 'java:declares-method');
  assert.equal(caller.path, call.path);
  assert.equal(run('refresh').reused, 2);
  fs.writeFileSync(path.join(root, 'Generator.java'), source.replace('schema(s)', 'replacement(s)'));
  assert.equal(run('query', { field: 'id', value: call.id, verify: true }).results[0].verification, 'stale-or-unavailable');
  const changed = run('refresh');
  assert.equal(changed.changed, 1);
  assert.equal(changed.reused, 1);
  assert.equal(run('query', { field: 'id', value: call.id }).results.length, 0);
  fs.writeFileSync(path.join(root, 'Generator.java'), 'class Generator { String broken( }');
  assert.equal(run('refresh').failed, 1);
  assert.equal(run('query', { value: 'replacement' }).results.length, 0);
  assert.equal(run('query', { value: 'kept' }).results.length, 1);
  fs.unlinkSync(path.join(root, 'Generator.java'));
  assert.equal(run('refresh').removed, 1);
  assert.equal(run('status').failedSources, 0);
  assert.equal(run('status').sources, 1);
});

test('mixed providers share extraction budgets and retire deleted sources together', { skip: !binary }, t => {
  const { root, run } = fixture(t);
  fs.writeFileSync(path.join(root, 'a.java'), 'class A { void one() { two(); } }');
  fs.writeFileSync(path.join(root, 'b.js'), 'export const two = 1;');
  assert.equal(run('refresh', { maxFiles: 1 }).coverage.pendingSources, 1);
  const completed = run('refresh', { maxFiles: 1 });
  assert.equal(completed.reused, 1);
  assert.equal(completed.changed, 1);
  assert.equal(completed.coverage.pendingSources, 0);
  fs.unlinkSync(path.join(root, 'a.java'));
  fs.unlinkSync(path.join(root, 'b.js'));
  const deleted = run('refresh');
  assert.equal(deleted.removed, 2);
  assert.equal(deleted.coverage.sources, 0);
  assert.equal(run('query', { value: 'two' }).results.length, 0);
});

test('source-scoped queries filter before pagination and preserve exact member-relative paths', { skip: !binary }, t => {
  const { root, run } = fixture(t);
  fs.writeFileSync(path.join(root, 'noise.java'), `class Noise { void run() { ${'ping();'.repeat(210)} } void ping() {} }`);
  fs.writeFileSync(path.join(root, 'selected.java'), 'class Selected { void run() { ping(); ping(); ping(); } void ping() {} }');
  run('refresh');
  assert.ok(run('query', { value: 'ping', limit: 200 }).continuation);
  const rows = [];
  let after = '';
  do {
    const result = run('query', { value: 'ping', sourcePath: 'selected.java', limit: 1, after, verify: true });
    assert.equal(result.sourcePath, 'selected.java');
    rows.push(...result.results);
    after = result.continuation;
  } while (after);
  assert.equal(rows.length, 4);
  assert.equal(new Set(rows.map(row => row.id)).size, 4);
  assert.ok(rows.every(row => row.path === 'selected.java' && row.verification === 'verified-selected-source'));
  assert.equal(run('query', { value: 'ping', sourcePath: 'absent.java' }).results.length, 0);
  for (const sourcePath of ['../selected.java', '/selected.java', '', null, 7]) {
    assert.throws(() => run('query', { value: 'ping', sourcePath }), /relative source path/);
  }
  const cli = JSON.parse(execFileSync(process.execPath, ['cli/project-evidence.mjs', 'query', '--enable', '--binary', binary,
    '--root', root, '--value', 'ping', '--source', 'selected.java', '--json'], {encoding: 'utf8'}));
  assert.equal(cli.results.length, 4);
});

test('native refresh records FIFO sources as unavailable without waiting for a writer', { skip: !binary || process.platform === 'win32', timeout: 15000 }, t => {
  const { root, run } = fixture(t);
  const source = path.join(root, 'pipe.mjs');
  fs.writeFileSync(source, 'export const replacedByPipe = 1;');
  execFileSync('git', ['-C', root, 'add', 'pipe.mjs']);
  assert.equal(run('refresh').changed, 1);
  fs.unlinkSync(source);
  execFileSync('mkfifo', [source]);
  // The child deadline keeps this regression bounded even with the old reader.
  const invoke = (operation, input = {}) => projectEvidence({ enabled: true, root, binary, operation, input, timeoutMs: 3000 });
  assert.equal(invoke('query', { value: 'replacedByPipe', verify: true }).results[0].verification, 'stale-or-unavailable');
  assert.equal(invoke('refresh').failed, 1);
  assert.equal(invoke('query', { value: 'replacedByPipe' }).results.length, 0);
  assert.equal(invoke('status').sources, 1);
});
