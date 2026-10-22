import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { spawnSync } from 'node:child_process';
import { run } from '../cli/main.mjs';
import { readSessionState, activeTask, bindingFile, attemptStatus } from '../application/session/activity.mjs';
import { engineeringContractDigest } from '../engine/session/acceptance.mjs';
import { renderCommand } from '../cli/render.mjs';
import { lockState, releaseState } from '../platform/index.mjs';
import { updateSessionState } from '../application/session/activity.mjs';
import { readCheckpoint, recoverCheckpoint } from '../application/checkpoint.mjs';
import { prepareEvent } from '../application/context-delivery/prepare.mjs';
import { readTelemetry } from '../platform/node/telemetry.mjs';

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
  assert.equal(fs.existsSync(path.join(root, '.craft', 'state')), true);
  assert.equal(fs.existsSync(path.join(root, '.craft', 'project.json')), false);
  assert.equal(git('status', '--porcelain'), '?? .craft/');
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

function structuredFixture(t) {
  const f = fixture(t);
  const contract = JSON.parse(fs.readFileSync(new URL('./fixtures/acceptance/contract.json', import.meta.url), 'utf8').replaceAll('"src"', '"value.mjs"'));
  const contractFile = path.join(f.root, '.git', 'acceptance-contract.json');
  const evidenceFile = path.join(f.root, '.git', 'acceptance-evidence.json');
  fs.writeFileSync(contractFile, JSON.stringify(contract));
  const start = () => f.command('task-start', '--task', 'fix-value', '--goal', 'Fix the value', '--file', 'value.mjs', '--contract', contractFile);
  const submit = (records, extra = [], expected = f.command('session')) => {
    fs.writeFileSync(evidenceFile, JSON.stringify({ formatVersion: 1, records }));
    return f.command('task-step', '--stage', 'verify', '--target', 'acceptance:value', '--approach', 'criterion-checks',
      '--evidence', 'retained/checks', '--outcome', 'passed', '--result', 'Criteria assessed', '--next', 'Report remaining gaps',
      '--acceptance-evidence', evidenceFile, '--expected-task-contract', expected.taskContract,
      '--expected-evidence-digest', expected.evidenceDigest, ...extra);
  };
  const records = (binding = f.command('session').binding) => contract.criteria.map(criterion => ({
    id: `check-${criterion.id}`, criterionIds: [criterion.id], scope: criterion.scope,
    contract: engineeringContractDigest(contract), binding, checkId: 'value-check', kind: 'check',
    artifact: 'retained/checks', observedAt: '2026-10-22T00:00:00Z', reporter: 'fixture assessor',
    basis: criterion.description, support: 'verified', execution: 'passed'
  }));
  return { ...f, contract, contractFile, start, submit, records };
}

test('structured CLI completion survives restart, attempt eviction and checkpoint recovery', t => {
  const { root, command, start, submit, records } = structuredFixture(t);
  const begun = start();
  assert.equal(begun.acceptance.complete, false);
  assert.equal(begun.acceptance.assurance, 'attributed');
  assert.throws(() => submit(records().slice(0, 1), ['--complete']), /every required criterion/);
  assert.equal(readSessionState(root, 'host-session', 'fix-value').attempts.length, 0);
  assert.equal(submit(records(), ['--complete']).complete, true);
  for (let i = 0; i < 26; i++) command('session-attempt', '--data', writeAttempt(root, i));
  const saved = readSessionState(root, 'host-session', 'fix-value');
  assert.equal(saved.formatVersion, 2);
  assert.equal(saved.attempts.length, 24);
  assert.equal(saved.taskRecord.acceptanceEvidence.records.length, 2);
  const restarted = spawnSync(process.execPath, ['cli/main.mjs', 'session', '--root', root, '--session', 'host-session', '--json'], { encoding: 'utf8' });
  assert.equal(restarted.status, 0, restarted.stderr);
  const status = JSON.parse(restarted.stdout);
  assert.equal(status.acceptance.complete, true);
  assert.equal(status.task.goalComplete, true);
  assert.equal(status.checkpoint.status, 'fresh');
  assert.equal(readCheckpoint(root, 'host-session').continuationSource.taskContract, status.taskContract);
});

