import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { spawn, spawnSync } from 'node:child_process';
import { pathToFileURL } from 'node:url';
import { startTask, taskAction } from '../application/session/task.mjs';
import { workerController } from '../application/session/workers.mjs';
import { decide } from '../application/engine-client.mjs';
import { readSessionState } from '../application/session/activity.mjs';

const brief = { goal: 'Inspect one independent source', sources: ['value.mjs'], acceptance: ['Return source evidence'], restrictions: ['Read only'], returnShape: 'Findings and evidence' };
function fixture(t, maxWorkers = 3) {
  const root = fs.realpathSync(fs.mkdtempSync(path.join(os.tmpdir(), 'craft-workers-')));
  t.after(() => fs.rmSync(root, { recursive: true, force: true }));
  const git = (...args) => {
    const result = spawnSync('git', ['-C', root, '-c', 'core.hooksPath=/dev/null', '-c', 'commit.gpgsign=false', '-c', 'user.name=Fixture', '-c', 'user.email=fixture@example.test', ...args], { encoding: 'utf8' });
    assert.equal(result.status, 0, result.stderr);
  };
  git('init', '-q'); fs.writeFileSync(path.join(root, 'value.mjs'), 'export const value = 1;\n');
  fs.mkdirSync(path.join(root, '.capabilities'));
  fs.writeFileSync(path.join(root, '.capabilities/project.json'), JSON.stringify({ formatVersion: 1,
    efficiency: { delegation: { maxWorkers } }, execution: { roles: { researcher: { mode: 'delegate', profile: 'standard' } },
      hosts: { fixture: { delegation: true, models: { standard: 'consumer-worker' } } } } }));
  git('add', '.'); git('commit', '-qm', 'fixture');
  startTask(root, 's1', 'T1', { goal: 'Deliver the scoped change' }, { host: 'fixture' });
  const workers = new Map(), operations = new Map(), fenced = new Set(), calls = [];
  let allowed = true, sequence = 0;
  const host = {
    observe: async ({ requests = [] } = {}) => ({ host: 'fixture', capacity: 4, occupiedSlots: 1 + [...workers.values()].filter(value => value.occupiesSlot).length,
      taskWorkers: [...workers.values()].filter(value => value.occupiesSlot).length, allowed, sessionId: 's1', observedAt: new Date().toISOString(), source: 'fixture-native-inventory', workers: [...workers.values()],
      requests: requests.flatMap(request => [request.key, request.cancelKey].filter(Boolean)).flatMap(key => operations.has(key) ? [{ key, status: 'started', settled: true, workerId: operations.get(key), evidence: 'host:lookup' }]
        : fenced.has(key) ? [{ key, status: 'not-started', fenced: true, workerId: null, evidence: 'host:permanent-rejection' }] : []) }),
    authorize: async () => ({ allowed, evidence: 'user:scoped-request' }),
    dispatch: async input => {
      calls.push(input);
      if (fenced.has(input.key)) throw new Error('permanently rejected operation');
      if (operations.has(input.key)) return { workerId: operations.get(input.key), ...workers.get(operations.get(input.key)), id: undefined };
      const id = input.reuseWorker ?? `worker-${++sequence}`;
      if (!input.reuseWorker && [...workers.values()].filter(value => value.occupiesSlot).length >= 3) throw new Error('native capacity');
      const value = { id, status: 'running', occupiesSlot: true, model: input.model, evidence: 'host:started' };
      workers.set(id, value); operations.set(input.key, id);
      const { id: _id, ...result } = value; return { workerId: id, ...result };
    },
    cancel: async ({ workerId, key }) => {
      operations.set(key, workerId);
      const worker = workers.get(workerId); Object.assign(worker, { status: 'cancelled', occupiesSlot: false });
      const { id: _id, ...result } = worker; return { workerId, ...result };
    }
  };
  const offer = async (approval = 'approved') => {
    const observed = await host.observe();
    const facts = { observations: { host: { name: 'fixture' } }, allowed: true, supported: true, independentTasks: 3,
      workers: Object.fromEntries(['capacity', 'occupiedSlots', 'taskWorkers', 'allowed', 'sessionId', 'observedAt', 'source'].map(field => [field, observed[field]])) };
    const proposed = taskAction(root, 's1', 'T1', 'action-propose', { action: 'delegate', scope: 'sources:independent', benefit: 'Inspect independent sources', evidence: 'scope:independent', role: 'researcher' }, facts);
    assert.ok(proposed.offer);
    if (approval) taskAction(root, 's1', 'T1', 'action-decide', { id: proposed.offer.id, decision: approval, evidence: 'user:decision' });
    return proposed.offer.id;
  };
  const controller = workerController(root, { session: 's1', task: 'T1' }, host);
  const assignment = (id, offerId, extra = {}) => ({ id, offer: offerId, scope: 'sources:independent', brief, ...extra });
  return { root, host, controller, offer, assignment, workers, operations, calls, fenced, setAllowed: value => { allowed = value; } };
}

