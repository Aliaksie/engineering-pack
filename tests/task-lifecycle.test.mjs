import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { spawnSync } from 'node:child_process';
import { run } from '../cli/main.mjs';
import { readSessionState, activeTask, bindingFile, attemptStatus } from '../application/session/activity.mjs';
import { readCheckpoint } from '../application/checkpoint.mjs';
import { prepareEvent } from '../application/context-delivery/prepare.mjs';

function fixture(t) {
  const root = fs.realpathSync(fs.mkdtempSync(path.join(os.tmpdir(), 'craft-task-lifecycle-')));
  t.after(() => fs.rmSync(root, { recursive: true, force: true }));
  const git = (...args) => {
    const result = spawnSync('git', ['-C', root, '-c', 'core.hooksPath=/dev/null', '-c', 'commit.gpgsign=false', '-c', 'user.name=Fixture', '-c', 'user.email=fixture@example.test', ...args], { encoding: 'utf8' });
    assert.equal(result.status, 0, result.stderr); return result.stdout.trim();
  };
  git('init', '-q');
  fs.writeFileSync(path.join(root, 'value.mjs'), 'export const SymbolValue = 1;\n');
  git('add', '.'); git('commit', '-qm', 'fixture');
  const command = (name, ...args) => run([name, '--root', root, '--session', 'host-session', ...args]);
  const start = (task = 'fix-value', goal = 'Fix the value') => command('task-start', '--task', task, '--goal', goal, '--file', 'value.mjs');
  const step = (extra = []) => command('task-step', '--stage', 'implement', '--target', 'acceptance:value', '--approach', 'bounded-fix', '--evidence', 'checks/value-1', '--outcome', 'passed', '--result', 'Value corrected', '--next', 'Verify the value', ...extra);
  return { root, git, command, start, step };
}

test('ordinary lifecycle binds discovery, records semantic acceptance, saves continuation and returns advice', t => {
  const { root, git, command, start, step } = fixture(t);
  const begun = start();
  assert.equal(begun.routing.intent, 'implement');
  assert.equal(activeTask(root, 'host-session'), 'fix-value');
  const first = command('locate', '--term', 'SymbolValue');
  assert.equal(first.discovery.fresh, 1);
  assert.equal(command('locate', '--term', 'SymbolValue').discovery.reused, 1);
  const accepted = step();
  assert.equal(accepted.recorded.outcome, 'passed');
  assert.equal(accepted.status.task.stage, 'implement');
  assert.equal(accepted.nextStep.action, 'design-verification');
  const saved = readCheckpoint(root, 'host-session');
  assert.equal(saved.taskId, 'fix-value');
  assert.equal(saved.continuation.currentState, 'Value corrected');
  assert.equal(saved.continuation.nextStep, 'Verify the value');
  assert.equal(accepted.status.usage.status, 'unknown');
  assert.equal(command('session').suppressed, true);
  assert.equal(readSessionState(root, 'host-session', 'fix-value').attempts.length, 1);
  assert.equal(fs.existsSync(path.join(root, '.craft')), false);
  assert.equal(git('status', '--porcelain'), '');
});

test('source edits invalidate stored acceptance and native tool success never creates a receipt', t => {
  const { root, command, start } = fixture(t);
  start();
  const verified = command('task-step', '--stage', 'verify', '--target', 'acceptance:value', '--approach', 'manual-check', '--evidence', 'checks/value', '--outcome', 'passed', '--result', 'Acceptance checked', '--next', 'Return the result', '--complete');
  assert.equal(verified.complete, true);
  assert.equal(verified.status.task.goalComplete, true);
  fs.appendFileSync(path.join(root, 'value.mjs'), '// new source\n');
  const status = command('session');
  assert.equal(status.taskEvidenceStale, true);
  assert.equal(status.task.goalComplete, false);
  assert.equal(status.task.verification, 'unknown');
  const prepared = prepareEvent({ cwd: root, event: 'after-tool', tool: 'shell', host: 'codex', session: 'host-session' });
  assert.equal(prepared.taskIdentity.task, 'fix-value');
  assert.equal(readSessionState(root, 'host-session', 'fix-value').attempts.length, 1);
});

