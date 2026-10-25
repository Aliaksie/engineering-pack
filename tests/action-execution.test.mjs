import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import { stepTask, taskAction } from '../application/session/task.mjs';
import { run } from '../cli/main.mjs';
import { actionController } from '../application/session/actions.mjs';
import { decide } from '../application/engine-client.mjs';
import { readSessionState } from '../application/session/activity.mjs';

// task-start runs in the native runtime; tests bind tasks through the CLI.
const startTask = (root, session, task, { goal, risks = [], contract }, { host } = {}) => run(['task-start', '--root', root, '--session', session, '--task', task, '--goal', goal,
  ...risks.flatMap(risk => ['--risk', risk]), ...(contract ? ['--contract', contract] : []), ...(host ? ['--host', host] : [])]);

function fixture(t) {
  const root = fs.realpathSync(fs.mkdtempSync(path.join(os.tmpdir(), 'craft-native-action-')));
  t.after(() => fs.rmSync(root, { recursive: true, force: true }));
  const git = (...args) => {
    const result = spawnSync('git', ['-C', root, '-c', 'core.hooksPath=/dev/null', '-c', 'commit.gpgsign=false', '-c', 'user.name=Fixture', '-c', 'user.email=fixture@example.test', ...args], { encoding: 'utf8' });
    assert.equal(result.status, 0, result.stderr);
  };
  git('init', '-q'); fs.writeFileSync(path.join(root, 'file.txt'), 'source'); git('add', '.'); git('commit', '-qm', 'fixture');
  startTask(root, 's1', 'T1', { goal: 'Verify the scoped change', risks: ['security'] });
  const verified = stepTask(root, 's1', 'T1', { stage: 'verify', target: 'acceptance:scope', approach: 'bounded-check', evidence: 'checks:current', outcome: 'passed', result: 'Acceptance checked', nextStep: 'Review current source' });
  const id = verified.actionOffer.offer.id;
  const decideOffer = decision => taskAction(root, 's1', 'T1', 'action-decide', { id, decision, evidence: 'user:scoped-decision' });
  const facts = { allowed: true, supported: true }, calls = [];
  const host = { authorize: async () => ({ allowed: true }),
    observe: async () => ({ sessionId: 's1', observedAt: new Date().toISOString(), facts }),
    execute: async input => { calls.push(input); return { state: 'completed', source: input.source, evidence: 'host:review-current-source' }; } };
  return { root, id, decideOffer, host, calls, controller: actionController(root, { session: 's1', task: 'T1' }, host) };
}

test('explicit host action execution requires a real decision and keeps one durable attempt per offer', async t => {
  const f = fixture(t);
  assert.equal((await actionController(f.root, { session: 's1', task: 'T1' }).execute(f.id)).supported, false);
  await assert.rejects(f.controller.execute(f.id), /pending|approval/);
  f.decideOffer('declined'); await assert.rejects(f.controller.execute(f.id), /declined/);
  assert.equal(f.calls.length, 0);
  assert.equal(decide('feature-report', { state: readSessionState(f.root, 's1', 'T1'), repository: {} }).features.review.status, 'unknown');
  f.decideOffer('approved');
  assert.equal((await f.controller.execute(f.id)).executed, true);
  const observed = readSessionState(f.root, 's1', 'T1').featureOutcomes.review;
  assert.equal(observed.status, 'used');
  assert.equal(observed.provenance, 'runtime-observed');
  assert.equal(observed.evidence, 'host:review-current-source');
  assert.equal((await f.controller.execute(f.id)).duplicate, true);
  assert.equal(f.calls.length, 1);
});

test('unknown native outcomes never redispatch automatically and reconcile from a scoped source receipt', async t => {
  const f = fixture(t); f.decideOffer('approved');
  f.host.execute = async input => { f.calls.push(input); throw new Error('lost acknowledgement'); };
  const first = await f.controller.execute(f.id);
  assert.equal(first.uncertain, true); assert.equal(first.executed, false);
  assert.equal((await f.controller.execute(f.id)).duplicate, true);
  f.host.lookup = async input => ({ state: 'completed', source: input.source, evidence: 'host:operation-confirmed' });
  assert.equal((await f.controller.reconcile(f.id)).executed, true);
  assert.equal(f.calls.length, 1);
});