test('compaction recovery reloads current criterion gaps without renewing evidence', t => {
  const { root, start, submit, records, command, contract, contractFile } = structuredFixture(t);
  start();
  submit(records().slice(0, 1));
  const recover = () => prepareEvent({ cwd: root, session: 'host-session', host: 'claude', event: 'after-compact' }).checkpoint.taskRecovery;
  const pending = recover();
  assert.equal(pending.status, 'available');
  assert.equal(pending.checkpointTaskMatches, true);
  assert.equal(pending.acceptance.complete, false);
  assert.equal(pending.acceptance.criteria[1].status, 'gap');
  assert.equal(pending.taskContract, command('session').taskContract);
  submit(records(), ['--complete']);
  assert.equal(recover().task.goalComplete, true);
  fs.appendFileSync(path.join(root, 'value.mjs'), '// changed after checks\n');
  const stale = recover();
  assert.equal(stale.task.goalComplete, false);
  assert.equal(stale.acceptance.complete, false);
  assert.equal(stale.acceptance.records[0].freshness, 'stale');
  contract.criteria[0].description = 'Revised criterion';
  fs.writeFileSync(contractFile, JSON.stringify(contract));
  start();
  assert.equal(recover().acceptance.records.length, 0);
});

test('checkpoint recovery keeps switched and unreadable tasks separate from historical acceptance', t => {
  const { root, start, submit, records, command } = structuredFixture(t);
  start(); submit(records(), ['--complete']);
  const preferences = path.join(root, '.git', 'no-checkpoint.json');
  fs.writeFileSync(preferences, '{"checkpoint":{"enabled":false}}');
  command('task-start', '--task', 'another-task', '--goal', 'Another task', '--preferences', preferences);
  const switched = recoverCheckpoint(root, 'host-session');
  assert.equal(switched.saved.taskId, 'fix-value');
  assert.equal(switched.taskRecovery.task.id, 'another-task');
  assert.equal(switched.taskRecovery.checkpointTaskMatches, false);
  fs.writeFileSync(bindingFile(root, 'host-session'), '{broken');
  const unavailable = recoverCheckpoint(root, 'host-session');
  assert.deepEqual(unavailable.taskRecovery, { status: 'unavailable' });
  assert.equal(unavailable.saved.taskId, 'fix-value');
});

test('criterion recovery stays functional without telemetry and keeps prose out of events', t => {
  const { root, contract, contractFile, start, submit, records } = structuredFixture(t);
  const privateText = 'PRIVATE-criterion-source-material-7321';
  contract.criteria[0].description = privateText;
  fs.writeFileSync(contractFile, JSON.stringify(contract));
  start(); submit(records().slice(0, 1));
  const recovery = recoverCheckpoint(root, 'host-session');
  assert.equal(recovery.taskRecovery.acceptance.complete, false);
  const before = readTelemetry(root);
  assert.ok(before.some(event => event.name === 'checkpoint.recovered'));
  assert.equal(JSON.stringify(before).includes(privateText), false);
  fs.writeFileSync(path.join(root, '.craft', 'telemetry.json'), '{"local":false}');
  // The new settings file changes the source snapshot; verify again rather than
  // expecting evidence from before that change to remain current.
  assert.equal(submit(records(), ['--complete']).complete, true);
  assert.equal(recoverCheckpoint(root, 'host-session').taskRecovery.acceptance.complete, true);
  assert.deepEqual(readTelemetry(root), before);
});

