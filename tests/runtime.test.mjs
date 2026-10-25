import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import test from 'node:test';
import assert from 'node:assert/strict';
import { fileURLToPath } from 'node:url';
import { spawnSync } from 'node:child_process';
import { route } from './native-route.mjs';
import { readProject } from '../application/project/configuration.mjs';
import { validateProject } from '../application/project/configuration.mjs';
import { saveCheckpoint, readCheckpoint, recoverCheckpoint } from './checkpoint-cli.mjs';
import { run } from '../cli/main.mjs';
import { snapshot, validateReview, publicationPlan } from '../application/review.mjs';
import { loadCapabilities } from '../tooling/lib/capabilities.mjs';

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const project = readProject(path.join(root, 'examples/project'));
const copy = (value) => structuredClone(value);
function temp(t) {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'craft runtime '));
  t.after(() => fs.rmSync(dir, { recursive: true, force: true }));
  return dir;
}
function git(root, ...args) {
  const result = spawnSync('git', ['-c', `core.hooksPath=${path.join(root, 'no-hooks')}`, '-c', 'commit.gpgsign=false', '-c', 'user.name=Fixture', '-c', 'user.email=fixture@example.test', '-C', root, ...args], { encoding: 'utf8' });
  assert.equal(result.status, 0, result.stderr);
  return result.stdout.trim();
}
// `lookup` runs in the native runtime; select through the public command line.
function lookup(config, { file, module } = {}) {
  const configured = fs.mkdtempSync(path.join(os.tmpdir(), 'craft-lookup-'));
  try {
    fs.mkdirSync(path.join(configured, '.craft'));
    fs.writeFileSync(path.join(configured, '.craft/project.json'), JSON.stringify(config));
    return run(['lookup', '--root', configured, '--preferences', path.join(configured, 'absent.json'),
      ...(file === undefined ? [] : ['--file', file]), ...(module === undefined ? [] : ['--module', module])]);
  } finally { fs.rmSync(configured, { recursive: true, force: true }); }
}
function repository(t) {
  const dir = temp(t);
  git(dir, 'init', '-q');
  fs.writeFileSync(path.join(dir, '.gitignore'), '.craft/state/\n');
  fs.writeFileSync(path.join(dir, 'orders.py'), 'value = 1\n');
  git(dir, 'add', '--', '.gitignore', 'orders.py'); git(dir, 'commit', '-qm', 'base');
  const base = git(dir, 'rev-parse', 'HEAD');
  fs.writeFileSync(path.join(dir, 'orders.py'), 'value = 2\n');
  git(dir, 'add', '--', 'orders.py'); git(dir, 'commit', '-qm', 'head');
  return { dir, base, head: git(dir, 'rev-parse', 'HEAD') };
}

test('Java backend routing stays independent of UI language and agent provider', () => {
  const result = route({ intent: 'review', files: ['services/orders/Order.java'], project });
  assert.ok(result.references.includes('craft-engineering:foundations/references/languages/jvm.md'));
  assert.ok(result.skills.includes('craft-engineering:backend-foundations'));
  assert.ok(!result.references.includes('craft-engineering:foundations/references/languages/typescript.md'));
  assert.ok(!result.skills.includes('craft-engineering:react-engineering'));
  assert.deepEqual(result.references, ['craft-engineering:foundations/references/languages/jvm.md']);
  assert.equal(result.modules[0].owner, 'orders-team');
  assert.deepEqual(new Set(result.affectedDependents), new Set(['console', 'assistant']));
  assert.equal(result.checks[0].execution, 'not-run');
});

test('UI and agent routing loads only the relevant layer/provider disciplines', () => {
  const ui = route({ intent: 'implement', files: ['ui/Orders.tsx'], project });
  assert.ok(ui.skills.includes('craft-engineering:ui-foundations'));
  assert.ok(ui.skills.includes('craft-engineering:react-engineering'));
  assert.deepEqual(ui.references, ['craft-engineering:foundations/references/languages/typescript.md']);
  const config = { ...copy(project), providers: ['bedrock'] };
  const agent = route({ intent: 'implement', files: ['agents/assistant/main.py'], project: config });
  assert.ok(agent.references.includes('craft-engineering:foundations/references/languages/python.md'));
  assert.ok(agent.skills.includes('craft-engineering:evaluate-agent'));
  assert.deepEqual(new Set(agent.references), new Set(['craft-engineering:design-agent/references/foundry.md', 'craft-engineering:foundations/references/languages/python.md']));
});

