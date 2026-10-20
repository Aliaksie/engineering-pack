import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { spawnSync } from 'node:child_process';
import { run } from '../cli/main.mjs';
import { decide } from '../application/engine-client.mjs';
import { validateProject } from '../application/project/configuration.mjs';
import { modelAdvice } from '../application/session/models.mjs';

function fixture(t, config = {}) {
  const root = fs.realpathSync(fs.mkdtempSync(path.join(os.tmpdir(), 'craft-action-controls-')));
  t.after(() => fs.rmSync(root, { recursive: true, force: true }));
  const git = (...args) => {
    const result = spawnSync('git', ['-C', root, '-c', 'core.hooksPath=/dev/null', '-c', 'commit.gpgsign=false', '-c', 'user.name=Fixture', '-c', 'user.email=fixture@example.test', ...args], { encoding: 'utf8' });
    assert.equal(result.status, 0, result.stderr);
  };
  git('init', '-q'); fs.writeFileSync(path.join(root, 'value.mjs'), 'export const value = 1;\n');
  if (Object.keys(config).length) {
    fs.mkdirSync(path.join(root, '.capabilities'));
    fs.writeFileSync(path.join(root, '.capabilities/project.json'), JSON.stringify({ formatVersion: 1, ...config }));
  }
  git('add', '.'); git('commit', '-qm', 'fixture');
  const command = (name, ...args) => run([name, '--root', root, '--session', 's1', ...args]);
  const facts = value => { const file = path.join(root, '.git', 'facts.json'); fs.writeFileSync(file, JSON.stringify(value), { mode: 0o600 }); return file; };
  const start = (...extra) => command('task-start', '--task', 'T1', '--goal', 'Deliver the scoped change', '--file', 'value.mjs', ...extra);
  const verify = () => command('task-step', '--stage', 'verify', '--target', 'acceptance:scope', '--approach', 'bounded-check', '--evidence', 'checks/current', '--outcome', 'passed', '--result', 'Current scope verified', '--next', 'Return evidence');
  return { root, command, facts, start, verify };
}
const capacity = (extra = {}) => ({ capacity: 4, occupiedSlots: 1, taskWorkers: 0, allowed: true, sessionId: 's1', observedAt: new Date().toISOString(), source: 'fixture-host-slots', ...extra });

test('task-wide worker budgets honor zero, capacity, occupied idle/nested slots and reductions', () => {
  const project = validateProject({ formatVersion: 1 });
  const budget = (facts, policy = project) => decide('worker-budget', { project: policy, facts, options: { session: 's1', now: Date.now() } });
  assert.equal(budget(capacity()).available, 3);
  assert.equal(budget(capacity({ occupiedSlots: 4, taskWorkers: 3 })).available, 0);
  assert.equal(budget(capacity({ occupiedSlots: 3, taskWorkers: 1 })).effectiveLimit, 2);
  const one = validateProject({ formatVersion: 1, efficiency: { delegation: { maxWorkers: 1 } } });
  assert.equal(budget(capacity({ occupiedSlots: 2, taskWorkers: 1 }), one).available, 0);
  const lowered = budget(capacity({ occupiedSlots: 4, taskWorkers: 3 }), one);
  assert.equal(lowered.reason, 'limit-reduced-no-new-assignments');
  assert.equal(lowered.cancelActiveWorkers, false);
  const zero = validateProject({ formatVersion: 1, efficiency: { delegation: { maxWorkers: 0 } } });
  assert.equal(budget({}, zero).effectiveLimit, 0);
  assert.equal(budget(capacity({ allowed: false })).effectiveLimit, 0);
  assert.equal(budget(capacity({ observedAt: '2020-01-01T00:00:00Z' })).effectiveLimit, null);
  assert.equal(budget(capacity({ sessionId: 'another' })).available, 0);
  assert.throws(() => budget(capacity({ taskWorkers: 3, occupiedSlots: 2 })), /slot counts/);
});

