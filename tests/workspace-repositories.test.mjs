import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { spawnSync } from 'node:child_process';
import { run } from '../cli/main.mjs';
import { activeTask } from '../application/session/activity.mjs';
import { saveCheckpoint, recoverCheckpoint } from '../application/checkpoint.mjs';
import { workspace, workspaceBoundary } from '../application/workspace.mjs';
import { workspaceOperation, repositoryState, scan, reviewSnapshot, stateFile } from '../platform/index.mjs';
import { DEFAULT_SETTINGS } from '../engine/policy/effective-settings.mjs';
import { decide, digest } from '../application/engine-client.mjs';
import { copy } from '../tooling/export/export.mjs';

function fixture(t) {
  const root = fs.realpathSync(fs.mkdtempSync(path.join(os.tmpdir(), 'craft-workspace-')));
  t.after(() => fs.rmSync(root, { recursive: true, force: true }));
  const git = (repo, ...args) => {
    const result = spawnSync('git', ['-C', path.join(root, repo), '-c', 'core.hooksPath=/dev/null', '-c', 'commit.gpgsign=false', '-c', 'user.name=Fixture', '-c', 'user.email=fixture@example.test', ...args], { encoding: 'utf8' });
    assert.equal(result.status, 0, result.stderr); return result.stdout.trim();
  };
  for (const repo of ['kit', 'profile']) {
    fs.mkdirSync(path.join(root, repo)); git(repo, 'init', '-q');
    fs.writeFileSync(path.join(root, repo, 'value.mjs'), 'export const value = 1;\n');
    git(repo, 'add', '.'); git(repo, 'commit', '-qm', 'fixture');
  }
  const command = (action, ...args) => run([action, '--root', root, '--session', 'session', ...args]);
  const start = (repos = ['kit'], task = 'task') => command('task-start', '--task', task, '--goal', 'Update selected repositories', ...repos.flatMap(repo => ['--repository', repo]));
  const accept = () => command('task-step', '--stage', 'verify', '--target', 'acceptance:scope', '--approach', 'fixture', '--evidence', 'checks/scope', '--outcome', 'passed', '--result', 'Selected source verified', '--next', 'Return verified result', '--complete');
  return { root, git, command, start, accept };
}

test('a non-Git workspace binds one repository and ignores changes in unselected siblings', t => {
  const { root, command, start, accept } = fixture(t);
  const begun = start();
  assert.deepEqual(begun.repositories, ['kit']);
  assert.equal(begun.checkpoint.state.fingerprints.complete, true);
  assert.equal(begun.checkpoint.state.repositorySnapshots[0].head, repositoryState(path.join(root, 'kit')).head);
  const lookup = command('locate', '--term', 'value');
  assert.deepEqual(lookup.paths, ['kit']);
  assert.ok(lookup.matches.every(match => match.path.startsWith('kit/')));
  assert.throws(() => command('locate', '--term', 'value', '--path', 'profile'), /selected repository/);
  fs.writeFileSync(path.join(root, 'kit/.gitignore'), 'ignored-secret.txt\n');
  fs.writeFileSync(path.join(root, 'kit/ignored-secret.txt'), 'value should not be searched');
  assert.equal(command('locate', '--term', 'should not be searched').matches.length, 0);
  assert.equal(accept().status.checkpoint.status, 'fresh');
  fs.appendFileSync(path.join(root, 'profile/value.mjs'), '// unrelated\n');
  assert.equal(command('session').task.goalComplete, true);
  assert.equal(recoverCheckpoint(root, 'session').changed, false);
  fs.appendFileSync(path.join(root, 'kit/value.mjs'), '// relevant\n');
  assert.equal(command('session').taskEvidenceStale, true);
  assert.equal(recoverCheckpoint(root, 'session').changed, true);
});