test('module lookup uses path boundaries, specificity and transitive declared impact', () => {
  const config = { formatVersion: 1, modules: [
    { name: 'root', path: '.', languages: ['typescript'] },
    { name: 'api', path: 'src/api', languages: ['jvm'] },
    { name: 'client', path: 'client', dependsOn: ['api'] },
    { name: 'console', path: 'console', dependsOn: ['client'] }
  ] };
  assert.deepEqual(lookup(config, { file: 'src/api2/a.java' }).modules.map((entry) => entry.name), ['root']);
  assert.deepEqual(lookup(config, { file: 'src/api/a.java' }).modules.map((entry) => entry.name), ['api']);
  const result = route({ intent: 'review', files: ['src/api/a.java'], project: config });
  assert.deepEqual(result.languages, ['jvm']);
  assert.deepEqual(result.affectedDependents, ['client', 'console']);
  assert.throws(() => lookup(config, { module: 'absent' }), /unknown module/);
  assert.throws(() => lookup(config, { module: '' }), /unknown module/);
  assert.throws(() => lookup(config, { module: 'api', file: 'src/api/a.java' }), /choose --file or --module/);
  const files = fs.mkdtempSync(path.join(os.tmpdir(), 'craft-lookup-'));
  try {
    fs.mkdirSync(path.join(files, '.craft'));
    fs.writeFileSync(path.join(files, '.craft/project.json'), JSON.stringify(config));
    const selected = run(['lookup', '--root', files, '--preferences', path.join(files, 'absent.json'), '--file', 'client/a', '--file', '.git/b', '--file', 'src/api/c']);
    assert.deepEqual(selected.modules.map((entry) => entry.name), ['client', 'api']);
  } finally { fs.rmSync(files, { recursive: true, force: true }); }
});

test('native route takes the task from a session binding and records session activity', t => {
  const dir = temp(t);
  git(dir, 'init', '-q');
  fs.mkdirSync(path.join(dir, '.craft'));
  fs.writeFileSync(path.join(dir, '.craft/project.json'), JSON.stringify({ formatVersion: 1,
    services: { tasks: { provider: 'tracker', transport: 'cli', command: 'tracker', operations: { 'task.read': { args: ['read', '{id}'] } } } } }));
  const routed = (...args) => run(['route', '--root', dir, '--intent', 'implement', ...args]);
  assert.deepEqual(routed('--session', 's1').operations, []);
  run(['task-start', '--root', dir, '--session', 's1', '--task', 'T-1', '--goal', 'route the bound task']);
  const activity = (task) => run(['session', '--root', dir, '--session', 's1', '--task', task]).activity.routingDecisions ?? 0;
  const started = activity('T-1');
  assert.deepEqual(routed('--session', 's1').operations.map((operation) => operation.args), [['read', 'T-1']]);
  assert.deepEqual(routed('--session', 's1', '--task', 'T-2').operations.map((operation) => operation.args), [['read', 'T-2']]);
  assert.equal(activity('T-1'), started + 1);
  assert.equal(activity('T-2'), 1);
});

test('equivalent module paths preserve ownership, checks and stack guidance without changing configuration', () => {
  const canonical = { formatVersion: 1, modules: [
    { name: 'root', path: '.', languages: ['python'] },
    { name: 'api', path: 'src', owner: 'api-team', languages: ['jvm'], layers: ['backend'],
      skills: ['craft-engineering:testing-engineering'], checks: [{ command: 'verify', args: ['--all'] }] },
    { name: 'consumer', path: 'consumer', dependsOn: ['api'] }
  ] };
  const dotted = copy(canonical);
  dotted.modules[1].path = '././src';
  const original = copy(dotted);
  const expected = route({ intent: 'review', files: ['src/main.java'], project: canonical });
  const actual = route({ intent: 'review', files: ['./src/./main.java'], project: dotted });
  assert.deepEqual(actual, expected);
  assert.deepEqual(actual.modules, [{ name: 'api', path: 'src', owner: 'api-team' }]);
  assert.deepEqual(actual.checks, [{ module: 'api', cwd: 'src', command: 'verify', args: ['--all'], execution: 'not-run' }]);
  assert.ok(actual.references.includes('craft-engineering:foundations/references/languages/jvm.md'));
  assert.ok(actual.skills.includes('craft-engineering:backend-foundations'));
  assert.ok(actual.skills.includes('craft-engineering:testing-engineering'));
  assert.deepEqual(actual.affectedDependents, ['consumer']);
  for (const selector of [{ file: 'src/main.java' }, { module: 'api' }]) {
    assert.deepEqual(lookup(dotted, selector), lookup(canonical, selector));
  }
  assert.deepEqual(lookup(dotted), lookup(canonical));
  assert.deepEqual(dotted, original);
});

