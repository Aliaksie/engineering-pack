import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { spawn, spawnSync } from 'node:child_process';
import { run } from '../cli/main.mjs';
import { activeTask } from '../application/session/activity.mjs';
import { saveCheckpoint, recoverCheckpoint } from '../application/checkpoint.mjs';
import { workspaceBoundary } from '../application/workspace.mjs';
import { workspaceOperation, repositoryRoots, repositoryState, nativeLocate, reviewSnapshot, stateFile } from '../platform/index.mjs';
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

test('Windows short and expanded workspace paths select the same Git root', { skip: process.platform !== 'win32' }, t => {
  const { root } = fixture(t);
  const expanded = fs.realpathSync.native(root);
  const result = spawnSync('cmd.exe', ['/d', '/c', `for %I in ("${expanded}") do @echo %~sI`], { encoding: 'utf8', windowsVerbatimArguments: true });
  assert.equal(result.status, 0, result.stderr);
  const short = result.stdout.trim();
  if (path.relative(short, expanded) === '') return t.skip('Windows short names are unavailable on this volume');
  for (const workspace of [short, expanded]) {
    const [selected] = repositoryRoots(workspace, ['kit']);
    assert.equal(path.relative(fs.realpathSync.native(selected.root), fs.realpathSync.native(path.join(expanded, 'kit'))), '');
    assert.throws(() => repositoryRoots(workspace, ['kit/.git']), /Git worktree root/);
  }
});

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
  const priorWorkspace = command('workspace-open');
  start(['kit']);
  assert.deepEqual(command('session').repositories, ['kit']);
  assert.equal(command('session').task.goalComplete, false);
  assert.equal(command('session').workspace.status, 'absent');
  assert.notEqual(command('workspace-open').directory, priorWorkspace.directory);
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
  const file = stateFile(root, `session-${digest(['session', 'task']).slice(0, 24)}.json`, false, 'session');
  fs.writeFileSync(file, '{broken');
  const result = command('locate', '--term', 'value');
  assert.equal(result.discovery.reused, 0);
  assert.equal(result.discovery.fresh, 1);
  assert.deepEqual(result.paths, ['kit']);
  assert.ok(result.matches.every(match => match.path.startsWith('kit/')));
  assert.ok(result.reuse.diagnostic);
  assert.notEqual(command('session').task.goalComplete, true);
});

let inputs = 0;
function input(root, value) {
  const file = path.join(root, `input-${++inputs}.json`);
  fs.writeFileSync(file, JSON.stringify(value)); return file;
}
// The JS boundaries (session/task/host) address the main scope of the bound task.
const mainScope = (task = 'task', repositories = ['kit']) => ({ session: digest('session'), task: digest([task, repositories]), operation: digest('main') });
const setOwners = (directory, pid) => {
  const file = path.join(directory, 'owner.json'), owner = JSON.parse(fs.readFileSync(file));
  for (const op of Object.values(owner.operations)) op.pid = pid;
  fs.writeFileSync(file, JSON.stringify(owner));
};

test('workspace is lazy, isolated by session/task/operation, promotes selective evidence and cleans on completion', t => {
  const { root, command, start, accept } = fixture(t);
  start();
  assert.equal(fs.existsSync(stateFile(root, 'sessions')), false);
  assert.equal(command('session').workspace.status, 'absent');
  const first = command('workspace-write', '--data', input(root, { name: 'result.json', content: '{"verified":true}' }));
  const other = command('workspace-open', '--operation', 'worker-1');
  assert.notEqual(first.scratchDirectory, other.scratchDirectory);
  const promoted = command('workspace-promote', '--data', input(root, { name: 'result.json', purpose: 'acceptance' }));
  assert.equal(promoted.evidence.artifacts, 1);
  assert.ok(JSON.parse(fs.readFileSync(path.join(first.directory, 'owner.json'), 'utf8')).evidence[0].sourceAtPromotion);
  assert.equal(command('workspace-release', '--operation', 'worker-1').scratch, 'retained', 'the main scope is still open');
  assert.equal(accept().workspace.scratch, 'cleaned');
  assert.equal(command('session').workspace.evidence.artifacts, 1);
  assert.equal(fs.existsSync(path.join(first.directory, 'scratch')), false);
  assert.equal(fs.readdirSync(path.join(first.directory, 'evidence')).length, 1);
  assert.equal(workspaceBoundary(root, 'session', 'task', 'complete').scratch, 'cleaned');
});