test('host transport and scoped decision are required; successful dispatch sends only a bounded mapped brief', async t => {
  const f = fixture(t), input = f.assignment('one', await f.offer(null));
  assert.equal((await workerController(f.root, { session: 's1', task: 'T1' }).dispatch(input)).supported, false);
  await assert.rejects(f.controller.dispatch(input), /approval/);
  assert.equal(f.calls.length, 0);
  await f.offer('approved');
  const result = await f.controller.dispatch(input);
  assert.equal(result.executionObserved, true); assert.equal(result.modelMatches, true);
  const observed = readSessionState(f.root, 's1', 'T1').featureOutcomes.delegation;
  assert.equal(observed.status, 'used');
  assert.equal(observed.provenance, 'runtime-observed');
  assert.equal(observed.evidence, 'host:started');
  assert.equal(f.calls[0].model, 'consumer-worker');
  assert.deepEqual(f.calls[0].brief, brief);
  assert.equal(Object.hasOwn(f.calls[0], 'transcript'), false);
  assert.equal((await f.controller.dispatch(input)).duplicate, true);
  assert.equal(f.calls.length, 1);
  await assert.rejects(f.controller.dispatch({ ...input, brief: { ...brief, goal: 'Different task' } }), /different content/);
  f.setAllowed(false);
  assert.equal((await f.controller.dispatch(f.assignment('denied', input.offer))).dispatchAttempted, false);
});

test('reservations bound concurrent/nested dispatch; completed occupied workers do not free capacity', async t => {
  const f = fixture(t, 1), offer = await f.offer();
  const spawned = await f.controller.dispatch(f.assignment('parent', offer));
  await assert.rejects(f.controller.dispatch(f.assignment('nested', offer, { parent: 'parent' })), /approval|capacity|ceiling/);
  const worker = f.workers.get(spawned.request.workerId); worker.status = 'completed';
  await f.controller.reconcile();
  await assert.rejects(f.controller.dispatch(f.assignment('next', offer)), /approval|capacity|ceiling/);
  assert.equal(f.calls.length, 1);
  worker.status = 'released'; worker.occupiesSlot = false;
  await f.controller.reconcile();
  assert.equal((await f.controller.dispatch(f.assignment('next', offer))).executionObserved, true);
});

test('idle workers can be reused at full capacity; a new zero policy stops assignments without cancellation', async t => {
  const f = fixture(t, 1), offer = await f.offer();
  const first = await f.controller.dispatch(f.assignment('one', offer)), id = first.request.workerId;
  f.workers.get(id).status = 'idle'; await f.controller.reconcile();
  const reused = await f.controller.dispatch(f.assignment('two', offer, { reuseWorker: id }));
  assert.equal(reused.request.workerId, id); assert.equal(f.workers.size, 1);
  const preference = path.join(f.root, '.git', 'preferences.json');
  fs.writeFileSync(preference, '{"efficiency":{"delegation":{"maxWorkers":0}}}');
  const lowered = workerController(f.root, { session: 's1', task: 'T1', preferencesFile: preference }, f.host);
  await assert.rejects(lowered.dispatch(f.assignment('three', offer)), /stale|approval|ceiling/);
  assert.equal(f.workers.get(id).occupiesSlot, true); assert.equal(f.calls.length, 2);
});

test('lost native acknowledgement holds capacity and reconciles by the original operation key without redispatch', async t => {
  const f = fixture(t, 1), offer = await f.offer(), dispatch = f.host.dispatch;
  f.host.dispatch = async input => { await dispatch(input); throw new Error('acknowledgement lost'); };
  const result = await f.controller.dispatch(f.assignment('one', offer));
  assert.equal(result.uncertain, true); assert.equal(f.controller.status().requests[0].occupied, true);
  assert.equal((await f.controller.dispatch(f.assignment('one', offer))).duplicate, true);
  await assert.rejects(f.controller.dispatch(f.assignment('two', offer)), /approval|capacity|ceiling/);
  await f.controller.reconcile();
  assert.equal(f.controller.status().requests[0].workerId, 'worker-1');
  assert.equal(f.calls.length, 1);
});