test('task-step accounts separately installed guidance with verified bundle roots', t => {
  const { command, start } = fixture(t); start();
  const delivery = path.resolve('dist/codex/packages/craft-delivery');
  const engineering = path.resolve('dist/codex/packages/craft-engineering');
  const result = command('task-step', '--stage', 'verify', '--target', 'acceptance:value',
    '--approach', 'manual-check', '--evidence', 'checks/value', '--outcome', 'passed',
    '--result', 'Acceptance checked', '--next', 'Return evidence', '--complete',
    '--bundle', delivery, '--bundle', engineering);
  assert.equal(result.status.packReport.complete, true);
  assert.ok(result.status.packReport.totals.bytes > 0);
  assert.equal(command('session', '--bundle', delivery, '--bundle', engineering).packReport.complete, true);
});

test('task transitions isolate attempts and reuse, clear continuation, and preserve the previous task', t => {
  const { root, command, start, step } = fixture(t);
  start(); step(); command('locate', '--term', 'SymbolValue');
  start('next-task', 'Implement a different outcome');
  assert.equal(command('locate', '--term', 'SymbolValue').discovery.reused, 0);
  assert.equal(readSessionState(root, 'host-session', 'next-task').attempts.length, 0);
  assert.equal(readSessionState(root, 'host-session', 'fix-value').attempts.length, 1);
  assert.equal(readCheckpoint(root, 'host-session').taskId, 'next-task');
  assert.equal(readCheckpoint(root, 'host-session').continuation, undefined);
  assert.throws(() => start('fix-value', 'A different goal with the old id'), /different goal/);
  assert.equal(activeTask(root, 'host-session'), 'next-task');
});

test('unrelated acceptance success cannot erase a failure or complete the task', t => {
  const { root, command, start } = fixture(t); start();
  const receipt = (target, outcome, extra = []) => command('task-step', '--target', target, '--approach', 'bounded-check', '--evidence', `checks/${target}`, '--outcome', outcome, '--result', 'Observed check outcome', '--next', 'Resolve remaining acceptance', ...extra);
  receipt('acceptance:primary', 'quality-failure');
  receipt('acceptance:secondary', 'passed');
  let state = readSessionState(root, 'host-session', 'fix-value');
  assert.equal(attemptStatus(state.attempts, 'implement').attempts, 1);
  assert.throws(() => receipt('acceptance:secondary', 'passed', ['--complete']), /unresolved acceptance/);
  assert.equal(readSessionState(root, 'host-session', 'fix-value').attempts.length, 2);
  assert.equal(receipt('acceptance:primary', 'passed', ['--complete']).complete, true);
});

test('prerequisite resolution clears only matching blockers without certifying acceptance or clearing quality failures', async t => {
  const { root, command, start } = fixture(t); start();
  const receipt = (target, outcome, extra = []) => command('task-step', '--stage', 'verify', '--target', target,
    '--approach', 'bounded-check', '--evidence', 'checks/prerequisite', '--outcome', outcome,
    '--result', 'Observed prerequisite state', '--next', 'Continue acceptance', ...extra);
  receipt('acceptance:scope', 'quality-failure');
  receipt('acceptance:scope', 'tool-unavailable');
  receipt('acceptance:other', 'blocked');
  assert.throws(() => receipt('acceptance:missing', 'prerequisite-resolved'), /matching unresolved blocker/);
  assert.throws(() => receipt('acceptance:scope', 'prerequisite-resolved', ['--complete']), /passed acceptance/);
  const resolved = receipt('acceptance:scope', 'prerequisite-resolved', ['--profile', 'standard', '--host', 'codex']);
  assert.equal(resolved.complete, false);
  assert.equal(resolved.status.task.verification, undefined);
  const state = readSessionState(root, 'host-session', 'fix-value');
  const { unresolvedAttempts } = (await import('../engine/session/activity.mjs'));
  assert.deepEqual(unresolvedAttempts(state.attempts).map(item => [item.target, item.outcome]),
    [['acceptance:scope', 'quality-failure'], ['acceptance:other', 'blocked']]);
  assert.throws(() => receipt('acceptance:scope', 'prerequisite-resolved'), /matching unresolved blocker/);
  assert.throws(() => receipt('acceptance:scope', 'passed', ['--complete']), /unresolved/);
  receipt('acceptance:other', 'prerequisite-resolved');
  const accepted = receipt('acceptance:scope', 'passed', ['--complete']);
  assert.equal(accepted.complete, true);
});

