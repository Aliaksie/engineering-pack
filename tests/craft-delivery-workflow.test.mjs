import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { spawnSync } from 'node:child_process';
import { run } from '../cli/main.mjs';
import { route } from './native-route.mjs';
import { readProject } from '../application/project/configuration.mjs';
import { prepareSnapshot, checkReview } from './review-tools.mjs';

function command(binary, args, cwd) {
  return spawnSync(binary, args, { cwd, shell: false, encoding: 'utf8' });
}
function fixture(t) {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'delivery workflow λ '));
  t.after(() => fs.rmSync(root, { recursive: true, force: true }));
  const write = (name, text) => { const file = path.join(root, name); fs.mkdirSync(path.dirname(file), { recursive: true }); fs.writeFileSync(file, text); };
  const git = (...args) => {
    const result = command('git', ['-c', `core.hooksPath=${path.join(root, 'no-hooks')}`, '-c', 'commit.gpgsign=false', '-c', 'user.name=Workflow Fixture', '-c', 'user.email=fixture@example.test', ...args], root);
    assert.equal(result.status, 0, result.stderr); return result.stdout.trim();
  };
  git('init', '-q');
  write('.gitignore', '.craft/state/\n');
  write('AGENTS.md', 'Use Node built-in checks. Local implementation only; no publication.\n');
  write('src/total.mjs', 'export const total = (price, quantity) => price + quantity;\n');
  write('checks/total.mjs', "import assert from 'node:assert/strict';\nimport { total } from '../src/total.mjs';\nassert.equal(total(3, 4), 12);\n");
  write('.craft/project.json', JSON.stringify({ formatVersion: 1, modules: [{ name: 'billing', path: 'src', languages: ['javascript'], runtimes: ['node'], layers: ['backend'], checks: [{ command: process.execPath, args: ['../checks/total.mjs'] }] }] }));
  git('add', '--', '.'); git('commit', '-qm', 'baseline');
  return { root, write, git, base: git('rev-parse', 'HEAD') };
}

test('delivery tool path supports investigate/plan → implement → actual verification → revision-bound review', (t) => {
  const { root, write, git, base } = fixture(t);
  const project = readProject(root);
  for (const intent of ['investigate', 'plan', 'implement', 'verify', 'review']) {
    const result = run(['route', '--root', root, '--intent', intent, '--file', 'src/total.mjs']);
    assert.equal(result.loading.workflow, `workflow:${intent}`);
    assert.equal(result.modules[0].name, 'billing');
    assert.equal(result.checks[0].execution, 'not-run');
    assert.ok(!result.loading.specializations.includes('framework:react'));
  }
  const planned = route({ intent: 'implement', files: ['src/total.mjs'], project });
  const check = planned.checks[0];
  const checkDirectory = path.resolve(root, check.cwd);
  assert.equal(checkDirectory, path.join(root, 'src'));
  const before = command(check.command, check.args, checkDirectory);
  assert.notEqual(before.status, 0, 'acceptance check must expose the defect');
  assert.match(before.stderr, /AssertionError/, 'failure must be the acceptance assertion, not a missing script');
  write('src/total.mjs', 'export const total = (price, quantity) => price * quantity;\n');
  assert.equal(command(check.command, check.args, checkDirectory).status, 0, 'actual configured check verifies the corrected result');
  git('add', '--', 'src/total.mjs'); git('commit', '-qm', 'correct multiplication');
  const head = git('rev-parse', 'HEAD');
  const artifacts = fs.mkdtempSync(path.join(os.tmpdir(), 'delivery review '));
  t.after(() => fs.rmSync(artifacts, { recursive: true, force: true }));
  const input = prepareSnapshot(artifacts, { root, base, head });
  assert.deepEqual(input.files, ['src/total.mjs']);
  assert.equal(checkReview(artifacts, input, { baseSha: base, headSha: head, summary: 'Fixture tool-chain review', recommendation: 'needs-verification', findings: [], verification: ['Node assertion command ran against the committed fixture; no semantic model review performed.'] }).valid, true);
  const saved = run(['checkpoint-save', '--root', root, '--session', 'delivery']);
  assert.equal(saved.state.head, head);
  assert.equal(run(['checkpoint-read', '--root', root, '--session', 'delivery']).changed, false);
  write('src/total.mjs', 'export const total = () => 0;\n');
  assert.equal(run(['checkpoint-read', '--root', root, '--session', 'delivery']).changed, true);
});

test('local story and missing optional guidance do not require a tracker or authorize writes', (t) => {
  const { root } = fixture(t);
  const story = run(['route', '--root', root, '--intent', 'story', '--file', 'src/total.mjs']);
  assert.ok(story.skills.every((skill) => skill.startsWith('craft-stories:')));
  assert.deepEqual(story.loading.specializations, []);
  assert.deepEqual(story.operations, []);
  const tracker = run(['route', '--root', root, '--intent', 'plan', '--task', 'TASK-1']);
  assert.ok(tracker.missing.some((item) => item.includes('missing tasks binding')));
  assert.throws(() => run(['operation', '--root', root, '--operation', 'git.push']), /not enabled/);
});

test('each host export exposes the standalone delivery CLI without shell entry points', (t) => {
  const { root } = fixture(t);
  for (const host of ['claude', 'codex', 'cursor', 'copilot', 'gemini', 'agents']) {
    const folder = host === 'claude' ? 'plugins' : 'packages';
    const cli = path.resolve('dist', host, folder, 'craft-delivery/support/cli/main.mjs');
    const result = command(process.execPath, [cli, 'route', '--root', root, '--intent', 'implement', '--file', 'src/total.mjs'], root);
    assert.equal(result.status, 0, result.stderr);
    assert.equal(JSON.parse(result.stdout).loading.workflow, 'workflow:implement');
    const located = command(process.execPath, [cli, 'locate', '--root', root, '--path', 'src', '--term', 'total', '--max-matches', '2'], root);
    assert.equal(located.status, 0, located.stderr);
    assert.equal(JSON.parse(located.stdout).matches[0].path, 'src/total.mjs');
    const prepared = command(process.execPath, [cli, 'prepare-services', '--root', root, '--available-command', 'git'], root);
    assert.equal(prepared.status, 0, prepared.stderr);
    const status = JSON.parse(prepared.stdout).services[0].operations.find(entry => entry.operation === 'git.status');
    assert.equal(status.availability, 'observed-available');
  }
});
