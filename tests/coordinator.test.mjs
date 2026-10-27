import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import { nativeCommand, run } from './craft.mjs';
import { readJson, stateLocation } from './state.mjs';

const repository = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');

function fixture(t) {
  const root = fs.realpathSync.native(fs.mkdtempSync(path.join(os.tmpdir(), 'craft-coordinator-')));
  t.after(() => fs.rmSync(root, { recursive: true, force: true }));
  const git = (...args) => assert.equal(spawnSync('git', ['-C', root, '-c', 'core.hooksPath=/dev/null', '-c', 'commit.gpgsign=false',
    '-c', 'user.name=Fixture', '-c', 'user.email=fixture@example.test', ...args]).status, 0);
  git('init', '-q');
  fs.writeFileSync(path.join(root, 'value.mjs'), 'export const value = 1;\n');
  git('add', '.'); git('commit', '-qm', 'fixture');
  return root;
}

const write = (root, name, value) => {
  const file = path.join(root, '..', `${path.basename(root)}-${name}`);
  fs.writeFileSync(file, JSON.stringify(value));
  return file;
};

const refused = argv => {
  const outcome = nativeCommand(argv[0] === 'task-step' ? [...argv, '--bundle', repository] : argv);
  assert.notEqual(outcome.exitCode, 0, outcome.stdout);
  return outcome.stderr;
};

function coordinated(t, tasks) {
  const root = fixture(t);
  run(['plan-create', '--root', root, '--plan', 'q', '--goal', 'Coordinate', '--data', write(root, 'plan.json', { tasks })]);
  const record = path.join(stateLocation(root).directory, 'plans', 'q', 'plan.json');
  const next = session => run(['plan-next', '--root', root, '--plan', 'q', ...(session ? ['--session', session] : [])]);
  const ready = session => run(['task-start', '--root', root, '--session', session, '--plan', 'q', '--ready']);
  const start = (session, task, extra = []) => run(['task-start', '--root', root, '--session', session, '--plan', 'q', '--task', task, ...extra]);
  const step = (session, outcome, extra = []) => run(['task-step', '--root', root, '--session', session, '--stage', 'verify',
    '--target', 'acceptance:q', '--approach', 'bounded-fix', '--evidence', 'checks/q', '--outcome', outcome,
    '--result', 'r', '--next', 'Continue', ...extra]);
  const update = (session, changes) => run(['plan-update', '--root', root, '--plan', 'q', '--session', session,
    '--expected-revision', String(readJson(record).revision), '--data', write(root, 'change.json', changes)]);
  const task = id => readJson(record).tasks.find(item => item.id === id);
  return { root, record, next, ready, start, step, update, task };
}