test('missing or corrupt identity stays limited; disabled snapshots do not disable task receipts', t => {
  const { root, command, start, step } = fixture(t);
  assert.throws(() => step(), /task-start binding/);
  assert.throws(() => run(['task-start', '--root', root, '--task', 't', '--goal', 'Outcome']), /session/);
  assert.equal(command('locate', '--term', 'SymbolValue').reuse.enabled, false);
  const preferences = path.join(root, '.git', 'test-preferences.json');
  fs.writeFileSync(preferences, '{"checkpoint":{"enabled":false}}', { mode: 0o600 });
  const begun = command('task-start', '--task', 'fix-value', '--goal', 'Fix the value', '--preferences', preferences);
  assert.equal(begun.checkpoint.status, 'disabled');
  const accepted = step(['--preferences', preferences]);
  assert.equal(accepted.checkpoint.status, 'disabled');
  assert.equal(readCheckpoint(root, 'host-session'), null);
  fs.writeFileSync(bindingFile(root, 'host-session'), '{malformed');
  assert.equal(command('locate', '--term', 'SymbolValue').reuse.enabled, false);
  assert.throws(() => step(), /JSON|property|position/i);
  assert.equal(readSessionState(root, 'host-session', 'fix-value').attempts.length, 1);
});

test('stale explicit task assignments and busy session bindings cannot record a step', t => {
  const { root, command, start, step } = fixture(t);
  start(); start('another-task', 'Different task');
  assert.throws(() => step(['--task', 'fix-value']), /active task binding/);
  assert.equal(readSessionState(root, 'host-session', 'fix-value').attempts.length, 0);
  const lock = `${bindingFile(root, 'host-session')}.lock`;
  fs.writeFileSync(lock, 'another task writer');
  assert.throws(() => step(), /writer is busy/);
  assert.equal(readSessionState(root, 'host-session', 'another-task').attempts.length, 0);
  assert.equal(fs.readFileSync(lock, 'utf8'), 'another task writer');
});

test('standalone exported CLI retains a binding across independent processes', t => {
  const { root } = fixture(t);
  const cli = path.resolve('dist/codex/packages/craft-delivery/support/cli/main.mjs');
  const invoke = (action, ...args) => {
    const result = spawnSync(process.execPath, [cli, action, '--root', root, '--session', 'export-session', ...args, ...(args.includes('--json') ? [] : ['--json'])], { encoding: 'utf8' });
    assert.equal(result.status, 0, result.stderr); return JSON.parse(result.stdout);
  };
  invoke('task-start', '--task', 'export-task', '--goal', 'Correct the exported fixture', '--file', 'value.mjs');
  invoke('locate', '--term', 'SymbolValue');
  assert.equal(invoke('locate', '--term', 'SymbolValue').discovery.reused, 1);
  const accepted = invoke('task-step', '--target', 'acceptance:export', '--approach', 'bounded-check', '--evidence', 'checks/export', '--outcome', 'passed', '--result', 'Exported CLI accepted the step', '--next', 'Verify the fixture');
  assert.equal(accepted.task, 'export-task');
  assert.equal(accepted.checkpoint.state.continuation.currentState, 'Exported CLI accepted the step');
  const resumed = invoke('session', '--json');
  assert.equal(resumed.task.id, 'export-task');
  assert.equal(resumed.checkpoint.status, 'fresh');
});

test('a source change after acceptance cannot freshen a semantic continuation', t => {
  const { root, start, step } = fixture(t); start();
  const rename = fs.renameSync;
  let changed = false;
  t.mock.method(fs, 'renameSync', (from, to) => {
    const result = rename(from, to);
    if (!changed && path.basename(to).startsWith('session-')) {
      changed = true;
      fs.appendFileSync(path.join(root, 'value.mjs'), '// concurrent source edit\n');
    }
    return result;
  });
  const accepted = step();
  assert.equal(accepted.recorded.outcome, 'passed');
  assert.equal(accepted.checkpoint.status, 'unavailable');
  assert.match(accepted.checkpoint.reason, /Source changed/);
  assert.equal(accepted.status.taskEvidenceStale, true);
  assert.equal(readCheckpoint(root, 'host-session').continuation, undefined);
  assert.equal(readSessionState(root, 'host-session', 'fix-value').attempts.length, 1);
});

