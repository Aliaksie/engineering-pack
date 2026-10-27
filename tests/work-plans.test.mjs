import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import { nativeCommand, run } from './craft.mjs';
import { hookPackage, runHook } from './hook-command.mjs';
import { readJson, stateLocation } from './state.mjs';

const repository = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');

function fixture(t) {
  const root = fs.realpathSync.native(fs.mkdtempSync(path.join(os.tmpdir(), 'craft-plans-')));
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

// The context Claude receives when `session` starts in `root`.
function started(root, session) {
  const result = runHook(hookPackage(path.join(repository, 'dist', 'claude'), 'claude', 'craft-delivery'), 'claude', 'restore',
    { cwd: root, input: JSON.stringify({ session_id: session, cwd: root, source: 'startup' }) });
  assert.equal(result.status, 0, result.stderr);
  return JSON.parse(result.stdout).hookSpecificOutput.additionalContext;
}

const listed = (context, prefix) => {
  const line = context.split('\n').find(text => text.startsWith(prefix));
  return line ? JSON.parse(line.slice(line.indexOf(': [') + 2)) : [];
};
const plans = context => listed(context, 'Open work plans');
const handoffs = context => listed(context, 'Unfinished tasks of earlier sessions');

const refused = argv => {
  const outcome = nativeCommand(argv);
  assert.notEqual(outcome.exitCode, 0, outcome.stdout);
  return outcome.stderr;
};

const step = (root, session, stage, next, extra = []) => run(['task-step', '--root', root, '--session', session,
  '--stage', stage, '--target', 'acceptance:value', '--approach', 'bounded-fix', '--evidence', `checks/${stage}`,
  '--outcome', 'passed', '--result', `${stage} done`, '--next', next, ...extra]);

const task = (plan, id) => plan.tasks.find(item => item.id === id);

test('a plan is worked on across sessions: claim, progress, readiness and take-over', t => {
  const root = fixture(t);
  const tasks = write(root, 'plan.json', { tasks: [
    { id: 'a', goal: 'Correct the value', files: ['value.mjs'], acceptance: ['value is 2'] },
    { id: 'b', goal: 'Document the value', dependsOn: ['a'] },
    { id: 'c', goal: 'Release the value', dependsOn: ['a', 'b'] },
  ] });
  const created = run(['plan-create', '--root', root, '--plan', 'b2', '--goal', 'Ship the value', '--reference', 'tracker:PROJ-1', '--data', tasks]);
  assert.equal(created.revision, 1);
  assert.deepEqual(created.tasks.map(item => [item.id, item.state, item.ready]), [['a', 'pending', true], ['b', 'pending', false], ['c', 'pending', false]]);
  const folder = path.join(stateLocation(root).directory, 'plans', 'b2');
  assert.deepEqual(fs.readdirSync(folder).sort(), ['plan.json', 'plan.md']);
  assert.match(refused(['plan-create', '--root', root, '--plan', 'b2', '--goal', 'Again']), /already exists/);

  // Session 1: B waits for A; A is claimed and worked to completion.
  assert.match(refused(['task-start', '--root', root, '--session', 'first', '--plan', 'b2', '--task', 'b']), /not ready: it waits for a/);
  const bound = run(['task-start', '--root', root, '--session', 'first', '--plan', 'b2', '--task', 'a']);
  assert.deepEqual([bound.plan.id, bound.plan.task, bound.plan.envelope.task.goal], ['b2', 'a', 'Correct the value']);
  const first = stateLocation(root, 'first').sessionId;
  assert.deepEqual(task(run(['plan', '--root', root, '--plan', 'b2']), 'a').owner.session, first);
  assert.equal(step(root, 'first', 'verify', 'Complete it').plan.state, 'verifying');
  assert.equal(step(root, 'first', 'verify', 'Nothing left', ['--complete']).plan.state, 'completed');
  const afterA = run(['plan', '--root', root, '--plan', 'b2']);
  assert.deepEqual(afterA.tasks.map(item => [item.id, item.state, item.ready]), [['a', 'completed', false], ['b', 'pending', true], ['c', 'pending', false]]);
  assert.deepEqual(task(afterA, 'a').evidence, ['checks/verify']);

  // Session 2 sees the plan at its start (not a handoff) and starts B.
  const second = started(root, 'second');
  assert.deepEqual(handoffs(second), []);
  assert.deepEqual(plans(second), [{ plan: 'b2', goal: 'Ship the value', ready: ['b'], held: [] }]);
  run(['task-start', '--root', root, '--session', 'second', '--plan', 'b2', '--task', 'b']);
  step(root, 'second', 'implement', 'Write the release note');

  // Session 3: B is held elsewhere; taking it over carries that session's record and checkpoint.
  const [shown] = plans(started(root, 'third'));
  assert.deepEqual(shown.held.map(item => [item.task, item.state, item.here]), [['b', 'running', false]]);
  assert.match(refused(['task-start', '--root', root, '--session', 'third', '--plan', 'b2', '--task', 'b']), /pass --take-over/);
  const taken = run(['task-start', '--root', root, '--session', 'third', '--plan', 'b2', '--task', 'b', '--take-over']);
  assert.equal(taken.plan.takenOver.from, stateLocation(root, 'second').sessionId);
  assert.equal(taken.plan.takenOver.checkpoint.saved.continuation.nextStep, 'Write the release note');
  const session = run(['session', '--root', root, '--session', 'third']);
  assert.equal(session.task.nextStep, 'Write the release note');
  assert.match(refused(['task-step', '--root', root, '--session', 'second', '--stage', 'implement', '--target', 'acceptance:value',
    '--approach', 'x', '--evidence', 'y', '--outcome', 'passed', '--result', 'r', '--next', 'n', '--bundle', repository]), /not held by this session/);
  step(root, 'third', 'implement', 'Publish the note');

  // People read the generated view; lookups by reference find the plan.
  const view = nativeCommand(['plan', '--root', root, '--plan', 'b2']);
  assert.equal(view.exitCode, 0, view.stderr);
  assert.match(view.stdout, /^# b2: Ship the value/);
  assert.match(view.stdout, /\| b \| running \| a \| [0-9a-f]{8} \| Publish the note \|/);
  const generated = fs.readFileSync(path.join(folder, 'plan.md'), 'utf8');
  assert.equal(view.stdout, `${generated}\nView: ${path.join(folder, 'plan.md')}\n`);
  assert.deepEqual(run(['plan', '--root', root, '--reference', 'tracker:PROJ-1']).plans.map(item => item.id), ['b2']);
  assert.deepEqual(run(['plan', '--root', root, '--reference', 'tracker:PROJ-2']).plans, []);
  assert.deepEqual(readJson(path.join(stateLocation(root).directory, 'plans', 'index.json')).plans.map(item => item.id), ['b2']);
});

test('plan changes are revision-fenced, acyclic and bounded by who holds a task', t => {
  const root = fixture(t);
  run(['plan-create', '--root', root, '--plan', 'p', '--goal', 'Outcome',
    '--data', write(root, 'tasks.json', { tasks: [{ id: 'a', goal: 'A' }, { id: 'b', goal: 'B', dependsOn: ['a'] }] })]);
  const update = (revision, changes, extra = []) => ['plan-update', '--root', root, '--plan', 'p', '--expected-revision', String(revision),
    '--data', write(root, `change-${revision}.json`, changes), ...extra];
  assert.match(refused(update(1, { tasks: [{ id: 'a', goal: 'A', dependsOn: ['b'] }] })), /cycle/);
  assert.match(refused(update(1, { tasks: [{ id: 'c', goal: 'C', dependsOn: ['z'] }] })), /unknown task/);
  const changed = run(update(1, { tasks: [{ id: 'c', goal: 'C', dependsOn: ['b'] }], decisions: [{ id: 'd1', text: 'Add C' }] }));
  assert.equal(changed.revision, 2);
  assert.match(refused(update(1, { goal: 'Stale' })), /changed \(revision 2 now, 1 expected\)/);
  assert.match(refused(update(2, { tasks: [{ id: 'a', state: 'running' }] })), /other states follow from task work/);

  run(['task-start', '--root', root, '--session', 'one', '--plan', 'p', '--task', 'a']);
  assert.match(refused(update(3, { tasks: [{ id: 'a', state: 'abandoned' }] }, ['--session', 'two'])), /owned by another session/);
  assert.match(refused(update(3, { status: 'completed' })), /every task is completed or abandoned/);
  const abandoned = run(update(3, { tasks: ['a', 'b', 'c'].map(id => ({ id, state: 'abandoned' })) }, ['--session', 'two', '--take-over']));
  assert.ok(abandoned.tasks.every(item => item.state === 'abandoned' && item.owner === null));
  assert.equal(run(update(4, { status: 'completed' })).status, 'completed');
  assert.match(refused(['task-start', '--root', root, '--session', 'one', '--plan', 'p', '--task', 'a']), /plan "p" is "completed"/);

  // Cleanup reports plans and keeps them within the retention period.
  const cleaned = run(['state-clean', '--root', root]);
  assert.deepEqual(cleaned.plans, [{ plan: 'p', status: 'completed', idleDays: 0, action: 'keep', reason: 'changed within the retention period' }]);
});

test('plan-less tasks keep their binding version and handoff', t => {
  const root = fixture(t);
  run(['task-start', '--root', root, '--session', 'first', '--task', 'fix', '--goal', 'Fix it']);
  const binding = readJson(path.join(stateLocation(root, 'first').directory, 'task.json'));
  assert.equal(binding.formatVersion, 1);
  assert.equal(binding.plan, undefined);
  assert.deepEqual(handoffs(started(root, 'second')).map(item => item.task), ['fix']);
  assert.deepEqual(plans(started(root, 'second')), []);
  assert.match(refused(['task-start', '--root', root, '--session', 'first', '--task', 'fix', '--goal', 'Fix it', '--take-over']), /--take-over requires --plan/);
});

test('cleanup removes long-closed plans and keeps sessions holding open plan tasks', t => {
  const root = fixture(t);
  const one = write(root, 'one.json', { tasks: [{ id: 'a', goal: 'A' }] });
  run(['plan-create', '--root', root, '--plan', 'open', '--goal', 'Still open', '--data', one]);
  run(['plan-create', '--root', root, '--plan', 'closed', '--goal', 'Closed', '--data', one]);
  run(['plan-update', '--root', root, '--plan', 'closed', '--expected-revision', '1',
    '--data', write(root, 'abandon.json', { tasks: [{ id: 'a', state: 'abandoned' }], status: 'abandoned' })]);
  run(['task-start', '--root', root, '--session', 'holder', '--plan', 'open', '--task', 'a']);
  run(['task-start', '--root', root, '--session', 'idle', '--task', 'other', '--goal', 'Other']);
  // Age the closed plan and both sessions past the retention period.
  const plans = path.join(stateLocation(root).directory, 'plans');
  const record = path.join(plans, 'closed', 'plan.json');
  const old = new Date(Date.now() - 30 * 86_400_000);
  fs.writeFileSync(record, JSON.stringify({ ...readJson(record), updated: old.toISOString() }));
  for (const session of ['holder', 'idle']) {
    const directory = stateLocation(root, session).directory;
    for (const name of fs.readdirSync(directory)) fs.utimesSync(path.join(directory, name), old, old);
    fs.utimesSync(directory, old, old);
  }
  const preview = run(['state-clean', '--root', root]);
  assert.deepEqual(preview.plans.map(item => [item.plan, item.action]), [['closed', 'remove'], ['open', 'keep']]);
  const kept = id => preview.sessions.find(item => item.session === stateLocation(root, id).sessionId);
  assert.equal(kept('holder').reason, 'holds a task of an open plan');
  assert.equal(kept('idle').action, 'remove');

  const applied = run(['state-clean', '--root', root, '--apply']);
  assert.deepEqual(applied.plans.map(item => [item.plan, item.action]), [['closed', 'removed'], ['open', 'keep']]);
  assert.deepEqual(fs.readdirSync(plans).sort(), ['index.json', 'open']);
  assert.deepEqual(run(['plan', '--root', root]).plans.map(item => item.id), ['open']);
});