test('cross-repository acceptance detects edits and same-SHA branch changes in either repository', t => {
  const { root, git, command, start, accept } = fixture(t);
  start(['profile', 'kit']);
  const accepted = accept();
  assert.deepEqual(accepted.status.repositories, ['kit', 'profile']);
  assert.equal(accepted.status.checkpoint.status, 'fresh');
  git('profile', 'switch', '-c', 'other');
  assert.equal(activeTask(root, 'session'), undefined);
  assert.throws(() => accept(), /task-start binding/);
  assert.equal(command('session', '--task', 'task').taskEvidenceStale, true);
  start(['kit', 'profile']); accept();
  fs.appendFileSync(path.join(root, 'profile/value.mjs'), '// changed\n');
  assert.equal(command('session').task.goalComplete, false);
  const priorWorkspace = workspace(root, 'session', 'task', 'open');
  start(['kit']);
  assert.deepEqual(command('session').repositories, ['kit']);
  assert.equal(command('session').task.goalComplete, false);
  assert.equal(command('session').workspace.status, 'absent');
  assert.notEqual(workspace(root, 'session', 'task', 'open').directory, priorWorkspace.directory);
});

test('explicit repository roots reject traversal, aliases, nonroots and out-of-scope task files', t => {
  const { root, start, command } = fixture(t);
  fs.symlinkSync(path.join(root, 'kit'), path.join(root, 'alias'));
  for (const scopes of [['../outside'], ['/tmp'], ['kit', './kit'], ['alias'], ['kit/.git'], ['.']]) assert.throws(() => start(scopes));
  assert.throws(() => command('task-start', '--task', 'task', '--goal', 'Update selected repositories', '--repository', 'kit', '--file', 'profile/value.mjs'), /selected repository/);
  assert.equal(activeTask(root, 'session'), undefined);
  saveCheckpoint(root, 'manual', { repositories: ['kit', 'profile'] });
  assert.equal(recoverCheckpoint(root, 'manual').changeDetection.complete, true);
});

test('a corrupt activity ledger keeps fresh lookup inside the separately saved repository selection', t => {
  const { root, start, command } = fixture(t); start(['kit']);
  const file = stateFile(root, `session-${digest(['session', 'task']).slice(0, 24)}.json`);
  fs.writeFileSync(file, '{broken');
  const result = command('locate', '--term', 'value');
  assert.equal(result.discovery.reused, 0);
  assert.equal(result.discovery.fresh, 1);
  assert.deepEqual(result.paths, ['kit']);
  assert.ok(result.matches.every(match => match.path.startsWith('kit/')));
  assert.ok(result.reuse.diagnostic);
  assert.notEqual(command('session').task.goalComplete, true);
});

test('workspace is lazy, isolated by session/task/operation, promotes selective evidence and cleans on completion', t => {
  const { root, command, start, accept } = fixture(t);
  start();
  assert.equal(fs.existsSync(path.join(root, '.capabilities/state/sessions')), false);
  assert.equal(command('session').workspace.status, 'absent');
  const first = workspace(root, 'session', 'task', 'write', { name: 'result.json', content: '{"verified":true}' });
  const other = workspace(root, 'session', 'task', 'open', { operation: 'worker-1' });
  assert.notEqual(first.scratchDirectory, other.scratchDirectory);
  const promoted = workspace(root, 'session', 'task', 'promote', { name: 'result.json', purpose: 'acceptance' });
  assert.equal(promoted.evidence.artifacts, 1);
  assert.ok(JSON.parse(fs.readFileSync(path.join(first.directory, 'owner.json'), 'utf8')).evidence[0].sourceAtPromotion);
  assert.equal(accept().workspace.scratch, 'cleaned');
  assert.equal(command('session').workspace.evidence.artifacts, 1);
  assert.equal(fs.existsSync(path.join(first.directory, 'scratch')), false);
  assert.equal(fs.readdirSync(path.join(first.directory, 'evidence')).length, 1);
  assert.equal(workspaceBoundary(root, 'session', 'task', 'complete').scratch, 'cleaned');
});

