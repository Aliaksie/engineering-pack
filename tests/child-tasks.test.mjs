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
  const root = fs.realpathSync.native(fs.mkdtempSync(path.join(os.tmpdir(), 'craft-children-')));
  t.after(() => fs.rmSync(root, { recursive: true, force: true }));
  const git = (...args) => assert.equal(spawnSync('git', ['-C', root, '-c', 'core.hooksPath=/dev/null', '-c', 'commit.gpgsign=false',
    '-c', 'user.name=Fixture', '-c', 'user.email=fixture@example.test', ...args]).status, 0);
  git('init', '-q');
  for (const file of ['src/a/api.mjs', 'src/b/use.mjs', 'README.md', 'value.mjs']) {
    fs.mkdirSync(path.dirname(path.join(root, file)), { recursive: true });
    fs.writeFileSync(path.join(root, file), 'export const value = 1;\n');
  }
  git('add', '.'); git('commit', '-qm', 'fixture');
  return { root, git };
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

const edit = (root, file) => fs.appendFileSync(path.join(root, file), '// changed\n');

test('a parent works its children in turn within owned paths and their returned results', t => {
  const { root, git } = fixture(t);
  // An uncommitted change that predates the claims is not attributed to any task.
  edit(root, 'README.md');
  run(['plan-create', '--root', root, '--plan', 's5', '--goal', 'Split the value', '--reference', 'tracker:PROJ-5',
    '--data', write(root, 'plan.json', { tasks: [
      { id: 'p', goal: 'Split the value module', scope: { paths: ['src'] } },
      { id: 'c1', goal: 'Add the API', parent: 'p', acceptance: ['API exported'], scope: { paths: ['src/a/**'] }, returns: 'The exported API' },
      { id: 'c2', goal: 'Use the API', parent: 'p', dependsOn: ['c1'], acceptance: ['Uses the API'], scope: { paths: ['src/b/*.mjs'] } },
    ], decisions: [{ id: 'd1', text: 'One module per directory' }] })]);
  const folder = path.join(stateLocation(root).directory, 'plans', 's5');
  assert.equal(readJson(path.join(folder, 'plan.json')).formatVersion, 2);

  const step = (stage, extra = []) => run(['task-step', '--root', root, '--session', 'one', '--stage', stage,
    '--target', 'acceptance:split', '--approach', 'bounded-fix', '--evidence', `checks/${stage}`, '--outcome', 'passed',
    '--next', 'Continue', ...(extra.includes('--result') ? [] : ['--result', `${stage} done`]), ...extra]);
  const start = id => run(['task-start', '--root', root, '--session', 'one', '--plan', 's5', '--task', id]);

  // One session holds the parent while it works the children in turn.
  const parent = start('p');
  assert.deepEqual(parent.plan.envelope.children.map(child => child.id), ['c1', 'c2']);
  const first = start('c1');
  assert.equal(first.plan.envelope.parent.id, 'p');
  assert.equal(first.plan.envelope.parent.checkpoint.session, stateLocation(root, 'one').sessionId);
  assert.equal(first.plan.envelope.task.returns, 'The exported API');
  edit(root, 'src/a/api.mjs');
  git('commit', '-qm', 'api', 'src/a/api.mjs');
  const done = step('verify', ['--result', 'export function api()', '--complete']);
  assert.equal(done.plan.state, 'completed');
  assert.deepEqual(done.plan.scope, { checked: true, changed: 1 });

  // The next child sees what its dependency returned, and its brief fits a worker assignment.
  const second = start('c2');
  assert.deepEqual(second.plan.envelope.dependencies.map(d => [d.id, d.state, d.result]), [['c1', 'completed', 'export function api()']]);
  assert.deepEqual(second.plan.envelope.decisions.map(d => d.id), ['d1']);
  const receipt = nativeCommand(['task-start', '--root', root, '--session', 'one', '--plan', 's5', '--task', 'c2']);
  assert.match(receipt.stdout, /^Owns: src\/b\/\*\.mjs$/m);
  assert.match(receipt.stdout, /^Part of: p$/m);
  assert.match(receipt.stdout, /^After c1 \(completed\): export function api\(\)$/m);
  const brief = run(['plan-envelope', '--root', root, '--plan', 's5', '--task', 'c2', '--brief']);
  assert.deepEqual(brief, { goal: 'Use the API', sources: ['tracker:PROJ-5', 'c1 returned: export function api()'],
    acceptance: ['Uses the API'], restrictions: ['Change only files matching src/b/*.mjs'], returnShape: 'What changed and how it was verified' });

  // A change outside the owned paths refuses completion until the scope is widened.
  edit(root, 'src/b/use.mjs');
  edit(root, 'value.mjs');
  const complete = ['task-step', '--root', root, '--session', 'one', '--stage', 'verify', '--target', 'acceptance:split',
    '--approach', 'bounded-fix', '--evidence', 'checks/verify', '--outcome', 'passed', '--result', 'r', '--next', 'Continue', '--complete'];
  assert.match(refused(complete), /scope exceeded: task c2 changed files outside its paths: value\.mjs;/);
  const plan = run(['plan', '--root', root, '--plan', 's5']);
  assert.equal(plan.tasks.find(item => item.id === 'c2').state, 'running');
  run(['plan-update', '--root', root, '--plan', 's5', '--expected-revision', String(plan.revision), '--session', 'one',
    '--data', write(root, 'widen.json', { tasks: [{ id: 'c2', scope: { paths: ['src/b/*.mjs', 'value.mjs'] } }] })]);
  assert.equal(step('verify', ['--complete']).plan.state, 'completed');

  // The parent completes last; its own and its children's paths cover every change since its claim.
  start('p');
  const closed = step('verify', ['--complete']);
  assert.equal(closed.plan.state, 'completed');
  assert.equal(closed.plan.scope.checked, true);
  const view = fs.readFileSync(path.join(folder, 'plan.md'), 'utf8');
  assert.match(view, /\| ↳ c1 \| completed \|/);
  assert.match(view, /Owns: src\/b\/\*\.mjs, value\.mjs/);
  assert.ok(readJson(path.join(folder, 'plan.json')).tasks.every(item => item.scopeBase === undefined));
});

