import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import test from 'node:test';
import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { saveCheckpoint, recoverCheckpoint } from '../application/checkpoint.mjs';

function fixture(t) {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'checkpoint-content-'));
  t.after(() => fs.rmSync(root, { recursive: true, force: true }));
  const git = (...args) => {
    const result = spawnSync('git', ['-C', root, '-c', 'user.name=Fixture', '-c', 'user.email=fixture@example.test', '-c', 'commit.gpgsign=false', ...args], { encoding: 'utf8' });
    assert.equal(result.status, 0, result.stderr);
    return result.stdout.trim();
  };
  const write = (name, text) => fs.writeFileSync(path.join(root, name), text);
  git('init', '-q');
  write('.gitignore', '.capabilities/state/\n');
  write('tracked.txt', 'base\n');
  git('add', '.'); git('commit', '-qm', 'fixture');
  return { root, git, write };
}

test('checkpoint detects another edit to an already modified tracked file', (t) => {
  const { root, write } = fixture(t);
  write('tracked.txt', 'first\n');
  const saved = saveCheckpoint(root, 'tracked').state;
  assert.equal(recoverCheckpoint(root, 'tracked').changed, false);
  write('tracked.txt', 'other\n');
  const recovered = recoverCheckpoint(root, 'tracked');
  assert.deepEqual(saved.status, recovered.current.status);
  assert.equal(recovered.changed, true);
  assert.equal(recovered.changeDetection.complete, true);
});

test('checkpoint detects content changes inside an untracked directory', (t) => {
  const { root, write } = fixture(t);
  fs.mkdirSync(path.join(root, 'new'));
  write('new/untracked.txt', 'first\n');
  const saved = saveCheckpoint(root, 'untracked').state;
  write('new/untracked.txt', 'other\n');
  const recovered = recoverCheckpoint(root, 'untracked');
  assert.deepEqual(saved.status, recovered.current.status);
  assert.equal(recovered.changed, true);
});

test('checkpoint fingerprints staged contents independently from working-tree contents', (t) => {
  const { root, git, write } = fixture(t);
  write('tracked.txt', 'index one\n'); git('add', 'tracked.txt');
  write('tracked.txt', 'working tree\n');
  const saved = saveCheckpoint(root, 'index').state;
  write('tracked.txt', 'index two\n'); git('add', 'tracked.txt');
  write('tracked.txt', 'working tree\n');
  const recovered = recoverCheckpoint(root, 'index');
  assert.deepEqual(saved.status, recovered.current.status);
  assert.equal(saved.fingerprints.workingTree, recovered.current.fingerprints.workingTree);
  assert.notEqual(saved.fingerprints.index, recovered.current.fingerprints.index);
  assert.equal(recovered.changed, true);
});

test('checkpoint reports unknown rather than unchanged when bounded inspection is incomplete', (t) => {
  const { root, write } = fixture(t);
  write('large.bin', Buffer.alloc(2 * 1024 * 1024 + 1));
  const saved = saveCheckpoint(root, 'bounded').state;
  assert.equal(saved.fingerprints.complete, false);
  const recovered = recoverCheckpoint(root, 'bounded');
  assert.equal(recovered.changeDetection.complete, false);
  assert.equal(recovered.changed, null);
  assert.match(recovered.changeDetection.reason, /incomplete/);
  assert.ok(recovered.current.fingerprints.issues.length > 0);
});

test('checkpoint marks failed Git inspection and legacy snapshots as incomplete', (t) => {
  const { root } = fixture(t);
  const saved = saveCheckpoint(root, 'legacy');
  const legacy = { ...saved.state };
  delete legacy.fingerprints;
  fs.writeFileSync(saved.file, JSON.stringify(legacy));
  assert.equal(recoverCheckpoint(root, 'legacy').changeDetection.complete, false);
  fs.renameSync(path.join(root, '.git'), path.join(root, 'git-unavailable'));
  const failed = recoverCheckpoint(root, 'legacy');
  assert.equal(failed.current.gitAvailable, false);
  assert.equal(failed.changeDetection.complete, false);
  assert.match(failed.current.fingerprints.issues.join('\n'), /could not be inspected/);
});

test('continuation checkpoints preserve bounded resume fields without conversation history', (t) => {
  const { root } = fixture(t);
  const continuation = Object.fromEntries(['goal', 'currentState', 'completed', 'approach', 'evidence', 'filesChanged', 'validation', 'remainingWork', 'blockers', 'nextStep'].map(key => [key, `${key}: identifier or concise summary`]));
  saveCheckpoint(root, 'resume', { continuation });
  assert.deepEqual(recoverCheckpoint(root, 'resume').saved.continuation, continuation);
  assert.deepEqual(saveCheckpoint(root, 'resume').state.continuation, continuation, 'automatic snapshots retain explicit continuation');
  assert.throws(() => saveCheckpoint(root, 'resume', { continuation: { ...continuation, transcript: 'old conversation' } }), /continuation/);
  assert.throws(() => saveCheckpoint(root, 'resume', { continuation: { goal: 'missing fields' } }), /continuation/);
  assert.throws(() => saveCheckpoint(root, 'resume', { continuation: Object.fromEntries(Object.keys(continuation).map(key => [key, 'x'.repeat(1000)])) }), /8 KiB/);
});
