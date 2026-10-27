import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { spawnSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import { saveCheckpoint } from './checkpoint-cli.mjs';
import { run } from './craft.mjs';
import { digest, readTelemetry, stateLocation } from './state.mjs';

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
  const worktree = stateLocation(root).previousDirectory;
  const previous = path.join(worktree, '0123456789abcdef01234567');
  fs.mkdirSync(previous, { recursive: true });
  fs.writeFileSync(path.join(previous, '0123456789abcdef01234567.json'), '{}\n');
  age(previous, 30);
  fs.mkdirSync(path.join(worktree, 'shared'), { recursive: true });
  fs.writeFileSync(path.join(worktree, 'shared', 'notes.txt'), '{}\n');

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
  assert.deepEqual(preview.previousShared, [{ name: 'notes.txt', directory: false, bytes: 3, action: 'keep', reason: 'unrecognized' }]);
  assert.ok(fs.existsSync(idle.directory) && fs.existsSync(previous), 'a preview removes nothing');

  const applied = run(['state-clean', '--root', root, '--apply']);
  assert.equal(applied.status, 'applied');
  assert.equal(applied.summary.removed, 2);
  assert.equal(bySession(applied)[idle.sessionId].action, 'removed');
  assert.ok(!fs.existsSync(idle.directory) && !fs.existsSync(previous));
  assert.ok(fs.existsSync(recent.directory) && fs.existsSync(held.directory));
  assert.ok(fs.existsSync(path.join(worktree, 'shared', 'notes.txt')), 'an unrecognized shared file is kept');
  assert.deepEqual(fs.readdirSync(path.dirname(idle.directory)).filter(name => name.startsWith('.sweep-')), []);
  assert.equal(run(['state-clean', '--root', root, '--apply']).summary.removed, 0);
});