test('deterministic tasks do not request a model and missing services are explicit', () => {
  const status = route({ intent: 'status' });
  assert.equal(status.execution.mode, 'tool');
  assert.deepEqual(status.skills, []);
  assert.deepEqual(status.operations[0].args, ['status', '--short']);
  assert.equal(route({ intent: 'locate' }).execution.mode, 'tool');
  const task = route({ intent: 'implement', task: 'TEAM-1' });
  assert.match(task.missing[0], /missing tasks binding/);
  assert.throws(() => route({ intent: 'invent' }), /unknown intent/);
  assert.throws(() => route({ intent: 'review', files: ['../other/private'] }), /relative/);
});

test('delegation requires explicit selection, role/host support and model mapping', () => {
  const config = { formatVersion: 1, execution: { roles: { researcher: { mode: 'delegate', profile: 'bounded' } }, hosts: { codex: { delegation: true, models: { bounded: 'fixture-approved-model' } } } } };
  assert.equal(route({ intent: 'research', project: config, host: 'codex' }).execution.mode, 'main');
  const selected = route({ intent: 'research', project: config, host: 'codex', delegate: true });
  assert.equal(selected.execution.mode, 'delegate');
  assert.equal(selected.execution.model, 'fixture-approved-model');
  assert.equal(route({ intent: 'research', project: config, host: 'unknown', delegate: true }).execution.mode, 'main');
  delete config.execution.hosts.codex.models.bounded;
  const missing = route({ intent: 'research', project: config, host: 'codex', delegate: true });
  assert.equal(missing.execution.mode, 'main');
  assert.match(missing.missing[0], /model mapping/);
});

test('locator starts in tools and selects an economical worker only through explicit host policy', () => {
  const config = { formatVersion: 1, execution: { roles: { locator: { mode: 'delegate', profile: 'lookup' } }, hosts: { agents: { delegation: true, models: { lookup: 'consumer-lookup-model' } } } } };
  assert.equal(route({ intent: 'locate', project: config }).execution.mode, 'tool');
  const worker = route({ intent: 'locate', project: config, delegate: true });
  assert.equal(worker.execution.mode, 'delegate');
  assert.equal(worker.execution.profile, 'lookup');
  assert.equal(worker.execution.model, 'consumer-lookup-model');
  assert.throws(() => validateProject({ formatVersion: 1, routing: { editHints: 'yes' } }), /editHints/);
});

test('all built-in routed skills resolve to actual capability source', () => {
  const ids = new Set(loadCapabilities(root).flatMap(({ manifest }) => [
    ...manifest.skills.map((folder) => `${manifest.name}:${path.basename(folder)}`),
    ...manifest.workflows.map((file) => `${manifest.name}:workflow-${path.basename(file, '.md')}`)
  ]));
  for (const intent of ['implement', 'review', 'plan', 'story', 'research', 'verify', 'locate', 'status', 'explore']) {
    const result = route({ intent, files: ['agents/assistant/main.py', 'ui/Orders.tsx', 'services/orders/Order.java'], project, risks: ['auth'] });
    for (const skill of result.skills) assert.ok(ids.has(skill), skill);
    assert.deepEqual(result.missing, []);
  }
});