test('workspace bounds, settings merge and ownership checks fail closed', t => {
  const { root, command, start } = fixture(t); start();
  const settings = decide('effective-settings', { project: { formatVersion: 1, workspace: { scratch: { maxBytes: 4 } } } }).settings;
  assert.equal(settings.workspace.evidence.retentionDays, 7);
  fs.mkdirSync(path.join(root, '.craft'), { recursive: true });
  fs.writeFileSync(path.join(root, '.craft/project.json'), JSON.stringify({ formatVersion: 1, workspace: { scratch: { maxBytes: 4 } } }));
  const write = (name, content = 'x') => command('workspace-write', '--data', input(root, { name, content }));
  assert.throws(() => write('large', '12345'), /budget/);
  for (const name of ['../outside', '/tmp/outside']) assert.throws(() => write(name), /safe filename/);
  assert.throws(() => write('x'.repeat(101)), /100 characters/);
  write('x'.repeat(100));
  assert.equal(command('workspace-promote', '--data', input(root, { name: 'x'.repeat(100), purpose: 'boundary' })).evidence.artifacts, 1);
  assert.throws(() => command('workspace-open', '--data', input(root, ['not', 'an', 'object'])), /JSON object/);
  assert.equal(command('workspace-open', '--preferences', input(root, { workspace: { enabled: false } })).status, 'disabled');
  const opened = command('workspace-open');
  const outside = path.join(root, 'outside'); fs.mkdirSync(outside); fs.writeFileSync(path.join(outside, 'keep'), 'keep');
  fs.symlinkSync(outside, path.join(opened.scratchDirectory, 'escape'));
  assert.equal(workspaceBoundary(root, 'session', 'task', 'complete').status, 'retained');
  assert.throws(() => command('workspace-release'), /symlink/);
  assert.equal(fs.readFileSync(path.join(outside, 'keep'), 'utf8'), 'keep');
  fs.unlinkSync(path.join(opened.scratchDirectory, 'escape'));
  fs.writeFileSync(path.join(opened.directory, 'owner.json'), '{"owner":"someone-else"}');
  assert.match(workspaceBoundary(root, 'session', 'task', 'complete').reason, /ownership/);
  assert.throws(() => command('workspace-open'), /ownership/);
  assert.ok(fs.existsSync(opened.directory));
});

test('stale cleanup retains live owners and reclaims abandoned scratch and aged evidence', t => {
  const { root, command, start } = fixture(t); start();
  const policy = structuredClone(DEFAULT_SETTINGS.workspace), now = Date.now();
  const opened = command('workspace-write', '--operation', 'scope', '--data', input(root, { name: 'note', content: 'intermediate' }));
  setOwners(opened.directory, process.pid);
  workspaceOperation(root, null, policy, 'maintain', now + 2 * 86400000);
  assert.ok(fs.existsSync(opened.scratchDirectory), 'age cannot remove live owners');
  setOwners(opened.directory, 2147483647);
  const cleaned = workspaceOperation(root, null, policy, 'maintain', now + 2 * 86400000);
  assert.equal(cleaned.cleaned[0].scratch, 'cleaned');
  assert.equal(fs.existsSync(path.join(opened.directory, 'scratch')), false);
  workspaceOperation(root, null, policy, 'maintain', now + 8 * 86400000);
  assert.equal(fs.existsSync(opened.directory), false);
});

for (const privateDirectory of ['.craft', '.capabilities']) test(`${privateDirectory} artifacts are excluded from lookup, routing, snapshots, review and export without ignores`, t => {
  const { root, git } = fixture(t);
  const repo = path.join(root, 'kit'), secret = path.join(repo, `nested/${privateDirectory}/state/sessions/task/scratch/tempting.py`);
  fs.mkdirSync(path.dirname(secret), { recursive: true }); fs.writeFileSync(secret, 'PRIVATE_NEEDLE\n');
  const before = repositoryState(repo);
  fs.appendFileSync(secret, 'more private content\n');
  const after = repositoryState(repo);
  assert.equal(before.fingerprints.workingTree, after.fingerprints.workingTree);
  assert.deepEqual(before.status, []);
  const result = nativeLocate(repo, { terms: ['PRIVATE_NEEDLE'], paths: ['.'], maxFiles: 100, maxMatches: 10, maxBytes: 10000 });
  assert.deepEqual(result.matches, []);
  assert.equal(result.filesScanned, 1);
  const catalog = JSON.parse(fs.readFileSync(new URL('../engine/routing/catalog.json', import.meta.url)));
  assert.deepEqual(decide('route', { intent: 'implement', files: [`nested/${privateDirectory}/state/tempting.py`], project: { formatVersion: 1 }, catalog }).files, []);
  const head = git('kit', 'rev-parse', 'HEAD'); git('kit', 'add', '.'); git('kit', 'commit', '-qm', 'private fixture');
  const review = reviewSnapshot({ root: repo, base: head, head: 'HEAD' });
  assert.deepEqual(review.files, []); assert.equal(review.diff, '');
  const output = path.join(root, 'export'); copy(repo, output);
  assert.equal(fs.existsSync(path.join(output, `nested/${privateDirectory}/state`)), false);
  assert.equal(fs.existsSync(path.join(output, '.git')), false);
});