test('negative lookup requires fencing, and a late dispatch result cannot overwrite newer cancellation', async t => {
  const f = fixture(t), offer = await f.offer(), native = f.host.dispatch;
  let resume, started;
  const ready = new Promise(resolve => { started = resolve; });
  f.host.dispatch = async input => { const result = await native(input); started(); await new Promise(resolve => { resume = resolve; }); return result; };
  const pending = f.controller.dispatch(f.assignment('one', offer)); await ready;
  await f.controller.reconcile();
  await f.controller.cancel('one');
  resume(); await pending;
  assert.equal(f.controller.status().requests[0].occupied, false);
  assert.equal(f.controller.status().requests[0].status, 'cancelled');
  const snapshot = await f.host.observe();
  snapshot.requests = [{ key: 'a'.repeat(64), status: 'not-started', workerId: null, evidence: 'host:missing' }];
  assert.throws(() => decide('worker-snapshot', { snapshot, session: 's1', now: Date.now() }), /fence/);
});

test('a native model mismatch remains visible and worker completion never marks task acceptance', async t => {
  const f = fixture(t), offer = await f.offer(), dispatch = f.host.dispatch;
  f.host.dispatch = async input => { const result = await dispatch(input); result.model = 'other-native-model'; f.workers.get(result.workerId).model = result.model; return result; };
  const result = await f.controller.dispatch(f.assignment('one', offer));
  assert.equal(result.modelMatches, false);
  assert.equal(result.request.occupied, true);
  await assert.rejects(f.controller.dispatch(f.assignment('child', offer, { parent: 'one' })), /parent/);
  assert.equal(readSessionState(f.root, 's1', 'T1').taskRecord.goalComplete, false);
});

test('simultaneous controllers share one tree ledger and never both pass a one-worker ceiling', async t => {
  const f = fixture(t, 1), offer = await f.offer();
  const other = workerController(f.root, { session: 's1', task: 'T1' }, f.host);
  const results = await Promise.allSettled([f.controller.dispatch(f.assignment('left', offer)), other.dispatch(f.assignment('right', offer))]);
  assert.equal(results.filter(value => value.status === 'fulfilled' && value.value.executionObserved).length, 1);
  assert.equal(f.calls.length, 1);
  assert.equal(f.controller.status().requests.filter(value => value.occupied).length, 1);
});

test('pending cancellation remains exclusive across idle observations and blocks worker reuse', async t => {
  const f = fixture(t, 1), offer = await f.offer();
  const first = await f.controller.dispatch(f.assignment('one', offer));
  f.workers.get(first.request.workerId).status = 'idle'; await f.controller.reconcile();
  let finish, entered;
  const ready = new Promise(resolve => { entered = resolve; }), cancel = f.host.cancel;
  f.host.cancel = async input => { entered(); await new Promise(resolve => { finish = resolve; }); return cancel(input); };
  const pending = f.controller.cancel('one'); await ready;
  await f.controller.reconcile();
  assert.equal(f.controller.status().requests[0].state, 'cancelling');
  await assert.rejects(f.controller.dispatch(f.assignment('two', offer, { reuseWorker: first.request.workerId })), /pending|scope|full/);
  finish(); await pending;
  assert.equal(f.calls.length, 1);
});

test('cross-process reservations reject a second dispatch even when both host snapshots omit the first worker', async t => {
  const f = fixture(t, 1), offer = await f.offer();
  const module = pathToFileURL(path.resolve('application/session/workers.mjs')).href;
  const script = `import fs from 'node:fs'; import path from 'node:path'; import {workerController} from ${JSON.stringify(module)};
const root=process.argv[1], input=JSON.parse(process.argv[2]);
const host={authorize:async()=>({allowed:true,evidence:'fixture:scope'}),observe:async()=>({host:'fixture',capacity:4,occupiedSlots:1,taskWorkers:0,allowed:true,sessionId:'s1',observedAt:new Date().toISOString(),source:'fixture:stale-but-fresh-inventory',workers:[],requests:[]}),dispatch:async call=>{fs.writeFileSync(path.join(root,'.git','effect-'+call.key),'one',{flag:'wx'});return {workerId:call.key,model:call.model,status:'running',occupiesSlot:true,evidence:'fixture:effect'};}};
try{console.log(JSON.stringify(await workerController(root,{session:'s1',task:'T1'},host).dispatch(input)));}catch(error){if(!/capacity|ceiling|busy|approval/.test(error.message))throw error;console.log(JSON.stringify({rejected:true}));}`;
  const results = await Promise.all(['left', 'right'].map(id => new Promise((resolve, reject) => {
    const child = spawn(process.execPath, ['--input-type=module', '-e', script, f.root, JSON.stringify(f.assignment(id, offer))], { stdio: 'pipe' });
    let output = '', error = '';
    child.stdout.on('data', data => output += data); child.stderr.on('data', data => error += data);
    child.on('error', reject); child.on('close', status => status ? reject(new Error(error)) : resolve(JSON.parse(output)));
  })));
  assert.equal(fs.readdirSync(path.join(f.root, '.git')).filter(name => name.startsWith('effect-')).length, 1);
  assert.equal(results.filter(value => value.rejected).length, 1);
});