function writeAttempt(root, i) {
  const file = path.join(root, '.git', 'attempt.json');
  fs.writeFileSync(file, JSON.stringify({ operation: 'verify', target: `closed-${i}`, approach: 'fixture', evidence: 'retained/checks', outcome: 'passed' }));
  return file;
}

test('structured evidence reports missing, failed, unavailable, irrelevant and optional coverage', t => {
  const { root, contract, contractFile, command, start, submit, records } = structuredFixture(t);
  start();
  for (const change of [r => { r[1].execution = 'failed'; }, r => { r[1].execution = 'not-run'; },
    r => { r[1].execution = 'not-available'; }, r => { r[1].checkId = 'unrelated'; }, r => { r[1].binding.identity = null; }]) {
    const evidence = records(); change(evidence);
    const observed = submit(evidence);
    assert.equal(observed.acceptance.complete, false);
    assert.throws(() => submit(evidence, ['--complete']), /every required criterion/);
  }
  contract.criteria[1].required = false;
  fs.writeFileSync(contractFile, JSON.stringify(contract)); start();
  const complete = submit(records().slice(0, 1), ['--complete']);
  assert.equal(complete.complete, true);
  assert.equal(complete.acceptance.criteria[1].status, 'gap');
  const compact = renderCommand('task-step', complete);
  assert.match(compact, /error — gap \(optional\)/);
  assert.match(compact, /attributed criterion evidence/);
  assert.match(renderCommand('session', command('session')), /value — verified/);
  assert.equal(readSessionState(root, 'host-session', 'fix-value').taskRecord.acceptanceEvidence.records.length, 1);
});

test('source changes, contract edits and delayed ledger writers cannot certify current completion', t => {
  const { root, contract, contractFile, command, start, submit, records } = structuredFixture(t);
  start();
  const old = records();
  submit(old);
  const delayed = command('session');
  const newer = records(); newer[0].basis = 'Updated assessment basis';
  submit(newer);
  assert.throws(() => submit(old, [], delayed), /ledger changed/);
  fs.appendFileSync(path.join(root, 'value.mjs'), '// changed after checks\n');
  assert.equal(command('session').acceptance.records[0].freshness, 'stale');
  assert.throws(() => submit(old, ['--complete']), /every required criterion/);
  submit(records(), ['--complete']);
  contract.criteria[0].description = 'Amended acceptance without source changes';
  fs.writeFileSync(contractFile, JSON.stringify(contract));
  const before = command('session'); start();
  assert.equal(command('session').task.goalComplete, false);
  assert.equal(command('session').acceptance.records.length, 0);
  assert.throws(() => submit(old, [], before), /task contract changed/);
  const scopeBefore = command('session');
  command('task-start', '--task', 'fix-value', '--goal', 'Fix the value', '--risk', 'new risk');
  assert.throws(() => submit(records(), [], scopeBefore), /task contract changed/);
});

test('structured state refuses busy writers and combined capacity overflow without losing prior evidence', t => {
  const { root, command, start, submit, records } = structuredFixture(t);
  start(); submit(records());
  const before = readSessionState(root, 'host-session', 'fix-value');
  const lock = lockState(bindingFile(root, 'host-session'));
  assert.ok(lock);
  try { assert.throws(() => submit(records()), /writer is busy/); }
  finally { releaseState(lock); }
  assert.deepEqual(readSessionState(root, 'host-session', 'fix-value'), before);
  updateSessionState(root, 'host-session', 'fix-value', state => { state.padding = 'x'.repeat(121000); });
  const padded = readSessionState(root, 'host-session', 'fix-value');
  const large = records();
  for (let i = 0; i < 10; i++) large.push({ ...large[0], id: `extra-${i}`, basis: 'b'.repeat(512), artifact: 'a'.repeat(1024) });
  assert.throws(() => submit(large), /limit|large|capacity|bound|size/);
  assert.deepEqual(readSessionState(root, 'host-session', 'fix-value'), padded);
  assert.equal(command('session').acceptance.complete, true);
});
