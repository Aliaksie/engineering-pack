import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { spawnSync } from 'node:child_process';
import { pathToFileURL } from 'node:url';
import { createCodexControls, openCodexControls } from '../hosts/codex/control.mjs';
import { startTask, stepTask, taskAction } from '../application/session/task.mjs';

// Protocol fixture only: no native process, credentials or model calls.
function protocol() {
  const threads = new Map([['main', { id: 'main', sessionId: 'session', model: 'coordinator-model', status: { type: 'idle' } }]]);
  const turns = new Map(), calls = [], listeners = new Set(); let sequence = 0;
  const emit = value => { for (const listener of listeners) listener(value); };
  const complete = (threadId, status = 'completed') => {
    const turn = turns.get(threadId); turn.status = status; threads.get(threadId).status = { type: 'idle' };
    emit({ method: 'turn/completed', params: { threadId, turn: { ...turn } } });
  };
  const client = { subscribe(fn) { listeners.add(fn); return () => listeners.delete(fn); },
    async request(method, params) {
      calls.push({ method, params });
      if (method === 'thread/loaded/list') return { data: [...threads.values()].filter(t => t.status.type !== 'notLoaded').map(t => t.id), nextCursor: null };
      if (method === 'thread/read' || method === 'thread/resume') return { thread: { ...threads.get(params.threadId) } };
      if (method === 'thread/start') {
        const id = `worker-${++sequence}`;
        const thread = { id, model: params.model, status: { type: 'idle' } }; threads.set(id, thread);
        return { thread, model: thread.model, sandbox: { type: 'readOnly' }, approvalPolicy: 'never' };
      }
      if (['turn/start', 'review/start', 'thread/compact/start'].includes(method)) {
        const turn = { id: `turn-${++sequence}`, status: 'inProgress' }; turns.set(params.threadId, turn);
        threads.get(params.threadId).status = { type: 'active', activeFlags: [] };
        if (method === 'thread/compact/start') {
          emit({ method: 'item/completed', params: { threadId: params.threadId, turnId: turn.id, item: { id: 'compact-item', type: 'contextCompaction' } } });
          complete(params.threadId); return {};
        }
        if (client.completeEarly) complete(params.threadId);
        if (client.loseAck) throw new Error('lost native acknowledgement');
        return { turn: { ...turn }, ...(method === 'review/start' ? { reviewThreadId: params.threadId } : {}) };
      }
      if (method === 'thread/turns/list') return { data: turns.has(params.threadId) ? [{ ...turns.get(params.threadId) }] : [], nextCursor: null };
      if (method === 'thread/items/list') return { data: [{ item: { type: 'agentMessage', text: 'Scoped native fixture output' } }], nextCursor: null };
      if (method === 'turn/interrupt') return {};
      if (method === 'thread/unsubscribe') return { status: 'unsubscribed' };
      throw new Error(`unexpected method ${method}`);
    } };
  return { client, calls, threads, complete, emit };
}
function fixture(t, factory = createCodexControls) {
  const root = fs.realpathSync(fs.mkdtempSync(path.join(os.tmpdir(), 'craft-codex-control-')));
  const git = (...args) => {
    const result = spawnSync('git', ['-C', root, '-c', 'core.hooksPath=/dev/null', '-c', 'commit.gpgsign=false', '-c', 'user.name=Fixture', '-c', 'user.email=fixture@example.test', ...args], { encoding: 'utf8' });
    assert.equal(result.status, 0, result.stderr);
  };
  git('init', '-q'); fs.writeFileSync(path.join(root, 'value.mjs'), 'export const value = 1;');
  fs.mkdirSync(path.join(root, '.capabilities'));
  fs.writeFileSync(path.join(root, '.capabilities/project.json'), JSON.stringify({ formatVersion: 1,
    execution: { roles: { researcher: { mode: 'delegate', profile: 'standard' } }, hosts: { codex: { delegation: true, models: { standard: 'consumer-worker' } } } } }));
  git('add', '.'); git('commit', '-qm', 'fixture');
  startTask(root, 'session', 'task', { goal: 'Inspect bounded source' });
  const p = protocol(); let allowed = true, resumed = false;
  const options = { root, session: 'session', task: 'task', threadId: 'main', capacity: 2, client: p.client,
    authorize: async () => ({ allowed, evidence: 'user:scoped-native-action' }),
    observations: async () => ({ sessionId: 'session', observedAt: new Date().toISOString(), facts: { allowed, supported: true,
      observations: { host: { name: 'codex', reliableCompaction: true }, task: { safeBoundary: true },
        usage: { kind: 'current-context', currentTokens: 80, capacityTokens: 100, sessionId: 'session', observedAt: new Date().toISOString(), source: 'fixture-context' } } } }),
    verifyResume: async () => { resumed = true; return { acceptanceEvidence: 'checks:resumed-checkpoint' }; } };
  let controls = factory(options);
  t.after(() => { controls.close(); fs.rmSync(root, { recursive: true, force: true }); });
  const action = (op, input, facts = {}) => taskAction(root, 'session', 'task', op, input, facts);
  const offer = async kind => {
    const snapshot = await controls.workerHost.observe({ root, session: 'session', task: 'task' });
    const facts = (await options.observations()).facts;
    const result = action('action-propose', { action: kind, scope: 'value.mjs', benefit: 'Inspect scoped source', evidence: 'scope:current',
      ...(kind === 'delegate' ? { role: 'researcher' } : {}), priorAuthorization: 'user:scoped-native-action' },
    { ...facts, independentTasks: 2, workers: Object.fromEntries(['capacity', 'occupiedSlots', 'taskWorkers', 'allowed', 'sessionId', 'observedAt', 'source'].map(k => [k, snapshot[k]])) });
    assert.ok(result.offer, JSON.stringify(result)); return result.offer.id;
  };
  const assignment = id => ({ id: 'inspect-one', offer: id, scope: 'value.mjs', brief: { goal: 'Inspect source', sources: ['value.mjs'],
    acceptance: ['Return evidence'], restrictions: ['Read only'], returnShape: 'Findings' } });
  return { root, ...p, options, controls, offer, assignment, action, resumed: () => resumed, allowed: value => { allowed = value; },
    reopen(overrides = {}) { controls.close(); Object.assign(options, overrides); controls = factory(options); return controls; } };
}