test('workspace bounds, settings merge and ownership checks fail closed', t => {
  const { root, start } = fixture(t); start();
  const settings = decide('effective-settings', { project: { formatVersion: 1, workspace: { scratch: { maxBytes: 4 } } } }).settings;
  assert.equal(settings.workspace.evidence.retentionDays, 7);
  assert.throws(() => workspace(root, 'session', 'task', 'write', { name: 'large', content: '12345' }, settings), /budget/);
  for (const name of ['../outside', '/tmp/outside']) assert.throws(() => workspace(root, 'session', 'task', 'write', { name, content: 'x' }), /safe filename/);
  assert.throws(() => workspace(root, 'session', 'task', 'write', { name: 'x'.repeat(101), content: 'x' }), /100 characters/);
  workspace(root, 'session', 'task', 'write', { name: 'x'.repeat(100), content: 'x' });
  assert.equal(workspace(root, 'session', 'task', 'promote', { name: 'x'.repeat(100), purpose: 'boundary' }).evidence.artifacts, 1);
  assert.equal(workspace(root, 'session', 'task', 'open', {}, { ...settings, workspace: { ...settings.workspace, enabled: false } }).status, 'disabled');
  const opened = workspace(root, 'session', 'task', 'open');
  const outside = path.join(root, 'outside'); fs.mkdirSync(outside); fs.writeFileSync(path.join(outside, 'keep'), 'keep');
  fs.symlinkSync(outside, path.join(opened.scratchDirectory, 'escape'));
  assert.equal(workspaceBoundary(root, 'session', 'task', 'complete').status, 'retained');
  assert.equal(fs.readFileSync(path.join(outside, 'keep'), 'utf8'), 'keep');
  fs.unlinkSync(path.join(opened.scratchDirectory, 'escape'));
  fs.writeFileSync(path.join(opened.directory, 'owner.json'), '{"owner":"someone-else"}');
  assert.throws(() => workspace(root, 'session', 'task', 'complete'), /ownership/);
  assert.ok(fs.existsSync(opened.directory));
});

test('stale cleanup retains live owners and reclaims abandoned scratch and aged evidence', t => {
  const { root } = fixture(t);
  const identity = { session: digest('old-session'), task: digest('old-task'), operation: digest('scope') };
  const policy = structuredClone(DEFAULT_SETTINGS.workspace), now = Date.now();
  const opened = workspaceOperation(root, identity, policy, 'write', { name: 'note', content: 'intermediate' }, now);
  workspaceOperation(root, null, policy, 'maintain', {}, now + 2 * 86400000);
  assert.ok(fs.existsSync(opened.scratchDirectory), 'age cannot remove live owners');
  const file = path.join(opened.directory, 'owner.json'), owner = JSON.parse(fs.readFileSync(file));
  owner.operations[identity.operation].pid = 2147483647; fs.writeFileSync(file, JSON.stringify(owner));
  const cleaned = workspaceOperation(root, null, policy, 'maintain', {}, now + 2 * 86400000);
  assert.equal(cleaned.cleaned[0].scratch, 'cleaned');
  assert.equal(fs.existsSync(path.join(opened.directory, 'scratch')), false);
  workspaceOperation(root, null, policy, 'maintain', {}, now + 8 * 86400000);
  assert.equal(fs.existsSync(opened.directory), false);
});