test('a parent cannot complete before its children, and plans nest one level', t => {
  const { root } = fixture(t);
  run(['plan-create', '--root', root, '--plan', 'n', '--goal', 'Nest',
    '--data', write(root, 'plan.json', { tasks: [{ id: 'p', goal: 'Parent' }, { id: 'c', goal: 'Child', parent: 'p' }] })]);
  const update = changes => ['plan-update', '--root', root, '--plan', 'n', '--expected-revision', '1', '--data', write(root, 'change.json', changes)];
  assert.match(refused(update({ tasks: [{ id: 'g', goal: 'Grandchild', parent: 'c' }] })), /plans nest one level/);
  assert.match(refused(update({ tasks: [{ id: 'c', dependsOn: ['p'] }] })), /cycle/);
  run(['task-start', '--root', root, '--session', 'one', '--plan', 'n', '--task', 'p']);
  assert.match(refused(['task-step', '--root', root, '--session', 'one', '--stage', 'verify', '--target', 'acceptance:n',
    '--approach', 'x', '--evidence', 'y', '--outcome', 'passed', '--result', 'r', '--next', 'n', '--complete']), /child tasks that are not completed or abandoned: c/);
  assert.match(refused(['plan-envelope', '--root', root, '--plan', 'n', '--task', 'p', '--brief']), /no acceptance criteria/);
  assert.match(refused(['plan', '--root', root, '--brief']), /--brief is supported by plan-envelope/);
});

test('a plan task names the engineering contract its session completes against', t => {
  const { root } = fixture(t);
  const contract = fs.readFileSync(new URL('./fixtures/acceptance/contract.json', import.meta.url), 'utf8').replaceAll('"src"', '"value.mjs"');
  fs.mkdirSync(path.join(root, 'contracts'));
  fs.writeFileSync(path.join(root, 'contracts', 'value.json'), contract);
  run(['plan-create', '--root', root, '--plan', 'k', '--goal', 'Contracted',
    '--data', write(root, 'plan.json', { tasks: [{ id: 'v', goal: 'Correct the value', contract: 'contracts/value.json' }] })]);
  const started = run(['task-start', '--root', root, '--session', 'one', '--plan', 'k', '--task', 'v']);
  assert.equal(started.acceptance.complete, false);
  assert.equal(started.plan.envelope.task.contract, 'contracts/value.json');
  assert.throws(() => run(['task-step', '--root', root, '--session', 'one', '--stage', 'verify', '--target', 'acceptance:value',
    '--approach', 'x', '--evidence', 'y', '--outcome', 'passed', '--result', 'r', '--next', 'n', '--complete']), /every required criterion/);
});

test('plans without the newer fields stay at the first format version', t => {
  const { root } = fixture(t);
  const created = run(['plan-create', '--root', root, '--plan', 'old', '--goal', 'Plain',
    '--data', write(root, 'plan.json', { tasks: [{ id: 'a', goal: 'A' }] })]);
  assert.equal(created.formatVersion, 1);
  const started = run(['task-start', '--root', root, '--session', 'one', '--plan', 'old', '--task', 'a']);
  assert.equal(started.plan.envelope.task.id, 'a');
  assert.equal(run(['task-step', '--root', root, '--session', 'one', '--stage', 'verify', '--target', 'acceptance:a',
    '--approach', 'x', '--evidence', 'y', '--outcome', 'passed', '--result', 'r', '--next', 'n', '--complete']).plan.scope, undefined);
  assert.equal(readJson(path.join(stateLocation(root).directory, 'plans', 'old', 'plan.json')).formatVersion, 1);
});
