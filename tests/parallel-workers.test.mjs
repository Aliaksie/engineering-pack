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

// Commits craft makes in worktrees and merges need an identity and no signing prompt;
// files checked out by merges keep LF even where the machine enables core.autocrlf.
Object.assign(process.env, {
  GIT_AUTHOR_NAME: 'Fixture', GIT_AUTHOR_EMAIL: 'fixture@example.test',
  GIT_COMMITTER_NAME: 'Fixture', GIT_COMMITTER_EMAIL: 'fixture@example.test',
  GIT_CONFIG_COUNT: '3', GIT_CONFIG_KEY_0: 'commit.gpgsign', GIT_CONFIG_VALUE_0: 'false',
  GIT_CONFIG_KEY_1: 'core.hooksPath', GIT_CONFIG_VALUE_1: '/dev/null',
  GIT_CONFIG_KEY_2: 'core.autocrlf', GIT_CONFIG_VALUE_2: 'false',
});

const git = (dir, ...args) => {
  const outcome = spawnSync('git', ['-C', dir, ...args], { encoding: 'utf8' });
  assert.equal(outcome.status, 0, outcome.stderr);
  return outcome.stdout.trim();
};

const put = (dir, file, text) => {
  fs.mkdirSync(path.dirname(path.join(dir, file)), { recursive: true });
  fs.writeFileSync(path.join(dir, file), text);
};

function repo(dir, files) {
  fs.mkdirSync(dir, { recursive: true });
  git(dir, 'init', '-q');
  for (const [file, text] of Object.entries(files)) put(dir, file, text);
  git(dir, 'add', '.'); git(dir, 'commit', '-qm', 'fixture');
}

function folder(t) {
  const root = fs.realpathSync.native(fs.mkdtempSync(path.join(os.tmpdir(), 'craft-parallel-')));
  t.after(() => fs.rmSync(root, { recursive: true, force: true }));
  return root;
}

const ignore = '.craft/state/\n.craft/worktrees/\n';

const refused = argv => {
  const outcome = nativeCommand(argv[0] === 'task-step' ? [...argv, '--bundle', repository] : argv);
  assert.notEqual(outcome.exitCode, 0, outcome.stdout);
  return outcome.stderr;
};

function planned(t, root, tasks) {
  const data = path.join(root, '..', `${path.basename(root)}-plan.json`);
  fs.writeFileSync(data, JSON.stringify({ tasks }));
  t.after(() => fs.rmSync(data, { force: true }));
  run(['plan-create', '--root', root, '--plan', 's7', '--goal', 'Parallel', '--data', data]);
  const record = path.join(stateLocation(root).directory, 'plans', 's7', 'plan.json');
  const task = id => readJson(record).tasks.find(item => item.id === id);
  const assign = (task, extra = []) => run(['plan-assign', '--root', root, '--plan', 's7', '--task', task, '--session', 'lead', ...extra]);
  const integrate = (task, extra = [], session = 'lead') => run(['plan-integrate', '--root', root, '--plan', 's7', '--task', task, '--session', session, ...extra]);
  const start = (session, task) => run(['task-start', '--root', root, '--session', session, '--plan', 's7', '--task', task]);
  const completion = session => ['task-step', '--root', root, '--session', session, '--stage', 'verify',
    '--target', 'acceptance:s7', '--approach', 'bounded-fix', '--evidence', 'checks/s7', '--outcome', 'passed',
    '--result', 'done', '--next', 'Integrate', '--complete'];
  const complete = session => run(completion(session));
  const next = () => run(['plan-next', '--root', root, '--plan', 's7', '--session', 'lead']);
  return { record, task, assign, integrate, start, completion, complete, next };
}