test('Codex adapter translates admitted work to fresh read-only native threads and retains occupied completion', async t => {
  const f = fixture(t), input = f.assignment(await f.offer('delegate'));
  f.client.completeEarly = true;
  const result = await f.controls.workers.dispatch(input);
  assert.equal(result.executionObserved, true); assert.equal(result.request.status, 'idle');
  const start = f.calls.find(call => call.method === 'thread/start').params;
  assert.equal(start.model, 'consumer-worker'); assert.equal(start.sandbox, 'read-only'); assert.equal(start.approvalPolicy, 'never');
  assert.equal(start.config['features.multi_agent'], false);
  assert.equal(f.calls.some(call => call.method === 'thread/fork'), false);
  const turn = f.calls.find(call => call.method === 'turn/start').params;
  assert.equal(JSON.parse(turn.input[0].text).goal, input.brief.goal);
  assert.equal((await f.controls.workers.dispatch(input)).duplicate, true);
  await assert.rejects(f.controls.workers.dispatch({ ...input, id: 'two' }), /capacity|approval|ceiling/);
  const reused = await f.controls.workers.dispatch({ ...input, id: 'follow-up', reuseWorker: result.request.workerId });
  assert.equal(reused.executionObserved, true);
  assert.equal(f.calls.filter(call => call.method === 'thread/start').length, 1);
  await f.controls.release(result.request.workerId);
  assert.equal((await f.controls.workers.reconcile()).requests[0].occupied, true);
  f.threads.get(result.request.workerId).status = { type: 'notLoaded' };
  assert.equal((await f.controls.workers.reconcile()).requests[0].occupied, false);
});

test('lost acknowledgements survive adapter restart without replay or invented negative lookup', async t => {
  const f = fixture(t), input = f.assignment(await f.offer('delegate'));
  f.client.loseAck = true;
  assert.equal((await f.controls.workers.dispatch(input)).uncertain, true);
  const reopened = f.reopen();
  assert.equal((await reopened.workers.dispatch(input)).duplicate, true);
  const status = await reopened.workers.reconcile();
  assert.equal(status.requests[0].occupied, true);
  assert.equal(status.requests[0].state, 'uncertain');
  assert.equal(f.calls.filter(call => call.method === 'turn/start').length, 1);
});

test('interrupt acknowledgement does not release a worker or settle cancellation before native turn completion', async t => {
  const f = fixture(t), input = f.assignment(await f.offer('delegate'));
  const first = await f.controls.workers.dispatch(input);
  assert.equal((await f.controls.workers.cancel(input.id)).uncertain, true);
  f.threads.get(first.request.workerId).status = { type: 'idle' };
  assert.equal((await f.controls.workers.reconcile()).requests[0].state, 'uncertain');
  f.complete(first.request.workerId, 'interrupted');
  const settled = await f.controls.workers.reconcile();
  assert.equal(settled.requests[0].status, 'cancelled'); assert.equal(settled.requests[0].occupied, true);
});

test('inline native review completes through its scoped action receipt without repeat execution', async t => {
  const f = fixture(t), id = await f.offer('review'); f.client.completeEarly = true;
  assert.equal((await f.controls.actions.execute(id)).executed, true);
  assert.equal((await f.controls.actions.execute(id)).duplicate, true);
  const review = f.calls.find(call => call.method === 'review/start').params;
  assert.equal(review.delivery, 'inline'); assert.equal(review.threadId, 'main');
  assert.match(review.target.instructions, /value\.mjs/);
  assert.equal(f.calls.filter(call => call.method === 'review/start').length, 1);
  const op = f.controls.nativeStatus().operations.find(value => value.kind === 'review');
  assert.equal((await f.controls.output(op.key)).items[0].item.text, 'Scoped native fixture output');
});

