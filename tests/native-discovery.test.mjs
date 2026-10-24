import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { spawnSync } from 'node:child_process';
import { run } from '../cli/main.mjs';
import { readSessionState, updateSessionState, addActivity } from '../application/session/activity.mjs';
import { nativeCommand } from '../platform/node/native-command.mjs';
import { stateFile } from '../platform/index.mjs';
import { digest } from '../engine/cache/identity.mjs';

function fixture(t) {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'craft-discovery-native-'));
  t.after(() => fs.rmSync(root, { recursive: true, force: true }));
  const git = (...args) => { const child = spawnSync('git', ['-c', 'user.name=Fixture', '-c', 'user.email=fixture@example.test', '-c', 'commit.gpgsign=false', ...args], { cwd: root, encoding: 'utf8' }); assert.equal(child.status, 0, child.stderr); };
  git('init', '-q'); fs.writeFileSync(path.join(root, 'source.txt'), 'needle alpha\nneedle beta\n'); git('add', '.'); git('commit', '-qm', 'fixture');
  const binary = process.env.CRAFT_NATIVE_BINARY || path.resolve('target/debug/craft');
  const invoke = (request, bridge) => {
    const outcome = nativeCommand(['locate', '--root', root, '--session', 'session', '--task', 'task'], { binary, discovery: { ...bridge, request } });
    assert.equal(outcome.exitCode, 0, outcome.stderr);
    return JSON.parse(outcome.stdout);
  };
  return { root, binary, invoke };
}

test('native discovery writes compatible session receipts and refuses to overwrite a newer Node receipt', t => {
  const { root, invoke } = fixture(t);
  const request = { terms: ['needle'], paths: ['source.txt'] };
  const initial = readSessionState(root, 'session', 'task');
  assert.equal(invoke(request, { state: initial }).discovery.fresh, 1);
  const snapshot = readSessionState(root, 'session', 'task');
  assert.equal(snapshot.activity.freshLookups, 1);
  const file = stateFile(root, `session-${digest(['session', 'task']).slice(0, 24)}.json`, false, 'session');
  fs.writeFileSync(file, fs.readFileSync(file, 'utf8').replace('"formatVersion":1', '"formatVersion":1.0'));
  const reused = invoke(request, { state: snapshot });
  assert.equal(reused.discovery.reused, 1);
  assert.equal(reused.reuse.diagnostic, null);
  const stale = readSessionState(root, 'session', 'task');
  updateSessionState(root, 'session', 'task', state => addActivity(state, { routingDecisions: 1 }));
  const current = readSessionState(root, 'session', 'task');
  const result = invoke(request, { state: stale });
  assert.match(result.reuse.diagnostic, /state changed/);
  assert.deepEqual(readSessionState(root, 'session', 'task'), current);
});

test('native discovery respects the Node session lock and preserves deferred lookup without project reads', t => {
  const { root, invoke } = fixture(t);
  const state = readSessionState(root, 'session', 'task');
  const file = stateFile(root, `session-${digest(['session', 'task']).slice(0, 24)}.json`, true, 'session');
  fs.writeFileSync(`${file}.lock`, 'held by another writer');
  const result = invoke({ terms: ['needle'] }, { state });
  assert.match(result.reuse.diagnostic, /busy/);
  assert.equal(fs.existsSync(file), false);
  fs.mkdirSync(path.join(root, '.craft'), { recursive: true });
  fs.writeFileSync(path.join(root, '.craft/project.json'), '{invalid');
  const deferred = invoke({ requests: [{ id: 'a', term: 'needle', dependsOn: ['b'] }, { id: 'b', term: 'alpha', dependsOn: ['a'] }] }, {});
  assert.equal(deferred.discovery.batches, 0);
  assert.equal(deferred.deferred.length, 2);
});

test('locate CLI forwards bounded requests to the same native use case', t => {
  const { root, binary } = fixture(t);
  const argv = ['locate', '--root', root, '--term', 'needle', '--term', 'alpha', '--path', 'source.txt', '--max-files', '0x10', '--max-output-bytes', '4096'];
  const direct = spawnSync(binary, argv, { encoding: 'utf8' });
  assert.equal(direct.status, 0, direct.stderr);
  const result = JSON.parse(direct.stdout);
  assert.deepEqual(run(argv), result);
  assert.deepEqual(result.matches[0].termIndexes, [0, 1]);
  assert.equal(result.bounds.maxFiles, 16);
  assert.ok(Buffer.byteLength(JSON.stringify(result)) <= 4096);
  const unbound = spawnSync(binary, [...argv, '--session', 'session'], { encoding: 'utf8' });
  assert.equal(unbound.status, 2);
  assert.match(unbound.stderr, /validated task context/);
  assert.equal(unbound.stdout, '');
  for (const value of ['-1', 'Infinity', '1.5', 'invalid']) {
    assert.throws(() => run(['locate', '--root', root, '--term', 'needle', '--max-files', value]));
  }
});

test('malformed lookup packets cannot become reused navigation evidence', t => {
  const { root, invoke } = fixture(t);
  const request = { terms: ['needle'], paths: ['source.txt'] };
  invoke(request, { state: readSessionState(root, 'session', 'task') });
  updateSessionState(root, 'session', 'task', state => {
    for (const match of state.lookups[digest(['needle', ['source.txt']])].matches) delete match.snippetTruncated;
  });
  assert.equal(invoke(request, { state: readSessionState(root, 'session', 'task') }).discovery.reused, 1);
  updateSessionState(root, 'session', 'task', state => {
    state.lookups[digest(['needle', ['source.txt']])].matches = [1];
  });
  const result = invoke(request, { state: readSessionState(root, 'session', 'task') });
  assert.equal(result.discovery.reused, 0);
  assert.equal(result.discovery.fresh, 1);
  assert.ok(result.matches.every(match => match.path === 'source.txt'));
});

test('same-size edits invalidate packets even when the previous modification time is restored', t => {
  const { root, invoke } = fixture(t);
  const request = { terms: ['needle'], paths: ['source.txt'] };
  const snapshot = () => ({ state: readSessionState(root, 'session', 'task') });
  invoke(request, snapshot());
  assert.equal(invoke(request, snapshot()).discovery.reused, 1);
  const file = path.join(root, 'source.txt');
  const before = fs.statSync(file);
  fs.writeFileSync(file, 'needle gamma\nneedle zeta\n');
  assert.equal(fs.statSync(file).size, before.size);
  fs.utimesSync(file, before.atime, before.mtime);
  const changed = invoke(request, snapshot());
  assert.equal(changed.discovery.fresh, 1);
  assert.equal(changed.matches[0].text, 'needle gamma');
});

test('numeric CLI bounds apply equally to cached and fresh evidence', t => {
  const { root } = fixture(t);
  const args = ['locate', '--root', root, '--session', 'session', '--task', 'task', '--term', 'needle', '--path', 'source.txt'];
  assert.equal(run(args).matches.length, 2);
  const cached = run([...args, '--max-matches', '1']);
  assert.equal(cached.discovery.reused, 1);
  assert.equal(cached.matches.length, 1);
  assert.ok(cached.truncation.includes('maxMatches'));
});