test('workers work in isolated worktrees and clean integrations merge without a model', t => {
  const root = folder(t);
  repo(root, { '.gitignore': ignore, 'src/a/x.mjs': 'export const x = 1;\n', 'src/b/y.mjs': 'export const y = 1;\n', 'shared.mjs': 'export const s = 1;\n' });
  const { record, task, assign, integrate, start, completion, complete, next } = planned(t, root, [
    { id: 'a', goal: 'Change A', acceptance: ['A works'], scope: { paths: ['src/a/**'] } },
    { id: 'b', goal: 'Change B', scope: { paths: ['src/b/**'] } },
    { id: 'c', goal: 'Build on A', dependsOn: ['a'], scope: { paths: ['src/c/**'] } },
    { id: 'd', goal: 'Touch A too', scope: { paths: ['src/a/inner.mjs'] } },
    { id: 'near', goal: 'Touch B too', scope: { paths: ['src/b/z.mjs'] } },
    { id: 'free', goal: 'Owns nothing' },
  ]);
  const head = git(root, 'rev-parse', 'HEAD');

  assert.match(refused(['plan-assign', '--root', root, '--plan', 's7', '--task', 'free', '--session', 'lead']), /owns no paths/);
  const assigned = assign('a');
  assert.equal(assigned.worker.session, 'lead/s7/a');
  assert.equal(assigned.isolated, true);
  assert.deepEqual(assigned.work.repositories, [{ path: '', worktree: '.craft/worktrees/s7/a-1', branch: 'craft/s7/a-1', base: head }]);
  assert.deepEqual(assigned.brief.restrictions, ['Change only files matching src/a/**',
    'Work only in the worktree .craft/worktrees/s7/a-1 (branch craft/s7/a-1); leave other checkouts unchanged']);
  assert.equal(readJson(record).formatVersion, 4);
  const worktree = path.join(root, '.craft/worktrees/s7/a-1');
  assert.equal(git(worktree, 'branch', '--show-current'), 'craft/s7/a-1');
  const receipt = nativeCommand(['plan-assign', '--root', root, '--plan', 's7', '--task', 'b', '--session', 'lead', '--shared']);
  assert.equal(receipt.exitCode, 0, receipt.stderr);
  assert.match(receipt.stdout, /^Assigned: b of plan s7 to worker session lead\/s7\/b/m);
  assert.match(receipt.stdout, /^Shared checkout: /m);
  assert.match(refused(['plan-assign', '--root', root, '--plan', 's7', '--task', 'd', '--session', 'lead']), /running tasks also own: a \(src\/a\/inner\.mjs ~ src\/a\/\*\*\)/);
  // An ordinary claim respects the paths a shared worker owns, and the queue does not offer it.
  assert.match(refused(['task-start', '--root', root, '--session', 'other', '--plan', 's7', '--task', 'near']), /running tasks also own: b \(src\/b\/z\.mjs ~ src\/b\/\*\*\)/);
  assert.equal(task('near').state, 'pending');
  assert.ok(!next().ready.includes('near'));
  assert.deepEqual(next().waiting.find(item => item.task === 'near').on, [{ id: 'b', state: 'running', paths: ['src/b/z.mjs ~ src/b/**'] }]);
  // The coordinator checkout does not see the ignored worktrees.
  assert.equal(git(root, 'status', '--porcelain'), '');

  // The worker binds its worktree and completes there, within its paths.
  const started = start('lead/s7/a', 'a');
  assert.deepEqual(started.repositories, ['.craft/worktrees/s7/a-1']);
  put(worktree, 'src/a/x.mjs', 'export const x = 2;\n');
  // A refused completion leaves the worktree as it was.
  const failed = completion('lead/s7/a').map(arg => arg === 'passed' ? 'quality-failure' : arg);
  assert.match(refused(failed), /requires a passed acceptance receipt/);
  assert.equal(git(worktree, 'rev-parse', 'HEAD'), head);
  assert.equal(git(worktree, 'status', '--porcelain'), 'M src/a/x.mjs');
  put(worktree, 'outside.mjs', 'export const o = 1;\n');
  assert.match(refused(completion('lead/s7/a')), /scope exceeded: task a changed files outside its paths: outside\.mjs/);
  fs.rmSync(path.join(worktree, 'outside.mjs'));
  const done = complete('lead/s7/a');
  assert.equal(done.plan.state, 'completed');
  const branchHead = git(root, 'rev-parse', 'craft/s7/a-1');
  assert.notEqual(branchHead, head);
  assert.equal(git(worktree, 'status', '--porcelain'), '');
  assert.equal(task('a').work.repositories[0].head, branchHead);

  // Dependents wait for integration, which only the assigning session does.
  const queued = next();
  assert.deepEqual(queued.integrate, [{ task: 'a' }]);
  assert.deepEqual(queued.waiting.find(item => item.task === 'c').on, [{ id: 'a', state: 'completed' }]);
  assert.match(refused(['plan-integrate', '--root', root, '--plan', 's7', '--task', 'a', '--session', 'other']), /session that assigned it/);
  put(root, 'src/a/x.mjs', 'export const x = 3;\n');
  assert.match(refused(['plan-integrate', '--root', root, '--plan', 's7', '--task', 'a', '--session', 'lead']), /uncommitted changes .* touch files the branch changes: src\/a\/x\.mjs/);
  git(root, 'checkout', '--', 'src/a/x.mjs');

  const merged = integrate('a');
  assert.equal(merged.integrated, true);
  assert.deepEqual(merged.worktrees, { removed: true, kept: [] });
  assert.equal(fs.readFileSync(path.join(root, 'src/a/x.mjs'), 'utf8'), 'export const x = 2;\n');
  assert.equal(git(root, 'rev-parse', 'HEAD^2'), branchHead);
  assert.equal(merged.merges[0].commit, git(root, 'rev-parse', 'HEAD'));
  assert.equal(fs.existsSync(path.join(root, '.craft/worktrees')), false);
  assert.equal(spawnSync('git', ['-C', root, 'rev-parse', '--verify', '-q', 'craft/s7/a-1']).status, 1);
  assert.equal(task('a').integration.state, 'integrated');
  assert.ok(next().ready.includes('c'));

  // The assigning session takes its shared worker's task back.
  const released = run(['plan-release', '--root', root, '--plan', 's7', '--task', 'b', '--session', 'lead']);
  assert.equal(released.state, 'pending');
  start('other', 'near');
  assert.equal(task('near').state, 'running');
});