// `operation` runs in the native runtime; plan through the public command line.
function planOperation(config, name, params) {
  const configured = typeof config === 'string' ? config : fs.mkdtempSync(path.join(os.tmpdir(), 'craft-operation-'));
  try {
    if (configured !== config) {
      fs.mkdirSync(path.join(configured, '.craft'));
      fs.writeFileSync(path.join(configured, '.craft/project.json'), JSON.stringify(config));
    }
    return run(['operation', '--root', configured, '--preferences', path.join(configured, 'absent.json'), '--operation', name, '--params', JSON.stringify(params)]);
  } finally { if (configured !== config) fs.rmSync(configured, { recursive: true, force: true }); }
}
const example = path.join(root, 'examples/project');

test('bindings distinguish local Git, codehost and tracker and preserve argument boundaries', () => {
  const github = { formatVersion: 1, services: { codehost: { provider: 'github', transport: 'cli', command: 'gh', host: 'github.example.com', repository: 'team/repo' } } };
  assert.deepEqual(planOperation(github, 'change.read', { id: 42 }).args, ['pr', 'view', '42', '--repo', 'github.example.com/team/repo', '--json', 'number,title,body,baseRefOid,headRefOid']);
  const gitlab = planOperation(example, 'change.read', { id: 42 });
  assert.deepEqual(gitlab.args, ['mr', 'view', '42', '--repo', 'https://gitlab.example.com/team/project', '--output', 'json']);
  const hostile = 'TEAM-1; $(touch /tmp/never-run)';
  const cli = planOperation(example, 'task.read', { id: hostile });
  assert.deepEqual(cli.args, ['read', hostile]);
  assert.equal(cli.shell, false);
  assert.equal(cli.availability, 'not-verified');
  const mcp = { formatVersion: 1, services: { tasks: { provider: 'tracker', transport: 'mcp', operations: { 'task.read': { tool: 'mcp__tracker__read', arguments: { issueId: '{id}' } } } } } };
  assert.deepEqual(planOperation(mcp, 'task.read', { id: hostile }).arguments, { issueId: hostile });
  assert.throws(() => planOperation(github, 'change.read', { id: '--help' }), /positive integer/);
});

test('write plans require policy and still do not confer user/job authorization', () => {
  const config = { formatVersion: 1, services: { tasks: { provider: 'tracker', transport: 'cli', command: 'taskctl', operations: { 'task.transition': { args: ['transition', '{id}', '{state}'] } } } } };
  assert.throws(() => planOperation(config, 'task.transition', { id: 'T-1', state: 'Done' }), /not enabled/);
  config.permissions = { writeOperations: ['task.transition'] };
  const result = planOperation(config, 'task.transition', { id: 'T-1', state: 'Done' });
  assert.equal(result.effect, 'write');
  assert.equal(result.authorization, 'explicit-user-or-job-approval-required');
  assert.throws(() => planOperation(config, 'task.transition', { id: 'T-1' }), /missing parameter/);
});

test('invalid configuration is rejected without silently selecting another integration', (t) => {
  for (const config of [
    { formatVersion: 2 }, { formatVersion: 1, surprise: true },
    { formatVersion: 1, modules: [{ name: 'escape', path: '../outside' }] },
    { formatVersion: 1, modules: [{ name: 'api', path: 'api', dependsOn: ['unknown'] }] },
    { formatVersion: 1, services: { tasks: { provider: 'tracker', transport: 'mcp', operations: { 'task.read': { tool: 'guess' } } } } },
    { formatVersion: 1, checkpoint: { enabled: 'yes' } }
  ]) assert.throws(() => validateProject(config));
  const dir = temp(t); fs.mkdirSync(path.join(dir, '.craft'));
  const source = path.join(dir, 'outside.json'); fs.writeFileSync(source, '{"formatVersion":1}');
  fs.symlinkSync(source, path.join(dir, '.craft/project.json'));
  assert.throws(() => readProject(dir), /symlink/);
  fs.unlinkSync(source);
  assert.throws(() => readProject(dir), /symlink/, 'a broken symlink must not silently disable configuration');
  assert.throws(() => run(['route', '--intent', 'review', '--typo']), /unknown argument/);
});