test('workspace operations retain other owners, recover only dead owned locks and enforce count limits', t => {
  const { root, command, start } = fixture(t); start();
  fs.mkdirSync(path.join(root, '.craft'), { recursive: true });
  fs.writeFileSync(path.join(root, '.craft/project.json'), JSON.stringify({ formatVersion: 1, workspace: { maxSessions: 1 } }));
  const policy = { ...DEFAULT_SETTINGS.workspace, maxSessions: 1 }, identity = mainScope();
  const opened = command('workspace-open');
  const ownerFile = path.join(opened.directory, 'owner.json');
  const owner = JSON.parse(fs.readFileSync(ownerFile));
  owner.operations[digest('other-worker')] = { pid: process.ppid, closed: false };
  fs.writeFileSync(ownerFile, JSON.stringify(owner));
  assert.equal(workspaceOperation(root, identity, policy, 'complete').scratch, 'retained');
  assert.ok(fs.existsSync(opened.scratchDirectory));
  assert.throws(() => command('workspace-write', '--operation', 'other-worker', '--data', input(root, { name: 'collision', content: 'x' })), /another active owner/);
  start(['kit'], 'another');
  assert.throws(() => command('workspace-open'), /capacity/);
  start();
  const lock = `${stateFile(root, 'workspace-guard.json')}.lock`;
  fs.writeFileSync(lock, ''); // Another writer has created but not yet published its lock.
  assert.equal(workspaceOperation(root, identity, policy, 'status').status, 'retained');
  assert.equal(command('workspace-open').status, 'retained');
  assert.equal(fs.existsSync(lock), true);
  fs.writeFileSync(lock, JSON.stringify({ owner: 'craft-workspace-lock', formatVersion: 1, pid: process.pid }));
  assert.equal(workspaceOperation(root, identity, policy, 'complete').status, 'retained');
  assert.equal(command('workspace-open').status, 'retained');
  fs.writeFileSync(lock, JSON.stringify({ owner: 'craft-workspace-lock', formatVersion: 1, pid: 2147483647 }));
  assert.equal(workspaceOperation(root, identity, policy, 'status').status, 'available');
  assert.equal(fs.existsSync(lock), false);
  fs.writeFileSync(lock, JSON.stringify({ owner: 'craft-workspace-lock', formatVersion: 1, pid: 2147483647 }));
  assert.equal(command('workspace-open').status, process.platform === 'win32' ? 'retained' : 'available', 'Windows cannot observe the holder; a fresh lock waits');
  assert.equal(fs.existsSync(lock), process.platform === 'win32');
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
  assert.equal(fs.existsSync(path.join(root, 'linked/.craft')), false);
});

test('a new workspace at capacity evicts the oldest inactive one; unverified entries occupy capacity', t => {
  const { root, command, start } = fixture(t);
  fs.mkdirSync(path.join(root, '.craft'), { recursive: true });
  fs.writeFileSync(path.join(root, '.craft/project.json'), JSON.stringify({ formatVersion: 1, workspace: { maxSessions: 2 } }));
  const policy = { ...DEFAULT_SETTINGS.workspace, maxSessions: 2 };
  start(['kit'], 'old');
  const old = command('workspace-open'); // its craft owner has exited
  fs.mkdirSync(path.join(path.dirname(old.directory), 'unverified'));
  assert.deepEqual(workspaceOperation(root, null, policy, 'maintain').cleaned, [], 'maintain at capacity keeps the entry');
  start(['kit'], 'new');
  const opened = command('workspace-open');
  assert.equal(opened.status, 'available');
  assert.deepEqual(opened.cleaned.filter(item => item.removed), [{ entry: path.basename(old.directory), removed: true }]);
  setOwners(opened.directory, process.pid);
  start(['kit'], 'third');
  assert.throws(() => command('workspace-open'), /capacity/, 'live and unverified entries are never evicted');
});

