import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import { stepTask, taskAction } from './task-cli.mjs';
import { run } from '../cli/main.mjs';
import { actionController } from '../hosts/actions.mjs';
import { decide } from '../application/engine-client.mjs';
import { readSessionState } from './session-state.mjs';

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


test('native claims reject stale observations, changed binding and a busy binding writer before dispatch', async t => {
  const f = fixture(t); f.decideOffer('approved');
  for (const observation of [
    { sessionId: 's1', observedAt: new Date(Date.now() - 31000).toISOString(), facts: { allowed: true, supported: true } },
    { sessionId: 's1', observedAt: new Date(Date.now() + 60000).toISOString(), facts: { allowed: true, supported: true } },
    { sessionId: 'other', observedAt: new Date().toISOString(), facts: {} },
  ]) {
    f.host.observe = async () => observation;
    await assert.rejects(f.controller.execute(f.id), /fresh matching/);
  }
  const { bindingFile } = await import('./session-state.mjs');
  const lock = `${bindingFile(f.root, 's1', true)}.lock`;
  fs.writeFileSync(lock, '');
  f.host.observe = async () => ({ sessionId: 's1', observedAt: new Date().toISOString(), facts: { allowed: true, supported: true } });
  try { await assert.rejects(f.controller.execute(f.id), /writer is busy/); }
  finally { fs.unlinkSync(lock); }
  f.host.observe = async () => {
    startTask(f.root, 's1', 'T2', { goal: 'Another outcome' });
    return { sessionId: 's1', observedAt: new Date().toISOString(), facts: { allowed: true, supported: true } };
  };
  await assert.rejects(f.controller.execute(f.id), /active coordinator task/);
  assert.equal(f.calls.length, 0);
});

test('native compaction binds the checkpoint and records use only after matching resume evidence', async t => {
  const f = fixture(t);
  const facts = () => ({ allowed: true, supported: true, observations: {
    sessionId: 's1', host: { name: 'fixture', reliableCompaction: true },
    usage: { kind: 'current-context', currentTokens: 90, capacityTokens: 100, sessionId: 's1', source: 'host:context', observedAt: new Date().toISOString() },
  } });
  const offered = taskAction(f.root, 's1', 'T1', 'action-propose', { action: 'compact', scope: 'Current task', benefit: 'Continue with bounded context', evidence: 'host:context' }, facts());
  const id = offered.offer.id;
  taskAction(f.root, 's1', 'T1', 'action-decide', { id, decision: 'approved', evidence: 'user:compact' });
  f.host.observe = async () => ({ sessionId: 's1', observedAt: new Date().toISOString(), facts: facts() });
  f.host.execute = async input => ({ state: 'completed', source: input.source, evidence: 'host:issued' });
  const unconfirmed = await f.controller.execute(id);
  assert.equal(unconfirmed.uncertain, true);
  assert.equal(readSessionState(f.root, 's1', 'T1').featureOutcomes?.compaction, undefined);
  f.host.lookup = async input => ({ state: 'completed', source: input.source, checkpoint: input.checkpoint, resumed: true, evidence: 'host:resumed', acceptanceEvidence: 'checks:after-resume' });
  const result = await f.controller.reconcile(id);
  assert.equal(result.executed, true);
  assert.equal(readSessionState(f.root, 's1', 'T1').featureOutcomes.compaction.provenance, 'runtime-observed');
  assert.equal(result.receipt.checkpoint, decide('digest', { value: run(['checkpoint-read', '--root', f.root, '--session', 's1']).saved }));
});

test('authorization receives an isolated offer and native completion preserves a durable recovery key on storage failure', async t => {
  const f = fixture(t); f.decideOffer('approved');
  f.host.authorize = async ({ offer }) => { offer.scope = 'Changed scope'; return { allowed: true }; };
  const file = path.join(path.dirname((await import('../platform/index.mjs')).stateFile(f.root, 'probe', false, 's1')), `session-${decide('digest', { value: ['s1', 'T1'] }).slice(0,24)}.json`);
  f.host.execute = async input => {
    f.calls.push(input);
    fs.writeFileSync(`${file}.lock`, '');
    return { state: 'completed', source: input.source, evidence: 'host:completed' };
  };
  const result = await f.controller.execute(f.id);
  fs.unlinkSync(`${file}.lock`);
  assert.equal(result.uncertain, true);
  assert.equal(result.recoveryKey, f.calls[0].key);
  assert.notEqual(f.calls[0].scope, 'Changed scope');
  assert.equal((await f.controller.execute(f.id)).duplicate, true);
  assert.equal(f.calls.length, 1);
});