test('state-clean measures current shared families and never removes them', t => {
  const root = fixture(t);
  saveCheckpoint(root, 'measured', { nextStep: 'continue' });
  const shared = stateLocation(root).directory;
  fs.mkdirSync(path.join(shared, 'intelligence'), { recursive: true });
  fs.writeFileSync(path.join(shared, 'intelligence', 'project.sqlite'), 'x'.repeat(10));
  fs.mkdirSync(path.join(shared, 'workspaces'), { recursive: true });
  fs.symlinkSync(os.tmpdir(), path.join(shared, 'workspaces', 'linked'));
  const families = result => Object.fromEntries(result.shared.map(item => [item.family, item]));

  const reported = families(run(['state-clean', '--root', root, '--apply']));
  assert.deepEqual(Object.keys(reported), ['telemetry', 'intelligence', 'workspaces', 'migrations']);
  assert.deepEqual([reported.intelligence.bytes, reported.intelligence.files], [10, 1]);
  assert.match(reported.intelligence.bounded, /rebuild/);
  assert.ok(reported.telemetry.bytes > 0, 'the command itself records telemetry');
  assert.equal(reported.workspaces.bytes, undefined);
  assert.match(reported.workspaces.reason, /symlink/);
  assert.equal(fs.readFileSync(path.join(shared, 'intelligence', 'project.sqlite'), 'utf8'), 'x'.repeat(10));
  assert.ok(fs.lstatSync(path.join(shared, 'workspaces', 'linked')).isSymbolicLink());
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

test('state-clean removes what the previous layout left in shared/ once nothing depends on it', t => {
  const root = fixture(t);
  const shared = path.join(stateLocation(root).previousDirectory, 'shared');
  const sessions = path.join(shared, 'sessions');
  const write = (name, text) => {
    fs.mkdirSync(path.dirname(path.join(shared, name)), { recursive: true });
    fs.writeFileSync(path.join(shared, name), text);
  };
  write('telemetry.jsonl', '{"schemaVersion":1,"previous":true}\n');
  write('telemetry.jsonl.1', '{"schemaVersion":1,"rotated":true}\n');
  write('telemetry-captures.json', '{"captures":[]}\n');
  write('migration-0123456789abcdef01234567.json', '{}\n');
  write('project-evidence.sqlite', 'not rebuilt\n');
  write('project-evidence.sqlite.lock', '');
  write('workspace-guard.json', '{}\n');
  // The evidence folder of the layout before the current one is not moved.
  write(path.join('evidence', 'project.sqlite'), 'not moved\n');
  // A closed workspace whose evidence retention expired long ago, and one nobody can verify.
  const sha256 = text => createHash('sha256').update(text).digest('hex');
  const session = digest('w'), task = digest('t'), entry = sha256(`${session}:${task}`);
  write(path.join('sessions', entry, 'owner.json'), JSON.stringify({ owner: 'craft', formatVersion: 1,
    project: sha256(fs.realpathSync.native(root)), session, task, updated: 0, closed: true, operations: {}, evidence: [] }));
  write(path.join('sessions', entry, 'evidence', 'result.txt'), 'old evidence');
  write(path.join('sessions', 'f'.repeat(64), 'owner.json'), '{"owner":"someone-else"}');
  // Any command copies the telemetry log into the current layout.
  saveCheckpoint(root, 'current', {});

  const actions = result => Object.fromEntries(result.previousShared.map(item => [item.name, [item.action, item.reason]]));
  const preview = actions(run(['state-clean', '--root', root]));
  assert.deepEqual(preview, {
    evidence: ['remove', undefined],
    'migration-0123456789abcdef01234567.json': ['remove', undefined],
    'project-evidence.sqlite': ['remove', undefined],
    'project-evidence.sqlite.lock': ['remove', undefined],
    sessions: ['keep', '1 workspace(s) open, unverified or within evidence retention'],
    'telemetry-captures.json': ['keep', 'not copied yet; it is copied when first used'],
    'telemetry.jsonl': ['remove', undefined],
    'telemetry.jsonl.1': ['remove', undefined],
    'workspace-guard.json': ['keep', 'a workspace of the previous layout remains'],
  });
  assert.ok(fs.existsSync(path.join(shared, 'telemetry.jsonl')), 'a preview removes nothing');

  const applied = actions(run(['state-clean', '--root', root, '--apply']));
  assert.deepEqual(applied['telemetry.jsonl'], ['removed', undefined]);
  assert.deepEqual(applied['project-evidence.sqlite'], ['removed', undefined]);
  assert.deepEqual(applied['workspace-guard.json'][0], 'keep');
  assert.deepEqual(applied.evidence, ['removed', undefined]);
  assert.deepEqual(fs.readdirSync(shared).sort(), ['sessions', 'telemetry-captures.json', 'workspace-guard.json']);
  assert.deepEqual(fs.readdirSync(sessions), ['f'.repeat(64)], 'the unverified workspace is kept');

  // The copied log stays current, and an older store cannot become its source again.
  const before = readTelemetry(root).length;
  saveCheckpoint(root, 'current', { nextStep: 'again' });
  assert.ok(readTelemetry(root).length > before);
  assert.ok(readTelemetry(root).some(event => event.previous), 'copied events are kept');

  fs.rmSync(path.join(sessions, 'f'.repeat(64)), { recursive: true });
  const last = actions(run(['state-clean', '--root', root, '--apply']));
  assert.deepEqual([last.sessions, last['workspace-guard.json']], [['removed', undefined], ['removed', undefined]]);
  assert.deepEqual(fs.readdirSync(shared).sort(), ['telemetry-captures.json']);
});

test('a cleanup interrupted while removing files is finished, not restored', t => {
  const root = fixture(t);
  saveCheckpoint(root, 'done', {});
  const done = stateLocation(root, 'done');
  age(done.directory, 20);
  // Set aside and partly removed: the directory's own time is now the cleanup's.
  const staged = path.join(path.dirname(done.directory), `.sweep-${done.sessionId}`);
  fs.renameSync(done.directory, staged);
  fs.rmSync(path.join(staged, 'checkpoint.json'));
  const resumed = bySession(run(['state-clean', '--root', root, '--apply']))[done.sessionId];
  assert.deepEqual([resumed.action, resumed.staged], ['removed', true]);
  assert.ok(!fs.existsSync(staged) && !fs.existsSync(done.directory));

  // Nothing left but the staged directory itself.
  saveCheckpoint(root, 'empty', {});
  const empty = stateLocation(root, 'empty');
  const emptied = path.join(path.dirname(empty.directory), `.sweep-${empty.sessionId}`);
  fs.renameSync(empty.directory, emptied);
  for (const name of fs.readdirSync(emptied)) fs.rmSync(path.join(emptied, name));
  assert.equal(bySession(run(['state-clean', '--root', root, '--apply']))[empty.sessionId].action, 'removed');
  assert.ok(!fs.existsSync(emptied) && !fs.existsSync(empty.directory));
});

test('a record held open during cleanup is removed once released', t => {
  const root = fixture(t);
  saveCheckpoint(root, 'open-file', {});
  const held = stateLocation(root, 'open-file');
  age(held.directory, 20);
  const staged = path.join(path.dirname(held.directory), `.sweep-${held.sessionId}`);
  // Windows refuses to rename or remove what another process holds open; POSIX does not.
  const descriptor = fs.openSync(path.join(held.directory, 'checkpoint.json'), 'r');
  let first;
  try {
    first = bySession(run(['state-clean', '--root', root, '--apply']))[held.sessionId];
  } finally {
    fs.closeSync(descriptor);
  }
  if (first.action === 'removed') assert.ok(!fs.existsSync(staged) && !fs.existsSync(held.directory));
  else assert.equal(typeof first.reason, 'string');
  const second = bySession(run(['state-clean', '--root', root, '--apply']))[held.sessionId];
  assert.ok(first.action === 'removed' ? second === undefined : second.action === 'removed', JSON.stringify(second));
  assert.ok(!fs.existsSync(staged) && !fs.existsSync(held.directory));
});
