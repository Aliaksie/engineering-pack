import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { spawnSync } from 'node:child_process';
import { saveCheckpoint } from './checkpoint-cli.mjs';
import { run } from './craft.mjs';
import { stateLocation } from './state.mjs';

const DAY = 86_400_000;

function fixture(t) {
  const root = fs.realpathSync.native(fs.mkdtempSync(path.join(os.tmpdir(), 'craft-state-clean-')));
  t.after(() => fs.rmSync(root, { recursive: true, force: true }));
  const git = (...args) => assert.equal(spawnSync('git', ['-C', root, '-c', 'core.hooksPath=/dev/null', '-c', 'commit.gpgsign=false',
    '-c', 'user.name=Fixture', '-c', 'user.email=fixture@example.test', ...args]).status, 0);
  git('init', '-q');
  fs.writeFileSync(path.join(root, 'value.mjs'), 'export const value = 1;\n');
  git('add', '.'); git('commit', '-qm', 'fixture');
  return root;
}

// Date a directory and everything in it `days` back.
function age(directory, days) {
  const when = new Date(Date.now() - days * DAY);
  for (const name of fs.readdirSync(directory)) fs.utimesSync(path.join(directory, name), when, when);
  fs.utimesSync(directory, when, when);
}

const bySession = result => Object.fromEntries(result.sessions.map(item => [item.session, item]));

test('state-clean previews, then removes only idle unheld session directories', t => {
  const root = fixture(t);
  for (const session of ['idle', 'recent', 'held']) saveCheckpoint(root, session, { nextStep: 'continue' });
  const idle = stateLocation(root, 'idle'), recent = stateLocation(root, 'recent'), held = stateLocation(root, 'held');
  fs.writeFileSync(path.join(held.directory, 'checkpoint.json.lock'), '');
  age(idle.directory, 15); age(held.directory, 15); age(recent.directory, 13);
  // A session directory of the previous layout and a shared record it left behind.
  const worktree = path.dirname(idle.directory.replace(`${path.sep}sessions${path.sep}`, path.sep));
  const previous = path.join(worktree, '0123456789abcdef01234567');
  fs.mkdirSync(previous);
  fs.writeFileSync(path.join(previous, '0123456789abcdef01234567.json'), '{}\n');
  age(previous, 30);
  fs.mkdirSync(path.join(worktree, 'shared'), { recursive: true });
  fs.writeFileSync(path.join(worktree, 'shared', 'telemetry.jsonl'), '{}\n');

  const preview = run(['state-clean', '--root', root]);
  assert.equal(preview.status, 'preview');
  assert.equal(preview.retentionDays, 14);
  assert.equal(preview.complete, true);
  assert.equal(preview.summary.removable, 2);
  const planned = bySession(preview);
  assert.equal(planned[idle.sessionId].action, 'remove');
  assert.equal(planned[idle.sessionId].idleDays, 15);
  assert.equal(planned['0123456789abcdef01234567'].layout, 'previous');
  assert.equal(planned['0123456789abcdef01234567'].action, 'remove');
  assert.deepEqual([planned[recent.sessionId].action, planned[recent.sessionId].reason], ['keep', 'written within the retention period']);
  assert.deepEqual([planned[held.sessionId].action, planned[held.sessionId].reason], ['keep', 'held by a writer']);
  assert.deepEqual(preview.previousShared, [{ name: 'telemetry.jsonl', directory: false, bytes: 3 }]);
  assert.ok(fs.existsSync(idle.directory) && fs.existsSync(previous), 'a preview removes nothing');

  const applied = run(['state-clean', '--root', root, '--apply']);
  assert.equal(applied.status, 'applied');
  assert.equal(applied.summary.removed, 2);
  assert.equal(bySession(applied)[idle.sessionId].action, 'removed');
  assert.ok(!fs.existsSync(idle.directory) && !fs.existsSync(previous));
  assert.ok(fs.existsSync(recent.directory) && fs.existsSync(held.directory));
  assert.ok(fs.existsSync(path.join(worktree, 'shared', 'telemetry.jsonl')), 'shared records are only reported');
  assert.deepEqual(fs.readdirSync(path.dirname(idle.directory)).filter(name => name.startsWith('.sweep-')), []);
  assert.equal(run(['state-clean', '--root', root, '--apply']).summary.removed, 0);
});

test('an open workspace keeps its idle session, and an interrupted cleanup is finished', t => {
  const root = fixture(t);
  const begin = session => {
    run(['task-start', '--root', root, '--session', session, '--task', 'task', '--goal', 'Keep scratch']);
    return run(['workspace-open', '--root', root, '--session', session]);
  };
  assert.equal(begin('open').status, 'available');
  const open = stateLocation(root, 'open');
  age(open.directory, 20);
  saveCheckpoint(root, 'done', {});
  const done = stateLocation(root, 'done');
  age(done.directory, 20);
  const preview = bySession(run(['state-clean', '--root', root]));
  assert.deepEqual([preview[open.sessionId].action, preview[open.sessionId].reason], ['keep', 'a workspace may still be open']);
  assert.equal(preview[done.sessionId].action, 'remove');

  // A cleanup interrupted after setting the directory aside resumes on the next run.
  const staged = path.join(path.dirname(done.directory), `.sweep-${done.sessionId}`);
  fs.renameSync(done.directory, staged);
  const resumed = bySession(run(['state-clean', '--root', root, '--apply']))[done.sessionId];
  assert.deepEqual([resumed.action, resumed.staged], ['removed', true]);
  assert.ok(!fs.existsSync(staged) && !fs.existsSync(done.directory));
  assert.ok(fs.existsSync(open.directory));
});
