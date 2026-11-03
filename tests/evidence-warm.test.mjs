import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { spawnSync } from 'node:child_process';
import { setTimeout as delay } from 'node:timers/promises';
import { nativeRequest } from './craft.mjs';

function fixture(t, project = null) {
  const root = fs.realpathSync(fs.mkdtempSync(path.join(os.tmpdir(), 'craft-warm-')));
  // Remove only after any background indexing of this fixture has stopped writing.
  t.after(async () => { await settled(root); fs.rmSync(root, { recursive: true, force: true }); });
  for (let i = 0; i < 12; i++) fs.writeFileSync(path.join(root, `m${i}.rs`), `pub fn f${i}() { g${i}(); }\nfn g${i}() {}\n`);
  if (project) { fs.mkdirSync(path.join(root, '.craft')); fs.writeFileSync(path.join(root, '.craft/project.json'), JSON.stringify(project)); }
  assert.equal(spawnSync('git', ['init', '-q'], { cwd: root }).status, 0);
  assert.equal(spawnSync('git', ['add', '-A'], { cwd: root }).status, 0);
  return root;
}
const indexed = root => {
  try { return nativeRequest('status', { root }).sources ?? 0; } catch { return null; }
};
async function settled(root, limitMs = 10_000) {
  const lock = path.join(root, '.craft');
  for (const start = Date.now(); Date.now() - start < limitMs; await delay(100)) {
    try { nativeRequest('status', { root }); return; } catch { /* busy while a chunk runs */ }
    if (!fs.existsSync(lock)) return;
  }
}

test('warming indexes the project in chunks and stops once nothing is deferred', t => {
  const root = fixture(t);
  const warmed = nativeRequest('warm', { root });
  assert.equal(warmed.warmed, true);
  assert.equal(warmed.reason, 'complete');
  assert.ok(warmed.chunks >= 1);
  assert.equal(indexed(root), 12);
  assert.equal(nativeRequest('warm', { root }).chunks, 1, 'a warm index takes one cheap pass');
});

test('the session start does not warm when the environment opts out', async t => {
  const root = fixture(t);
  const hook = spawnSync(process.env.CRAFT_NATIVE_BINARY, ['hook', 'claude', 'session-start', '--package', 'craft-delivery', '--handler', 'deliver', '--scope', 'any'],
    { cwd: root, input: JSON.stringify({ session_id: 'cold-session', cwd: root }), encoding: 'utf8', env: { ...process.env, CRAFT_EVIDENCE_WARM: '0' } });
  assert.equal(hook.status, 0, hook.stderr);
  await delay(1_000);
  assert.ok(!indexed(root), 'nothing indexed in the background');
});

test('warming respects a project that turned evidence off', t => {
  const root = fixture(t, { formatVersion: 1, evidence: { enabled: false } });
  assert.deepEqual(nativeRequest('warm', { root }), { warmed: false, reason: 'disabled', chunks: 0 });
});

test('a session start warms the index in the background without delaying the hook', { skip: process.platform === 'win32' && 'Windows has no detached start without unsafe handle changes' }, async t => {
  const root = fixture(t);
  const started = Date.now();
  const hook = spawnSync(process.env.CRAFT_NATIVE_BINARY, ['hook', 'claude', 'session-start', '--package', 'craft-delivery', '--handler', 'deliver', '--scope', 'any'],
    { cwd: root, input: JSON.stringify({ session_id: 'warm-session', cwd: root }), encoding: 'utf8', timeout: 10_000, env: { ...process.env, CRAFT_EVIDENCE_WARM: '' } });
  assert.equal(hook.status, 0, hook.stderr);
  assert.ok(Date.now() - started < 5_000, 'the hook returned while indexing continues');
  let sources = null;
  for (const start = Date.now(); Date.now() - start < 10_000 && sources !== 12; await delay(100)) sources = indexed(root);
  assert.equal(sources, 12);
});