test('logical defaults give useful main-model advice without workers or exact model aliases', () => {
  const project = validateProject({ formatVersion: 1, efficiency: { delegation: { maxWorkers: 0 } } });
  const advice = modelAdvice({ host: 'unknown', role: 'implementer', task: 'T1', revision: 'r1', capabilityClass: 'strong' }, project);
  assert.equal(advice.profile, 'strong');
  assert.equal(advice.execution.mode, 'main');
  assert.equal(advice.mainModel.model, null);
  assert.equal(advice.mainModel.switchSupported, false);
  assert.equal(advice.attemptsRemaining, 3);
  const adequate = modelAdvice({ host: 'unknown', role: 'implementer', task: 'T1', revision: 'r1', currentProfile: 'strong' }, project);
  assert.equal(adequate.mainModel.decision, 'keep-current');
  const mapped = validateProject({ formatVersion: 1, execution: { hosts: { fixture: { mainModelSwitch: true, models: { standard: 'explicit-consumer-model' } } } } });
  const suggestion = modelAdvice({ host: 'fixture', role: 'implementer', task: 'T1', revision: 'r1' }, mapped);
  assert.equal(suggestion.mainModel.switchSupported, true);
  assert.equal(suggestion.mainModel.model, 'explicit-consumer-model');
  assert.equal(suggestion.execution.mode, 'main');
});

test('risky verified steps create a pending review offer; decline suppresses repeats and approval does not execute', t => {
  const { command, facts, start, verify } = fixture(t);
  start('--risk', 'security');
  const checked = verify(), offer = checked.actionOffer.offer;
  assert.equal(offer.decision, 'pending');
  assert.equal(checked.actionOffer.executed, false);
  const data = facts({ allowed: true, supported: true });
  assert.equal(command('task-action-check', '--offer', offer.id, '--data', data).ready, false);
  command('task-decision', '--offer', offer.id, '--decision', 'declined', '--evidence', 'user:decline-1');
  assert.equal(verify().actionOffer.suppressed, true);
  assert.equal(verify().nextStep, null);
  assert.equal(command('task-action-check', '--offer', offer.id, '--data', data).ready, false);
  assert.throws(() => command('task-decision', '--offer', offer.id, '--decision', 'completed', '--evidence', 'user:1'), /approval\/decline/);
  command('task-decision', '--offer', offer.id, '--decision', 'approved', '--evidence', 'user:approve-2');
  const ready = command('task-action-check', '--offer', offer.id, '--data', data);
  assert.equal(ready.ready, true); assert.equal(ready.executed, false);
  assert.equal(ready.offer.execution, 'not-observed');
});

test('existing scoped review authorization avoids another approval and stale source/policy rejects decisions', t => {
  const { root, command, start, verify, facts } = fixture(t);
  start('--risk', 'security', '--review-authorization', 'user:review-current-scope');
  const offer = verify().actionOffer.offer;
  assert.equal(offer.decision, 'already-authorized');
  const data = facts({ allowed: true, supported: true });
  assert.equal(command('task-action-check', '--offer', offer.id, '--data', data).ready, true);
  const preferences = path.join(root, '.git', 'preferences.json');
  fs.writeFileSync(preferences, '{"efficiency":{"delegation":{"maxWorkers":0}}}', { mode: 0o600 });
  assert.throws(() => command('task-action-check', '--offer', offer.id, '--data', data, '--preferences', preferences), /stale/);
  fs.appendFileSync(path.join(root, 'value.mjs'), '// changed\n');
  assert.throws(() => command('task-decision', '--offer', offer.id, '--decision', 'approved', '--evidence', 'user:2'), /stale/);
});

test('delegation offers require real declared mappings, independent work and fresh available capacity', t => {
  const config = { execution: { roles: { researcher: { mode: 'delegate', profile: 'standard' } }, hosts: { fixture: { delegation: true, models: { standard: 'explicit-worker-model' } } } } };
  const { command, facts, start } = fixture(t, config); start();
  const args = ['--action', 'delegate', '--scope', 'extract-independent-sources', '--role', 'researcher', '--benefit', 'Compare independent sources', '--evidence', 'scope:independent'];
  const observations = { host: { name: 'fixture' } };
  let data = facts({ observations, independentTasks: 3, allowed: true, supported: true });
  assert.equal(command('task-offer', ...args, '--data', data).offer, null);
  data = facts({ observations, independentTasks: 3, workers: capacity(), allowed: true, supported: true });
  const offered = command('task-offer', ...args, '--data', data);
  assert.equal(offered.offer.profile, 'standard');
  assert.equal(offered.offer.workerBudget.available, 3);
  command('task-decision', '--offer', offered.offer.id, '--decision', 'approved', '--evidence', 'user:worker-scope');
  assert.equal(command('task-action-check', '--offer', offered.offer.id, '--data', data).ready, true);
  data = facts({ observations, independentTasks: 3, workers: capacity({ occupiedSlots: 4, taskWorkers: 3 }), allowed: true, supported: true });
  assert.equal(command('task-action-check', '--offer', offered.offer.id, '--data', data).ready, false);
});

