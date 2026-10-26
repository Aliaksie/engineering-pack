import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { spawnSync } from 'node:child_process';
import { run } from '../cli/main.mjs';
import { decide } from '../application/engine-client.mjs';

test('feature evidence is bounded, distinguishes declaration/runtime receipts, and never fills missing applicability', () => {
  const state = { taskRecord: { id: 'task' }, activity: { routingDecisions: 1, operationsAttempted: 2 } };
  const record = decide('feature-record', { state, source: 'a'.repeat(64), at: '2026-10-20T00:00:00.000Z',
    input: { feature: 'compaction', status: 'unsupported', reason: 'No verified host control', evidence: 'host:capabilities' } });
  const report = decide('feature-report', { state: record.state, repository: {} });
  assert.equal(report.features.compaction.provenance, 'agent-declared');
  assert.equal(report.features.compaction.sourceStatus, 'unknown');
  assert.equal(report.features.routing.provenance, 'runtime-receipt');
  assert.equal(report.features.routing.sourceStatus, 'historical');
  assert.equal(report.features.review.status, 'unknown');
  assert.equal(report.allClassified, false);
  assert.equal(Object.hasOwn(state, 'featureOutcomes'), false);
  for (const input of [{ feature: 'unknown', status: 'used' }, { feature: 'review', status: 'passed' },
    { feature: 'review', status: 'used', reason: 'x'.repeat(513), evidence: 'check:one' }]) {
    assert.throws(() => decide('feature-record', { state, input, source: null, at: '2026-10-20T00:00:00.000Z' }));
  }
});

test('an agent declaration never replaces a runtime-observed outcome', () => {
  const at = '2026-10-20T00:00:00.000Z', source = 'a'.repeat(64);
  const input = { feature: 'review', status: 'disabled', reason: 'Declared afterwards', evidence: 'agent:claim' };
  const observed = decide('feature-record', { state: { taskRecord: { id: 'task' } }, source, at, provenance: 'runtime-observed',
    input: { feature: 'review', status: 'used', reason: 'Host adapter reported the scoped action', evidence: 'review:receipt' } }).state;
  assert.throws(() => decide('feature-record', { state: observed, input, source, at, provenance: 'agent-declared' }), /runtime-observed/);
  assert.equal(observed.featureOutcomes.review.status, 'used');
  const declared = decide('feature-record', { state: { taskRecord: { id: 'task' } }, input, source, at, provenance: 'agent-declared' }).state;
  const replaced = decide('feature-record', { state: declared, source, at, provenance: 'runtime-observed',
    input: { feature: 'review', status: 'used', reason: 'Host adapter reported the scoped action', evidence: 'review:receipt' } }).state;
  assert.equal(replaced.featureOutcomes.review.provenance, 'runtime-observed');
});

test('normal task commands retain outcomes and mark changed sources stale without changing the recorded verdict', t => {
  const root = fs.realpathSync(fs.mkdtempSync(path.join(os.tmpdir(), 'craft-feature-outcomes-')));
  t.after(() => fs.rmSync(root, { recursive: true, force: true }));
  const git = (...args) => {
    const result = spawnSync('git', ['-C', root, '-c', 'core.hooksPath=/dev/null', '-c', 'commit.gpgsign=false', '-c', 'user.name=Fixture', '-c', 'user.email=fixture@example.test', ...args], { encoding: 'utf8' });
    assert.equal(result.status, 0, result.stderr);
  };
  git('init', '-q'); fs.writeFileSync(path.join(root, 'file.txt'), 'one'); git('add', '.'); git('commit', '-qm', 'fixture');
  const command = (action, ...args) => run([action, '--root', root, '--session', 's1', ...args]);
  command('task-start', '--task', 'T1', '--goal', 'Inspect the requested scope');
  const result = command('task-feature', '--feature', 'review', '--feature-status', 'used', '--reason', 'Reviewed the actual source changes', '--evidence', 'review:current');
  assert.equal(result.features.review.sourceStatus, 'current');
  fs.appendFileSync(path.join(root, 'file.txt'), 'two');
  const changed = command('task-features');
  assert.equal(changed.features.review.status, 'used');
  assert.equal(changed.features.review.sourceStatus, 'stale');
  const status = command('session');
  assert.equal(status.featureOutcomes.features.review.sourceStatus, 'stale');
  command('task-start', '--task', 'T2', '--goal', 'A separate outcome');
  assert.equal(command('task-features').features.review.status, 'unknown');
});

test('relocated standalone runtime persists feature outcomes across processes', t => {
  const root = fs.realpathSync(fs.mkdtempSync(path.join(os.tmpdir(), 'craft-controls-export-')));
  t.after(() => fs.rmSync(root, { recursive: true, force: true }));
  const support = path.join(root, 'support');
  fs.cpSync(path.resolve('dist/codex/packages/craft-delivery/support'), support, { recursive: true });
  const options = { session: 'standalone-session', task: 'standalone-task' };
  assert.equal(fs.existsSync(path.join(support, 'hosts')), false);
  const invoke = (...args) => {
    const result = spawnSync(process.execPath, [path.join(support, 'cli/main.mjs'), ...args, '--root', root, '--session', options.session, '--json'], { encoding: 'utf8' });
    assert.equal(result.status, 0, result.stderr); return JSON.parse(result.stdout);
  };
  invoke('task-start', '--task', options.task, '--goal', 'Inspect portable controls');
  invoke('task-feature', '--feature', 'delegation', '--feature-status', 'unsupported', '--reason', 'No native adapter', '--evidence', 'host:capabilities');
  assert.equal(invoke('task-features').features.delegation.provenance, 'agent-declared');
  assert.deepEqual(invoke('worker-status').requests, []);
});