test('native compaction requires lifecycle completion and checkpoint-bound resume acceptance', async t => {
  const f = fixture(t);
  stepTask(f.root, 'session', 'task', { stage: 'verify', target: 'acceptance:current', approach: 'bounded-check', evidence: 'checks:source',
    outcome: 'passed', result: 'Source verified', nextStep: 'Continue inspection' });
  const id = await f.offer('compact');
  const result = await f.controls.actions.execute(id);
  assert.equal(result.executed, false); assert.equal(f.resumed(), false);
  const resumed = await f.controls.actions.reconcile(id);
  assert.equal(resumed.executed, true); assert.equal(f.resumed(), true);
  assert.equal(f.calls.filter(call => call.method === 'thread/compact/start').length, 1);
  assert.equal(f.calls.filter(call => call.method === 'thread/resume').length, 1);
  assert.equal((await f.controls.actions.execute(id)).duplicate, true);
});

test('native compaction acknowledgements cannot substitute for resume acceptance or matching lifecycle completion', async t => {
  const f = fixture(t);
  const controls = f.reopen({ verifyResume: async () => ({}) });
  const input = { root: f.root, session: 'session', task: 'task', key: 'a'.repeat(64), action: 'compact', source: 'b'.repeat(64), checkpoint: 'c'.repeat(64), scope: 'value.mjs' };
  // Source checks reject stale direct native calls before even the compact RPC.
  await assert.rejects(controls.actionHost.execute(input), /source changed/);
  assert.equal(f.calls.some(call => call.method === 'thread/compact/start'), false);
  stepTask(f.root, 'session', 'task', { stage: 'verify', target: 'acceptance:current', approach: 'bounded-check', evidence: 'checks:source',
    outcome: 'passed', result: 'Source verified', nextStep: 'Continue inspection' });
  const id = await f.offer('compact');
  assert.equal((await controls.actions.execute(id)).executed, false);
  assert.equal((await controls.actions.reconcile(id)).executed, false);
  assert.equal(f.calls.filter(call => call.method === 'thread/resume').length, 1);
});

test('owned stdio entry initializes and resumes only the explicit coordinator without model execution', async t => {
  const root = fs.realpathSync(fs.mkdtempSync(path.join(os.tmpdir(), 'craft-codex-owned-')));
  let controls;
  t.after(() => { controls?.close(); fs.rmSync(root, { recursive: true, force: true }); });
  const code = `let initialized=false,buffer='';process.stdin.setEncoding('utf8');process.stdin.on('data',chunk=>{buffer+=chunk;let end;
while((end=buffer.indexOf('\\n'))>=0){const m=JSON.parse(buffer.slice(0,end));buffer=buffer.slice(end+1);let result;
if(m.method==='initialize')result={userAgent:'fixture'};
else if(m.method==='initialized'){initialized=true;continue;}
else if(m.method==='thread/resume'&&initialized&&m.params.threadId==='known-main')result={thread:{id:'known-main',sessionId:'known-session',status:{type:'idle'}}};
else {process.stdout.write(JSON.stringify({id:m.id,error:{code:-32601,message:'unexpected'}})+'\\n');continue;}
process.stdout.write(JSON.stringify({id:m.id,result})+'\\n');}});`;
  controls = await openCodexControls({ root, session: 'known-session', task: 'known-task', threadId: 'known-main', capacity: 2,
    authorize: async () => ({ allowed: false, evidence: 'user:no-effects' }),
    processOptions: { command: process.execPath, args: ['-e', code], cwd: root, timeoutMs: 3000 } });
  assert.deepEqual(controls.nativeStatus().operations, []);
});

test('native owner, coordinator binding, fresh authority and actual permission mapping are enforced', async t => {
  const f = fixture(t);
  assert.throws(() => createCodexControls(f.options), /already has an owner/);
  await assert.rejects(f.controls.workerHost.observe({ root: f.root, session: 'other', task: 'task' }), /binding/);
  const id = await f.offer('delegate'); f.allowed(false);
  assert.equal((await f.controls.workers.dispatch(f.assignment(id))).dispatchAttempted, false);
  f.allowed(true);
  const request = f.client.request;
  f.client.request = async (method, params) => {
    const result = await request(method, params);
    return method === 'thread/start' ? { ...result, sandbox: { type: 'dangerFullAccess' } } : result;
  };
  assert.equal((await f.controls.workers.dispatch(f.assignment(id))).uncertain, true);
  assert.equal(f.calls.some(call => call.method === 'turn/start'), false);
});

test('relocated standalone export ships the concrete Codex adapter with local runtime imports', async t => {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'craft-native-export-'));
  t.after(() => fs.rmSync(root, { recursive: true, force: true }));
  fs.cpSync(path.resolve('dist/codex/packages/craft-delivery/support'), path.join(root, 'support'), { recursive: true });
  const module = await import(pathToFileURL(path.join(root, 'support/hosts/codex/control.mjs')));
  const f = fixture(t, module.createCodexControls); f.client.completeEarly = true;
  assert.equal((await f.controls.workers.dispatch(f.assignment(await f.offer('delegate')))).executionObserved, true);
});