test('checkpoints preserve explicit task metadata and detect changed working trees', (t) => {
  const { dir, head } = repository(t);
  saveCheckpoint(dir, 'session/../../data', { task: 'Review orders', authorization: 'read-only review', nextStep: 'inspect retry path', sources: ['AGENTS.md'] });
  const saved = readCheckpoint(dir, 'session/../../data');
  assert.equal(saved.head, head);
  assert.equal(saved.gitAvailable, true);
  saveCheckpoint(dir, 'session/../../data');
  assert.equal(readCheckpoint(dir, 'session/../../data').authorization, 'read-only review');
  assert.equal(recoverCheckpoint(dir, 'session/../../data').changed, false);
  fs.appendFileSync(path.join(dir, 'orders.py'), 'more = True\n');
  assert.equal(recoverCheckpoint(dir, 'session/../../data').changed, true);
  assert.throws(() => saveCheckpoint(dir, 's', { prompt: 'do not capture transcripts' }), /unknown checkpoint/);
  assert.equal(fs.existsSync(path.join(dir, 'data.json')), false);
});

test('checkpoint writes reject symlinked paths and concurrent writers', (t) => {
  const dir = temp(t); const outside = temp(t);
  fs.symlinkSync(outside, path.join(dir, '.craft'), process.platform === 'win32' ? 'junction' : 'dir');
  assert.throws(() => saveCheckpoint(dir, 's'), /symlink/);
  fs.unlinkSync(path.join(dir, '.craft'));
  const saved = saveCheckpoint(dir, 's');
  fs.writeFileSync(`${saved.file}.lock`, 'fixture lock');
  assert.throws(() => saveCheckpoint(dir, 's'), /busy/);
  assert.equal(readCheckpoint(dir, 's').formatVersion, 1);
});

test('review snapshots pin actual Git revisions without running project code', (t) => {
  const { dir, base, head } = repository(t);
  const input = snapshot({ root: dir, base, head, provider: 'gitlab', repository: 'team/repo', changeId: 42 });
  assert.equal(input.baseSha, base); assert.equal(input.headSha, head); assert.equal(input.mergeBase, base);
  assert.deepEqual(input.files, ['orders.py']); assert.match(input.diff, /value = 2/);
  const result = { baseSha: base, headSha: head, summary: 'Static review', recommendation: 'request-changes', verification: ['Tests not run.'], findings: [{ id: 'F1', severity: 'medium', path: 'orders.py', side: 'new', line: 1, summary: 'Fixture finding', evidence: 'Synthetic validation fixture.' }] };
  assert.equal(validateReview(input, result).valid, true);
  for (const bad of [
    { ...result, headSha: base }, { ...result, verification: [] },
    { ...result, findings: [{ ...result.findings[0], path: 'not-changed.py' }] },
    { ...result, findings: [{ ...result.findings[0], line: 0 }] },
    { ...result, findings: [...result.findings, result.findings[0]] }
  ]) assert.throws(() => validateReview(input, bad));
  assert.throws(() => publicationPlan(input, result, { currentHead: head, currentBase: base }), /authorization/);
  assert.throws(() => publicationPlan(input, result, { currentHead: head, currentBase: base, authorized: 'false' }), /authorization/);
  assert.throws(() => publicationPlan(input, result, { currentHead: base, currentBase: base, authorized: true }), /changed/);
  assert.equal(publicationPlan(input, result, { currentHead: head, currentBase: base, authorized: true }).execution, 'not-published');
});

test('review snapshot CLI saves a new artifact and prints only compact metadata', (t) => {
  const { dir, base, head } = repository(t);
  const command = path.join(root, 'packages/craft-review/bin/prepare-review.mjs');
  const artifact = path.join(dir, 'review-snapshot.json');
  const result = spawnSync(process.execPath, [command, '--root', dir, '--base', base, '--head', head, '--output', artifact], { encoding: 'utf8' });
  assert.equal(result.status, 0, result.stderr);
  assert.equal(JSON.parse(result.stdout).files, 1);
  assert.ok(!result.stdout.includes('value = 2'));
  assert.equal(JSON.parse(fs.readFileSync(artifact, 'utf8')).headSha, head);
  const overwrite = spawnSync(process.execPath, [command, '--root', dir, '--base', base, '--head', head, '--output', artifact], { encoding: 'utf8' });
  assert.equal(overwrite.status, 2);
});