test('cleanup reports only scratch it removed and touching a closed workspace keeps its retention clock', t => {
  const { root, command, start } = fixture(t); start();
  const policy = DEFAULT_SETTINGS.workspace, identity = mainScope(), now = Date.now() + 1000;
  const opened = command('workspace-write', '--data', input(root, { name: 'note', content: 'x' }));
  assert.equal(workspaceOperation(root, identity, policy, 'complete', now + 1).scratch, 'cleaned');
  const owner = () => JSON.parse(fs.readFileSync(path.join(opened.directory, 'owner.json')));
  assert.equal(workspaceOperation(root, identity, policy, 'touch', now + 5).status, 'available');
  assert.equal(owner().updated, now + 1);
  assert.deepEqual(workspaceOperation(root, null, policy, 'maintain', now + 6).cleaned, []);
  assert.deepEqual(run(['workspace-clean', '--root', root]).cleaned, []);
});

test('promotion counts unregistered evidence files against the artifact budget', t => {
  const { root, command, start } = fixture(t); start();
  fs.mkdirSync(path.join(root, '.craft'), { recursive: true });
  fs.writeFileSync(path.join(root, '.craft/project.json'), JSON.stringify({ formatVersion: 1, workspace: { evidence: { maxArtifacts: 1 } } }));
  const opened = command('workspace-write', '--data', input(root, { name: 'r', content: 'x' }));
  fs.mkdirSync(path.join(opened.directory, 'evidence'));
  fs.writeFileSync(path.join(opened.directory, 'evidence/stray'), '');
  assert.throws(() => command('workspace-promote', '--data', input(root, { name: 'r', purpose: 'acceptance' })), /unsafe or oversized evidence/);
});

test('capacity eviction refuses a workspace holding unknown files', t => {
  const { root, command, start } = fixture(t);
  fs.mkdirSync(path.join(root, '.craft'), { recursive: true });
  fs.writeFileSync(path.join(root, '.craft/project.json'), JSON.stringify({ formatVersion: 1, workspace: { maxSessions: 1 } }));
  start(['kit'], 'old');
  const old = command('workspace-open');
  fs.writeFileSync(path.join(old.directory, 'stray'), 'not managed');
  start(['kit'], 'new');
  assert.throws(() => command('workspace-open'), /capacity/);
  assert.ok(fs.existsSync(path.join(old.directory, 'stray')));
});

test('an exited workspace writer is reclaimable while its parent stays alive', t => {
  const { root, command, start } = fixture(t); start();
  const policy = DEFAULT_SETTINGS.workspace;
  const opened = command('workspace-write', '--data', input(root, { name: 'note', content: 'abandoned' }));
  const owner = JSON.parse(fs.readFileSync(path.join(opened.directory, 'owner.json')));
  assert.notEqual(owner.operations[digest('main')].pid, process.pid);
  const result = workspaceOperation(root, null, policy, 'maintain', Date.now() + 2 * 86400000);
  assert.equal(result.cleaned[0].scratch, 'cleaned');
  assert.equal(fs.existsSync(opened.scratchDirectory), false);
});

test('workspace creation refuses an unowned file where its directory belongs', t => {
  const { root, command, start } = fixture(t); start();
  fs.mkdirSync(path.dirname(stateFile(root, 'sessions')), { recursive: true });
  fs.writeFileSync(stateFile(root, 'sessions'), 'unowned');
  assert.throws(() => command('workspace-open'), /workspace directory must not be a symlink/);
});