test('explicit task data and notice inputs take precedence over the session binding', t => {
  const { root, command, start, step } = fixture(t);
  start(); step();
  const data = path.join(root, '.git', 'explicit-task.json');
  fs.writeFileSync(data, JSON.stringify({ previous: { key: 'caller-notice', at: new Date().toISOString() } }));
  assert.equal(command('session', '--data', data).suppressed, false);
  start('another-task', 'Different task');
  fs.writeFileSync(data, JSON.stringify({ task: { id: 'fix-value' } }));
  assert.equal(command('session', '--data', data).task.id, 'fix-value');
  assert.equal(activeTask(root, 'host-session'), 'another-task');
  fs.writeFileSync(data, JSON.stringify({ taskId: 'fix-value' }));
  assert.equal(command('checkpoint-save', '--data', data).state.taskId, 'fix-value');
  assert.equal(activeTask(root, 'host-session'), 'another-task');
});

test('scope and risk changes invalidate completed acceptance and continuation without source edits', t => {
  const { root, command, start } = fixture(t); start();
  command('task-step', '--stage', 'verify', '--target', 'acceptance:value', '--approach', 'manual-check', '--evidence', 'checks/value', '--outcome', 'passed', '--result', 'Acceptance checked', '--next', 'Return the result', '--complete');
  assert.equal(command('session').checkpoint.status, 'fresh');
  command('task-start', '--task', 'fix-value', '--goal', 'Fix the value', '--stage', 'verify', '--file', 'value.mjs', '--file', 'another.mjs', '--risk', 'security');
  const status = command('session');
  assert.equal(status.task.goalComplete, false);
  assert.notEqual(status.task.verification, 'passed');
  assert.equal(status.taskEvidenceStale, true);
  assert.equal(status.checkpoint.status, 'stale-or-incomplete');
  assert.equal(readCheckpoint(root, 'host-session').continuation.currentState, 'Acceptance checked');
});

test('same-SHA branch and detached transitions require rebinding and new acceptance', t => {
  const { root, git, command, start } = fixture(t); start();
  const complete = () => command('task-step', '--stage', 'verify', '--target', 'acceptance:value', '--approach', 'manual-check', '--evidence', 'checks/value', '--outcome', 'passed', '--result', 'Acceptance checked', '--next', 'Return the result', '--complete');
  complete();
  const original = git('rev-parse', 'HEAD');
  for (const transition of [['switch', '-c', 'consumer-trunk'], ['switch', '--detach', original]]) {
    git(...transition);
    assert.equal(git('rev-parse', 'HEAD'), original);
    assert.equal(activeTask(root, 'host-session'), undefined);
    assert.throws(() => complete(), /task-start binding/);
    const stale = command('session', '--task', 'fix-value');
    assert.equal(stale.task.goalComplete, false);
    assert.equal(stale.checkpoint.status, 'stale-or-incomplete');
    command('task-start', '--task', 'fix-value', '--goal', 'Fix the value', '--stage', 'verify');
    assert.equal(activeTask(root, 'host-session'), 'fix-value');
    assert.equal(command('session').task.goalComplete, false);
    assert.equal(complete().status.checkpoint.status, 'fresh');
  }
});


test('risk-only changes invalidate verification and continuation for unchanged files', t => {
  const { command, start } = fixture(t); start();
  command('task-step', '--stage', 'verify', '--target', 'acceptance:value', '--approach', 'manual-check', '--evidence', 'checks/value', '--outcome', 'passed', '--result', 'Acceptance checked', '--next', 'Return the result', '--complete');
  command('task-start', '--task', 'fix-value', '--goal', 'Fix the value', '--stage', 'verify', '--file', 'value.mjs', '--risk', 'security');
  const status = command('session');
  assert.equal(status.task.goalComplete, false);
  assert.notEqual(status.task.verification, 'passed');
  assert.equal(status.checkpoint.status, 'stale-or-incomplete');
});