test('compaction needs fresh matching pressure, safe boundary, supported control and fresh continuation', t => {
  const { command, facts, start, verify } = fixture(t); start(); verify();
  const observed = { host: { name: 'fixture', reliableCompaction: true }, task: { safeBoundary: true },
    usage: { kind: 'current-context', currentTokens: 80, capacityTokens: 100, source: 'fixture-host', sessionId: 's1', observedAt: new Date().toISOString() } };
  const args = ['--action', 'compact', '--scope', 'current-task', '--benefit', 'Continue with a fresh context', '--evidence', 'host:usage'];
  let data = facts({ observations: observed, allowed: true, supported: true });
  const offered = command('task-offer', ...args, '--data', data);
  assert.equal(offered.offer.decision, 'pending');
  command('task-decision', '--offer', offered.offer.id, '--decision', 'approved', '--evidence', 'user:compact');
  assert.equal(command('task-action-check', '--offer', offered.offer.id, '--data', data).ready, true);
  data = facts({ observations: { ...observed, task: { safeBoundary: true, almostComplete: true } }, allowed: true, supported: true });
  assert.equal(command('task-action-check', '--offer', offered.offer.id, '--data', data).ready, false);
  data = facts({ observations: { ...observed, usage: { ...observed.usage, kind: 'cumulative' } } });
  assert.equal(command('task-offer', ...args, '--data', data).offer, null);
});

test('zero worker limits suppress configured delegation and manual controls suppress optional offers', t => {
  const { command, facts, start, verify } = fixture(t, {
    efficiency: { delegation: { maxWorkers: 0 }, review: { mode: 'manual' }, context: { action: 'manual' } },
    execution: { roles: { researcher: { mode: 'delegate', profile: 'standard' } }, hosts: { fixture: { delegation: true, models: { standard: 'explicit-worker-model' } } } }
  });
  start('--risk', 'security');
  assert.equal(command('route', '--intent', 'research', '--host', 'fixture', '--delegate').execution.mode, 'main');
  assert.equal(verify().actionOffer, undefined);
  const data = facts({ observations: { host: { name: 'fixture' } }, independentTasks: 3, workers: capacity(), supported: true, allowed: true });
  const offered = command('task-offer', '--action', 'delegate', '--scope', 'independent-scope', '--role', 'researcher', '--benefit', 'Parallel extraction', '--evidence', 'scope:1', '--data', data);
  assert.equal(offered.offer, null);
  assert.equal(offered.applicability.budget.effectiveLimit, 0);
  assert.throws(() => command('settings', '--data', facts({ permissions: { writeOperations: ['git.push'] } })), /unknown/);
});

test('a worker proposal cannot bypass a recorded permission or tool prerequisite', t => {
  const { command, start, facts } = fixture(t, { execution: { roles: { researcher: { mode: 'delegate', profile: 'standard' } }, hosts: { fixture: { delegation: true, models: { standard: 'explicit-worker-model' } } } } });
  start();
  command('task-step', '--target', 'acceptance:blocked', '--approach', 'bounded-check', '--evidence', 'checks/denied', '--outcome', 'permission-denied', '--result', 'Required access is denied', '--next', 'Resolve access');
  const data = facts({ observations: { host: { name: 'fixture' } }, independentTasks: 3, workers: capacity(), allowed: true, supported: true });
  const offered = command('task-offer', '--action', 'delegate', '--scope', 'blocked-scope', '--role', 'researcher', '--benefit', 'Try another worker', '--evidence', 'checks/denied', '--data', data);
  assert.equal(offered.offer, null);
  assert.match(offered.applicability.reason, /prerequisite/);
});

test('declining compaction suppresses the repeated session offer without inventing approval', t => {
  const { command, facts, start, verify } = fixture(t); start(); verify();
  const observations = { host: { name: 'fixture', reliableCompaction: true }, task: { safeBoundary: true },
    usage: { kind: 'current-context', currentTokens: 80, capacityTokens: 100, source: 'fixture-host', sessionId: 's1', observedAt: new Date().toISOString() } };
  const data = facts({ observations, allowed: true, supported: true });
  const offer = command('task-offer', '--action', 'compact', '--scope', 'current-task', '--benefit', 'Preserve context', '--evidence', 'host:usage', '--data', data).offer;
  command('task-decision', '--offer', offer.id, '--decision', 'declined', '--evidence', 'user:decline-compaction');
  const status = command('session', '--data', facts(observations));
  assert.equal(status.actionOffer.decision, 'declined');
  assert.equal(status.recommendation.approvalRequired, true);
  assert.equal(status.suggestion, null);
  assert.equal(status.recommendation.execution, 'not-observed');
});