test('private source-like artifacts are excluded from lookup, routing, snapshots, review and export without ignores', t => {
  const { root, git } = fixture(t);
  const repo = path.join(root, 'kit'), secret = path.join(repo, 'nested/.capabilities/state/sessions/task/scratch/tempting.py');
  fs.mkdirSync(path.dirname(secret), { recursive: true }); fs.writeFileSync(secret, 'PRIVATE_NEEDLE\n');
  const before = repositoryState(repo);
  fs.appendFileSync(secret, 'more private content\n');
  const after = repositoryState(repo);
  assert.equal(before.fingerprints.workingTree, after.fingerprints.workingTree);
  assert.deepEqual(before.status, []);
  const result = scan(repo, { terms: ['PRIVATE_NEEDLE'], scopes: ['.'], maxFiles: 100, maxMatches: 10, maxBytes: 10000, maxFileBytes: 10000, maxSnippetCharacters: 100 }, ({ content }) => { assert.equal(content.includes('PRIVATE_NEEDLE'), false); return []; });
  assert.equal(result.filesScanned, 1);
  const catalog = JSON.parse(fs.readFileSync(new URL('../engine/routing/catalog.json', import.meta.url)));
  assert.deepEqual(decide('route', { intent: 'implement', files: ['nested/.capabilities/state/tempting.py'], project: { formatVersion: 1 }, catalog }).files, []);
  const head = git('kit', 'rev-parse', 'HEAD'); git('kit', 'add', '.'); git('kit', 'commit', '-qm', 'private fixture');
  const review = reviewSnapshot({ root: repo, base: head, head: 'HEAD' });
  assert.deepEqual(review.files, []); assert.equal(review.diff, '');
  const output = path.join(root, 'export'); copy(repo, output);
  assert.equal(fs.existsSync(path.join(output, 'nested/.capabilities/state')), false);
  assert.equal(fs.existsSync(path.join(output, '.git')), false);
});

test('workspace operations retain other owners, recover only dead owned locks and enforce count limits', t => {
  const { root } = fixture(t);
  const identity = { session: digest('s'), task: digest('t'), operation: digest('main') };
  const policy = { ...DEFAULT_SETTINGS.workspace, maxSessions: 1 };
  const opened = workspaceOperation(root, identity, policy, 'open');
  const ownerFile = path.join(opened.directory, 'owner.json');
  const owner = JSON.parse(fs.readFileSync(ownerFile));
  owner.operations[digest('other-worker')] = { pid: process.pid, closed: false };
  fs.writeFileSync(ownerFile, JSON.stringify(owner));
  assert.equal(workspaceOperation(root, identity, policy, 'complete').scratch, 'retained');
  assert.ok(fs.existsSync(opened.scratchDirectory));
  assert.throws(() => workspaceOperation(root, { ...identity, operation: digest('other-worker') }, policy, 'write', { name: 'collision', content: 'x' }), /another active owner/);
  assert.throws(() => workspaceOperation(root, { ...identity, task: digest('another') }, policy, 'open'), /capacity/);
  const lock = `${stateFile(root, 'workspace-guard.json')}.lock`;
  fs.writeFileSync(lock, JSON.stringify({ owner: 'craft-workspace-lock', formatVersion: 1, pid: process.pid }));
  assert.equal(workspaceOperation(root, identity, policy, 'complete').status, 'retained');
  fs.writeFileSync(lock, JSON.stringify({ owner: 'craft-workspace-lock', formatVersion: 1, pid: 2147483647 }));
  assert.equal(workspaceOperation(root, identity, policy, 'status').status, 'available');
  assert.equal(fs.existsSync(lock), false);
  assert.throws(() => workspaceOperation(root, { ...identity, task: '../bad' }, policy, 'complete'), /identity/);
});

test('linked worktree selection and relocated exported workspace commands preserve repository isolation', t => {
  const { root, git } = fixture(t);
  git('kit', 'worktree', 'add', '-qb', 'linked-task', path.join(root, 'linked'));
  const cli = path.resolve('dist/codex/packages/craft-delivery/support/cli/main.mjs');
  const invoke = (action, ...args) => {
    const result = spawnSync(process.execPath, [cli, action, '--root', root, '--session', 'export-session', '--json', ...args], { encoding: 'utf8' });
    assert.equal(result.status, 0, result.stderr); return JSON.parse(result.stdout);
  };
  const started = invoke('task-start', '--task', 'export-task', '--goal', 'Verify the linked workspace', '--repository', 'linked');
  assert.equal(started.checkpoint.state.repositorySnapshots[0].checkout, 'refs/heads/linked-task');
  const opened = invoke('workspace-open');
  assert.equal(opened.status, 'available');
  assert.equal(invoke('session').workspace.directory, opened.directory);
  assert.equal(invoke('session').repositories[0], 'linked');
  assert.equal(fs.existsSync(path.join(root, 'linked/.capabilities')), false);
});