test('concurrent action callers share the durable claim while the native call is pending', async t => {
  const f = fixture(t); f.decideOffer('approved');
  let finish;
  f.host.execute = input => { f.calls.push(input); return new Promise(resolve => { finish = () => resolve({ state: 'completed', source: input.source, evidence: 'host:review-complete' }); }); };
  const pending = f.controller.execute(f.id);
  // Both calls reach their asynchronous host observations before either claim is persisted.
  const second = await actionController(f.root, { session: 's1', task: 'T1' }, f.host).execute(f.id);
  assert.equal(second.duplicate, true); assert.equal(second.executed, false);
  assert.equal(f.calls.length, 1);
  finish(); assert.equal((await pending).executed, true);
});

test('source mutation rejects execution and malformed completion does not claim a completed action', async t => {
  const f = fixture(t); f.decideOffer('approved');
  fs.appendFileSync(path.join(f.root, 'file.txt'), 'changed');
  await assert.rejects(f.controller.execute(f.id), /stale/); assert.equal(f.calls.length, 0);
  fs.writeFileSync(path.join(f.root, 'file.txt'), 'source');
  f.host.execute = async input => ({ state: 'completed', source: 'a'.repeat(64), evidence: 'host:wrong-source' });
  const result = await f.controller.execute(f.id);
  assert.equal(result.executed, false); assert.equal(result.uncertain, true);
});

test('amended task criteria invalidate an existing approval without a source edit', async t => {
  const f = fixture(t); f.decideOffer('approved');
  startTask(f.root, 's1', 'T1', { goal: 'Verify the scoped change', contract: fileURLToPath(new URL('./fixtures/acceptance/contract.json', import.meta.url)) });
  await assert.rejects(f.controller.execute(f.id), /stale.*contract/);
  assert.equal(f.calls.length, 0);
  assert.equal(readSessionState(f.root, 's1', 'T1').taskRecord.goalComplete, false);
});

test('compaction completion requires actual resume/acceptance references and negative action lookup requires fencing', () => {
  const id = 'a'.repeat(64), source = 'b'.repeat(64), key = 'c'.repeat(64), checkpoint = 'd'.repeat(64);
  const state = { recommendations: { [id]: { action: 'compact', executionReceipt: { key, source, checkpoint, generation: 0, state: 'dispatching' } } } };
  const finish = result => decide('action-finish', { state, id, key, generation: 0, result });
  assert.throws(() => finish({ state: 'completed', source, evidence: 'host:compact-issued' }), /resumed checkpoint/);
  assert.throws(() => finish({ state: 'not-started', source, evidence: 'host:missing' }), /fence/);
  const completed = finish({ state: 'completed', source, checkpoint, resumed: true, evidence: 'host:resumed', acceptanceEvidence: 'checks:resumed-task' });
  assert.equal(completed.receipt.state, 'completed');
  assert.equal(Object.hasOwn(state.recommendations[id], 'execution'), false);
  const later = decide('action-finish', { state: completed.state, id, key, generation: 0, result: { state: 'uncertain' } });
  assert.equal(later.ignored, true); assert.equal(later.receipt.state, 'completed');
});

test('recommendation retention never evicts native execution receipts, including stale or declined offers', () => {
  const recommendations = Object.fromEntries(Array.from({ length: 16 }, (_, index) => [String(index).padStart(64, '0'),
    { source: 'old-source', decision: 'declined', executionReceipt: { state: 'uncertain' } }]));
  const args = { state: { taskRecord: { id: 'task' }, recommendations },
    input: { action: 'review', scope: 'current source', benefit: 'Check risks', evidence: 'checks:current', priorAuthorization: 'user:review' },
    project: {}, repository: { gitAvailable: true, head: 'a'.repeat(40), fingerprints: { complete: true, index: 'index', workingTree: 'work' } },
    status: {}, facts: {}, session: 'session', now: Date.now() };
  assert.throws(() => decide('action-propose', args), /capacity reached/);
  assert.equal(Object.keys(recommendations).length, 16);
  const removable = Object.keys(recommendations)[15]; delete recommendations[removable].executionReceipt;
  const result = decide('action-propose', args);
  assert.equal(Object.keys(result.state.recommendations).length, 16);
  assert.equal(Object.hasOwn(result.state.recommendations, removable), false);
});