test('conflicts return to the integrator, escalate when the budget is spent and can be discarded', t => {
  const root = folder(t);
  repo(root, { '.gitignore': ignore, 'shared.mjs': 'export const s = 1;\n', 'other.mjs': 'export const o = 1;\n' });
  const { record, task, assign, integrate, start, complete, next } = planned(t, root, [
    { id: 'e', goal: 'Change shared', scope: { paths: ['shared.mjs'] } },
    { id: 'f', goal: 'Change other', maxAttempts: 1, scope: { paths: ['other.mjs'] } },
  ]);
  assign('e');
  assign('f');
  const e = path.join(root, '.craft/worktrees/s7/e-1');
  const f = path.join(root, '.craft/worktrees/s7/f-1');
  start('lead/s7/e', 'e');
  put(e, 'shared.mjs', 'export const s = 2;\n');
  complete('lead/s7/e');
  start('lead/s7/f', 'f');
  put(f, 'other.mjs', 'export const o = 2;\n');
  complete('lead/s7/f');
  // Meanwhile the coordinator changes the same lines.
  put(root, 'shared.mjs', 'export const s = 3;\n');
  put(root, 'other.mjs', 'export const o = 3;\n');
  git(root, 'commit', '-qam', 'coordinator change');
  const coordinator = git(root, 'rev-parse', 'HEAD');

  const conflict = integrate('e');
  assert.equal(conflict.integrated, false);
  assert.deepEqual(conflict.conflicts, ['shared.mjs']);
  assert.equal(conflict.state, 'running');
  assert.equal(conflict.coordinator[0].merge, coordinator);
  assert.equal(git(root, 'status', '--porcelain'), '');
  assert.equal(git(root, 'rev-parse', 'HEAD'), coordinator);
  const receipt = nativeCommand(['plan-integrate', '--root', root, '--plan', 's7', '--task', 'f', '--session', 'lead']);
  assert.match(receipt.stdout, /^Needs the user: f conflicts on other\.mjs$/m);
  assert.deepEqual(next().needsUser.map(item => item.task), ['f']);

  // The integrator resolves in the worktree, completes again and integrates.
  assert.deepEqual(start('lead', 'e').repositories, ['.craft/worktrees/s7/e-1']);
  assert.equal(spawnSync('git', ['-C', e, 'merge', '-q', coordinator]).status, 1);
  put(e, 'shared.mjs', 'export const s = 4;\n');
  git(e, 'add', 'shared.mjs'); git(e, 'commit', '-qm', 'resolve');
  complete('lead');
  assert.equal(task('e').attempts, 2);
  assert.equal(integrate('e').integrated, true);
  assert.equal(fs.readFileSync(path.join(root, 'shared.mjs'), 'utf8'), 'export const s = 4;\n');

  // Discarding drops the escalated result; its unmerged branch is kept.
  assert.match(refused(['plan-integrate', '--root', root, '--plan', 's7', '--task', 'f', '--session', 'lead', '--escalate']), /--reason/);
  const escalated = integrate('f', ['--escalate', '--reason', 'Needs a product decision']);
  assert.equal(escalated.integration.reason, 'Needs a product decision');
  const discarded = integrate('f', ['--discard']);
  assert.equal(discarded.state, 'pending');
  assert.equal(fs.existsSync(f), false);
  assert.match(discarded.worktrees.kept[0], /branch craft\/s7\/f-1 kept/);
  assert.equal(task('f').work, undefined);

  // A new attempt works on its own branch; the discarded one stays.
  const data = path.join(root, '..', `${path.basename(root)}-change.json`);
  fs.writeFileSync(data, JSON.stringify({ tasks: [{ id: 'f', maxAttempts: 3 }] }));
  t.after(() => fs.rmSync(data, { force: true }));
  run(['plan-update', '--root', root, '--plan', 's7', '--expected-revision', String(readJson(record).revision), '--data', data]);
  const attempt = task('f').attempts + 1;
  const retried = assign('f');
  assert.deepEqual(retried.work.repositories.map(r => [r.worktree, r.branch]), [[`.craft/worktrees/s7/f-${attempt}`, `craft/s7/f-${attempt}`]]);
  assert.equal(spawnSync('git', ['-C', root, 'rev-parse', '--verify', '-q', 'craft/s7/f-1']).status, 0);
});