test('concurrent native writers and JS boundaries serialize on one workspace lock', async t => {
  const { root, command, start } = fixture(t); start();
  command('workspace-open');
  const binary = process.env.CRAFT_NATIVE_BINARY;
  const module = new URL('../platform/node/workspace.mjs', import.meta.url).href;
  const policy = DEFAULT_SETTINGS.workspace, identity = mainScope();
  const run = (file, args) => new Promise((resolve, reject) => {
    const child = spawn(file, args);
    let output = '', error = '';
    child.stdout.on('data', data => output += data); child.stderr.on('data', data => error += data);
    child.on('error', reject); child.on('close', code => code === 0 ? resolve(output) : reject(new Error(error)));
  });
  const writers = Array.from({ length: 3 }, (_, worker) => (async () => {
    const results = [];
    for (let i = 0; i < 6; i++) {
      const data = input(root, { name: `note-${worker}-${i}`, content: `worker-${worker}` });
      results.push(JSON.parse(await run(binary, ['workspace-write', '--root', root, '--session', 'session', '--operation', `worker-${worker}`, '--data', data, '--json'])));
    }
    return results.map(value => ({ status: value.status, directory: value.directory }));
  })());
  const script = `import { workspaceOperation } from ${JSON.stringify(module)};
    const identities = [null, ${JSON.stringify(identity)}, ${JSON.stringify(identity)}], actions = ['maintain', 'touch', 'status'], results = [];
    for (let i = 0; i < 600; i++) results.push(workspaceOperation(${JSON.stringify(root)}, identities[i % 3], ${JSON.stringify(policy)}, actions[i % 3]).status);
    console.log(JSON.stringify(results));`;
  const boundaries = run(process.execPath, ['--input-type=module', '-e', script]).then(JSON.parse);
  const [written, observed] = await Promise.all([Promise.all(writers), boundaries]);
  const all = written.flat(), accepted = all.filter(value => value.status === 'available');
  const counts = observed.reduce((sum, status) => ({ ...sum, [status]: (sum[status] ?? 0) + 1 }), {});
  t.diagnostic(`${all.length} native writes: ${accepted.length} accepted, ${all.length - accepted.length} busy; JS boundaries ${JSON.stringify(counts)}`);
  assert.ok(accepted.length > 0);
  assert.ok(all.every(value => ['available', 'retained'].includes(value.status)));
  assert.ok(observed.every(status => ['inspected', 'available', 'retained'].includes(status)));
  const directory = accepted[0].directory;
  const owner = JSON.parse(fs.readFileSync(path.join(directory, 'owner.json')));
  assert.ok(Object.keys(owner.operations).length > 0);
  const files = fs.readdirSync(path.join(directory, 'scratch'), { recursive: true }).filter(file => file.includes('note-'));
  assert.equal(files.length, accepted.length);
  assert.equal(workspaceOperation(root, identity, policy, 'status').status, 'available');
  assert.equal(workspaceOperation(root, null, policy, 'maintain', Date.now() + 2 * 86400000).cleaned[0].scratch, 'cleaned');
});

test('structured criterion evidence binds all selected repositories and reports missing roots as unknown', t => {
  const { root, command } = fixture(t);
  const contract = JSON.parse(fs.readFileSync(new URL('./fixtures/acceptance/contract.json', import.meta.url), 'utf8'));
  const contractFile = path.join(root, 'contract.json'), ledgerFile = path.join(root, 'ledger.json');
  fs.writeFileSync(contractFile, JSON.stringify(contract));
  command('task-start', '--task', 'task', '--goal', 'Update selected repositories', '--repository', 'kit', '--repository', 'profile', '--contract', contractFile);
  const context = command('session');
  const records = contract.criteria.map(c => ({ id: c.id, criterionIds: [c.id], scope: c.scope,
    contract: decide('engineering-contract-digest', { contract }), binding: context.binding,
    checkId: 'value-check', kind: 'check', artifact: 'retained/checks', observedAt: '2026-10-22T00:00:00Z',
    reporter: 'fixture', basis: c.description, support: 'verified', execution: 'passed' }));
  fs.writeFileSync(ledgerFile, JSON.stringify({ formatVersion: 1, records }));
  const result = command('task-step', '--stage', 'verify', '--target', 'criteria', '--approach', 'assessed', '--evidence', 'checks', '--outcome', 'passed',
    '--result', 'Criteria checked', '--next', 'Return result', '--acceptance-evidence', ledgerFile,
    '--expected-task-contract', context.taskContract, '--expected-evidence-digest', context.evidenceDigest, '--complete');
  assert.equal(result.complete, true);
  fs.appendFileSync(path.join(root, 'profile/value.mjs'), '// edit in the second selected repository\n');
  assert.equal(command('session').acceptance.complete, false);
  assert.equal(command('session').acceptance.records[0].freshness, 'stale');
  fs.renameSync(path.join(root, 'profile'), path.join(root, 'moved-profile'));
  const missing = command('session', '--task', 'task');
  assert.equal(missing.acceptance.complete, false);
  assert.equal(missing.acceptance.records[0].freshness, 'unknown');
});