test('sessions take ready tasks in order, release them and see completions propagate', t => {
  const { root, record, next, ready, step, task } = coordinated(t, [
    { id: 'a', goal: 'First' }, { id: 'b', goal: 'Second' }, { id: 'c', goal: 'Third', dependsOn: ['a'] },
  ]);
  const queued = next('one');
  assert.equal(queued.next, 'a');
  assert.deepEqual(queued.ready, ['a', 'b']);
  assert.deepEqual(queued.waiting, [{ task: 'c', on: [{ id: 'a', state: 'pending' }], stuck: false }]);
  const receipt = nativeCommand(['plan-next', '--root', root, '--plan', 'q']);
  assert.match(receipt.stdout, /^Next: a — craft task-start --plan q --task a \(or --ready\)$/m);
  assert.match(receipt.stdout, /^Waiting: c \(on a pending\)$/m);

  // Two sessions asking for the next ready task get different ones.
  assert.equal(ready('one').task, 'a');
  assert.equal(ready('two').task, 'b');
  assert.equal(task('a').attempts, 1);
  assert.equal(readJson(record).formatVersion, 3);
  assert.deepEqual(next('two').elsewhere.map(item => [item.task, item.stale, item.orphaned]), [['a', false, false]]);
  assert.match(refused(['task-start', '--root', root, '--session', 'three', '--plan', 'q', '--ready']), /no ready task \(0 held here, 2 elsewhere, 1 waiting/);

  // Completion records what verified it and readies the dependent without a write to it.
  step('one', 'passed', ['--complete']);
  const verified = task('a').verified;
  assert.match(verified.revision, /^[0-9a-f]{40}$/);
  assert.equal(verified.contract, null);
  assert.deepEqual(next('one').ready, ['c']);

  // Releasing returns the task to pending and removes the session's binding to it.
  assert.match(refused(['plan-release', '--root', root, '--plan', 'q', '--task', 'b', '--session', 'one']), /--take-over/);
  const released = run(['plan-release', '--root', root, '--plan', 'q', '--task', 'b', '--session', 'two', '--reason', 'Needs a decision']);
  assert.deepEqual([released.state, released.unbound], ['pending', true]);
  assert.equal(task('b').nextStep, 'Needs a decision');
  assert.equal(task('b').attempts, 1);
  assert.match(refused(['task-step', '--root', root, '--session', 'two', '--stage', 'verify', '--target', 'acceptance:q',
    '--approach', 'x', '--evidence', 'y', '--outcome', 'passed', '--result', 'r', '--next', 'n']), /task-start binding/);
  assert.deepEqual(next('two').ready, ['b', 'c']);
  assert.match(fs.readFileSync(path.join(path.dirname(record), 'plan.md'), 'utf8'), /Verified: [0-9a-f]{40} at /);
  assert.match(refused(['plan-next', '--root', root, '--plan', 'q', '--ready']), /--ready is supported by task-start/);
});

test('a task is retried within its attempt budget and a spent session budget fails it', t => {
  const { root, next, start, step, update, task } = coordinated(t, [{ id: 'a', goal: 'Flaky' }, { id: 'b', goal: 'Hard' }]);
  for (let attempt = 1; attempt <= 3; attempt += 1) {
    start('one', 'a');
    update('one', { tasks: [{ id: 'a', state: 'failed' }] });
    if (attempt < 3) assert.deepEqual(next('one').retry, [{ task: 'a', state: 'failed', attempts: attempt, maxAttempts: 3 }]);
    update('one', { tasks: [{ id: 'a', state: 'pending' }] });
  }
  assert.deepEqual(next('one').exhausted, [{ task: 'a', state: 'pending', attempts: 3, maxAttempts: 3 }]);
  assert.match(refused(['task-start', '--root', root, '--session', 'one', '--plan', 'q', '--task', 'a']), /attempt budget exhausted \(3 of 3\)/);
  update('one', { tasks: [{ id: 'a', maxAttempts: 4 }] });
  assert.equal(start('one', 'a').plan.task, 'a');
  assert.equal(task('a').attempts, 4);

  // Failed checks keep the task running until the session's attempt budget is spent.
  start('one', 'b');
  const states = [1, 2, 3].map(() => step('one', 'quality-failure').plan.state);
  assert.deepEqual(states, ['running', 'running', 'failed']);
  assert.equal(task('b').owner, null);
  assert.deepEqual(next('one').retry.map(item => item.task), ['b']);
});

test('stale and orphaned claims are shown and continue only by explicit take-over', t => {
  const { root, record, next, start, task } = coordinated(t, [{ id: 'a', goal: 'Long' }, { id: 'b', goal: 'Lost' }]);
  start('one', 'a');
  start('two', 'b');
  // Three hours without a step on a: stale, never released by age.
  const saved = readJson(record);
  const earlier = new Date(Date.now() - 3 * 3600000).toISOString();
  Object.assign(saved.tasks[0], { updated: earlier });
  saved.tasks[0].owner.since = earlier;
  fs.writeFileSync(record, JSON.stringify(saved));
  fs.rmSync(stateLocation(root, 'two').directory, { recursive: true, force: true });
  const shown = next('three');
  assert.deepEqual(shown.elsewhere.map(item => [item.task, item.stale, item.orphaned]), [['a', true, false], ['b', false, true]]);
  const receipt = nativeCommand(['plan-next', '--root', root, '--plan', 'q', '--session', 'three']);
  assert.match(receipt.stdout, /^Held elsewhere: a \(running, idle 180 min, stale\); b \(running, idle 0 min, orphaned\)$/m);
  assert.match(receipt.stdout, /--take-over$/m);
  assert.equal(task('a').owner.session, stateLocation(root, 'one').sessionId);
  assert.match(refused(['task-start', '--root', root, '--session', 'three', '--plan', 'q', '--task', 'b']), /--take-over/);
  const taken = start('three', 'b', ['--take-over']);
  assert.deepEqual(taken.plan.takenOver, { from: stateLocation(root, 'two').sessionId, carried: false,
    reason: 'the holding session left no record of this task' });
  assert.equal(task('b').attempts, 1);
});