test('a folder of repositories gets one worktree per repository and a plain folder shares', t => {
  const root = folder(t);
  repo(path.join(root, 'api'), { 'a.mjs': 'export const a = 1;\n' });
  repo(path.join(root, 'web'), { 'w.mjs': 'export const w = 1;\n' });
  const { assign, integrate, start, complete } = planned(t, root, [
    { id: 'both', goal: 'Change both', repositories: ['api', 'web'], scope: { paths: ['api/a.mjs', 'web/w.mjs'] } },
    { id: 'plain', goal: 'No repository', scope: { paths: ['notes'] } },
  ]);
  const assigned = assign('both');
  assert.deepEqual(assigned.work.repositories.map(r => [r.path, r.worktree]),
    [['api', '.craft/worktrees/s7/both-1/api'], ['web', '.craft/worktrees/s7/both-1/web']]);
  start('lead/s7/both', 'both');
  put(path.join(root, '.craft/worktrees/s7/both-1/api'), 'a.mjs', 'export const a = 2;\n');
  put(path.join(root, '.craft/worktrees/s7/both-1/web'), 'w.mjs', 'export const w = 2;\n');
  complete('lead/s7/both');
  const merged = integrate('both');
  assert.deepEqual(merged.merges.map(m => m.path), ['api', 'web']);
  assert.equal(fs.readFileSync(path.join(root, 'api/a.mjs'), 'utf8'), 'export const a = 2;\n');
  assert.equal(fs.readFileSync(path.join(root, 'web/w.mjs'), 'utf8'), 'export const w = 2;\n');

  // A plain folder has nothing to isolate; the worker shares it.
  assert.equal(assign('plain').isolated, false);
});

test('worktrees need the ignore rule and a repository top folder', t => {
  const root = folder(t);
  repo(root, { '.gitignore': '.craft/state/\n', 'src/x.mjs': 'export const x = 1;\n', 'sub/y.mjs': 'export const y = 1;\n' });
  planned(t, root, [{ id: 'g', goal: 'Change', scope: { paths: ['src/**'] } }]);
  assert.match(refused(['plan-assign', '--root', root, '--plan', 's7', '--task', 'g', '--session', 'lead']), /does not ignore \.craft\/worktrees\/; add it to \.gitignore/);
  fs.writeFileSync(path.join(root, '.gitignore'), ignore);
  git(root, 'commit', '-qam', 'ignore worktrees');
  put(root, 'src/x.mjs', 'export const x = 2;\n');
  assert.match(refused(['plan-assign', '--root', root, '--plan', 's7', '--task', 'g', '--session', 'lead']), /uncommitted changes in the task's paths would not reach its worktree: src\/x\.mjs/);
  assert.equal(fs.existsSync(path.join(root, '.craft/worktrees')), false);
  const sub = path.join(root, 'sub');
  planned(t, sub, [{ id: 'h', goal: 'Change', scope: { paths: ['y.mjs'] } }]);
  assert.match(refused(['plan-assign', '--root', sub, '--plan', 's7', '--task', 'h', '--session', 'lead']), /folder inside a repository/);
});
