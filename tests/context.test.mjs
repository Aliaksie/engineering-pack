import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import test from 'node:test';
import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import { contextReport } from '../runtime/context.mjs';
import { route } from '../runtime/router.mjs';
import { run } from '../runtime/cli.mjs';

const root = path.resolve('.');
const exploration = () => route({ intent: 'explore' });
function temporary(t) {
  const directory = fs.mkdtempSync(path.join(os.tmpdir(), 'craft context '));
  t.after(() => fs.rmSync(directory, { recursive: true, force: true }));
  return directory;
}

test('context report hashes the selected files, deduplicates selections and reports rather than trims a budget overrun', () => {
  const plan = route({ intent: 'verify' });
  const before = structuredClone(plan);
  const report = contextReport(root, plan, { budgetBytes: 1 });
  assert.equal(report.complete, true);
  assert.equal(report.files.length, 3); // Testing workflow, discipline entry point and required package instructions.
  assert.equal(report.totals.repeatedSelections, 1);
  assert.equal(report.budget.status, 'exceeded');
  assert.equal(report.sessionUsage.currentTokens, null);
  assert.deepEqual(plan, before);
  let bytes = 0;
  for (const entry of report.files) {
    const content = fs.readFileSync(path.join(root, entry.path));
    bytes += content.length;
    assert.equal(entry.bytes, content.length);
    assert.equal(entry.sha256, createHash('sha256').update(content).digest('hex'));
  }
  assert.equal(report.totals.bytes, bytes);
  assert.deepEqual(report.files, contextReport(root, plan).files);
  assert.equal(contextReport(root, plan, { budgetBytes: bytes }).budget.status, 'within');
  assert.ok(report.files.every((entry) => !entry.path.includes('/references/')),
    'linked references are not implicitly loaded');
});

test('context report covers configured skills and leaves missing optional packages explicit', (t) => {
  const plan = route({ intent: 'implement', files: ['app/main.py'], project: { formatVersion: 1,
    modules: [{ name: 'app', path: 'app', skills: ['craft-delivery:write-adr'] }] } });
  assert.ok(contextReport(root, plan).files.some(({ target }) => target === 'craft-delivery:write-adr'));
  const directory = temporary(t);
  fs.cpSync('packages/craft-engineering', directory, { recursive: true });
  const partial = contextReport(directory, plan, { budgetBytes: 1000000 });
  assert.equal(partial.complete, false);
  assert.ok(partial.missing.some(({ target }) => target === 'craft-delivery:apply-change'));
  assert.equal(partial.budget.status, 'unknown');
  assert.ok(partial.totals.bytes > 0);
});

test('context reporting supports every export and relocated standalone runtime without authoring imports', (t) => {
  const directory = temporary(t);
  for (const host of ['claude', 'codex', 'agents', 'cursor', 'copilot', 'gemini', 'gemini-extension', 'jetbrains']) {
    const bundle = path.join(root, 'dist', host);
    const report = contextReport(bundle, exploration());
    assert.equal(report.complete, true, host);
    const packagePath = path.join(bundle, host === 'claude' ? 'plugins' : 'packages', 'craft-engineering');
    const relocated = path.join(directory, host);
    fs.cpSync(packagePath, relocated, { recursive: true });
    const processResult = spawnSync(process.execPath, [path.join(relocated, 'support/runtime/cli.mjs'),
      'context-report', '--root', directory, '--intent', 'explore', '--budget-bytes', '1'], { encoding: 'utf8' });
    assert.equal(processResult.status, 0, processResult.stderr);
    const standalone = JSON.parse(processResult.stdout);
    assert.equal(standalone.complete, true, host);
    assert.equal(standalone.totals.bytes, report.totals.bytes, host);
    assert.equal(standalone.budget.status, 'exceeded');
  }
});

test('context report rejects traversal, symlinked resources, oversized files and invalid budgets', (t) => {
  const directory = temporary(t);
  fs.cpSync('packages/craft-engineering', directory, { recursive: true });
  const entry = path.join(directory, 'workflows/explore-testing.md');
  fs.writeFileSync(entry, Buffer.alloc(1024 * 1024 + 1));
  assert.throws(() => contextReport(directory, exploration()), /bounded regular file/);
  for (const budgetBytes of [0, -1, NaN, Infinity, 1.5]) assert.throws(() => contextReport(root, exploration(), { budgetBytes }), /positive safe integer/);
  const outside = temporary(t);
  fs.rmSync(path.join(directory, 'workflows'), { recursive: true });
  fs.symlinkSync(outside, path.join(directory, 'workflows'), process.platform === 'win32' ? 'junction' : 'dir');
  assert.throws(() => contextReport(directory, exploration()), /symlink/);
  assert.throws(() => contextReport(root, { skills: ['craft-engineering:../secrets'] }), /invalid context target/);
  assert.throws(() => run(['context-report', '--intent', 'verify', '--budget-bytes', 'invalid']), /positive safe integer/);
});

test('story and exploration context stay independent of configured stack and risks', () => {
  const config = { formatVersion: 1, languages: ['typescript'], frameworks: ['react'],
    layers: ['agent', 'ui'], providers: ['bedrock'], runtimes: ['node'] };
  for (const intent of ['story', 'explore']) {
    const plan = route({ intent, project: config, files: ['app/main.tsx'], risks: ['auth', 'retry'] });
    const report = contextReport(root, plan);
    assert.equal(report.complete, true);
    assert.ok(report.files.every(({ path: file }) => !file.includes('/references/')));
    assert.equal(plan.loading.specializations.length, 0);
    const cli = run(['context-report', '--intent', intent, '--bundle', root]);
    assert.equal(cli.complete, true);
    assert.deepEqual(cli.sessionUsage, report.sessionUsage);
  }
});
