import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { spawnSync } from 'node:child_process';
import { pathToFileURL } from 'node:url';
import { createHostControls, openHostControls } from '../hosts/control.mjs';
import { featureSupport } from '../hosts/capabilities.mjs';
import { stepTask, taskAction } from './task-cli.mjs';
import { run } from '../cli/main.mjs';

// task-start runs in the native runtime; tests bind tasks through the CLI.
const startTask = (root, session, task, { goal, risks = [], contract }, { host } = {}) => run(['task-start', '--root', root, '--session', session, '--task', task, '--goal', goal,
  ...risks.flatMap(risk => ['--risk', risk]), ...(contract ? ['--contract', contract] : []), ...(host ? ['--host', host] : [])]);

const clone = value => JSON.parse(JSON.stringify(value));
const brief = { goal: 'Inspect bounded source', sources: ['value.mjs'], acceptance: ['Return real source evidence'], restrictions: ['Read only'], returnShape: 'Findings' };
function repository(t, host) {
  const root = fs.realpathSync(fs.mkdtempSync(path.join(os.tmpdir(), 'craft-host-controls-')));
  const git = (...args) => {
    const result = spawnSync('git', ['-C', root, '-c', 'core.hooksPath=/dev/null', '-c', 'commit.gpgsign=false', '-c', 'user.name=Fixture', '-c', 'user.email=fixture@example.test', ...args], { encoding: 'utf8' });
    assert.equal(result.status, 0, result.stderr);
  };
  git('init', '-q'); fs.writeFileSync(path.join(root, 'value.mjs'), 'export const value = 1;');
  fs.mkdirSync(path.join(root, '.craft'));
  fs.writeFileSync(path.join(root, '.craft/project.json'), JSON.stringify({ formatVersion: 1,
    execution: { roles: { researcher: { mode: 'delegate', profile: 'standard' } }, hosts: { [host]: { delegation: true, models: { standard: 'consumer-worker' } } } } }));
  git('add', '.'); git('commit', '-qm', 'fixture');
  startTask(root, 'main-native', 'task', { goal: 'Inspect scoped source' });
  let controls;
  t.after(async () => { await controls?.close(); fs.rmSync(root, { recursive: true, force: true }); });
  const options = { root, session: 'main-native', task: 'task', coordinatorId: 'main-native', capacity: 2,
    authorize: async () => ({ allowed: true, evidence: 'user:scoped-work' }),
    observations: async () => ({ sessionId: 'main-native', observedAt: new Date().toISOString(), facts: { allowed: true, supported: true,
      observations: { host: { name: host, reliableCompaction: true }, task: { safeBoundary: true }, usage:
        { kind: 'current-context', currentTokens: 80, capacityTokens: 100, source: 'fixture:occupancy', sessionId: 'main-native', observedAt: new Date().toISOString() } } } }),
    verifyResume: async () => ({ acceptanceEvidence: 'checks:checkpoint-resumed' }) };
  const binding = { root, session: options.session, task: options.task };
  const offer = async kind => {
    const facts = (await options.observations()).facts;
    if (kind === 'delegate') {
      const view = await controls.workerHost.observe(binding);
      facts.independentTasks = 2;
      facts.workers = Object.fromEntries(['capacity', 'occupiedSlots', 'taskWorkers', 'allowed', 'sessionId', 'observedAt', 'source'].map(key => [key, view[key]]));
    }
    if (kind === 'compact') stepTask(root, options.session, 'task', { stage: 'verify', target: 'acceptance:source', approach: 'bounded-check', evidence: 'checks:source', outcome: 'passed', result: 'Scoped source checked', nextStep: 'Continue task' });
    const result = taskAction(root, options.session, 'task', 'action-propose', { action: kind, scope: 'value.mjs', benefit: 'Inspect source', evidence: 'scope:source', priorAuthorization: 'user:scoped-work', ...(kind === 'delegate' ? { role: 'researcher' } : {}) }, facts);
    assert.ok(result.offer, JSON.stringify(result)); return result.offer.id;
  };
  return { root, options, binding, offer, install(value) { controls = value; return value; }, assignment: offer => ({ id: 'one', offer, scope: 'value.mjs', brief }) };
}
function acp() {
  let sequence = 0;
  const configs = () => [{ id: 'model', category: 'model', type: 'select', currentValue: 'default-model', options: [{ value: 'default-model' }, { value: 'consumer-worker' }] },
    { id: 'mode', category: 'mode', type: 'select', currentValue: 'agent', options: [{ value: 'agent' }, { value: 'ask' }] }];
  const state = { configOptions: configs(), availableCommands: [{ name: 'compact' }] }, states = new Map([['main-native', clone(state)]]);
  const pending = new Map(), listeners = new Set(), calls = [];
  const emit = (sessionId, update) => { for (const listener of listeners) listener({ method: 'session/update', params: { sessionId, update } }); };
  const complete = (id, stopReason = 'end_turn') => { const resolve = pending.get(id); assert.ok(resolve); pending.delete(id); resolve({ stopReason }); };
  const client = { autoComplete: false, compactEvent: true,
    subscribe(fn) { listeners.add(fn); return () => listeners.delete(fn); },
    notify(method, params) { calls.push({ method, params }); },
    async request(method, params) {
      calls.push({ method, params });
      if (method === 'session/new') { const id = `acp-worker-${++sequence}`; states.set(id, clone(state)); return { sessionId: id, ...clone(states.get(id)) }; }
      if (method === 'session/set_config_option') {
        states.get(params.sessionId).configOptions.find(option => option.id === params.configId).currentValue = params.value;
        return clone(states.get(params.sessionId));
      }
      if (method === 'session/prompt') return new Promise(resolve => {
        pending.set(params.sessionId, resolve);
        emit(params.sessionId, { sessionUpdate: 'agent_message_chunk', content: { type: 'text', text: 'Actual protocol fixture output' } });
        if (params.prompt[0].text === '/compact' && client.compactEvent) emit(params.sessionId, { sessionUpdate: 'compaction_update', compactionId: 'compaction-one', status: 'completed' });
        if (client.autoComplete) complete(params.sessionId);
      });
      if (['session/resume', 'session/load'].includes(method)) return clone(states.get(params.sessionId));
      if (method === 'session/close') return {};
      throw new Error(`unexpected ACP method ${method}`);
    } };
  return { client, state, states, pending, complete, emit, calls };
}
for (const host of ['cursor', 'copilot', 'gemini', 'gemini-extension']) test(`${host}: ACP dispatch uses advertised read-only mode and exact model, shares limits and reuses idle sessions`, async t => {
  const f = repository(t, host), p = acp();
  const controls = f.install(createHostControls(host, { ...f.options, client: p.client, protocolVersion: 1, coordinatorState: p.state,
    agentCapabilities: { loadSession: true, sessionCapabilities: { close: {} } }, readOnlyMode: 'ask' }));
  const input = f.assignment(await f.offer('delegate'));
  const result = await controls.workers.dispatch(input);
  assert.equal(result.executionObserved, true); assert.equal(result.request.status, 'running');
  assert.deepEqual(p.calls.find(call => call.method === 'session/new').params.mcpServers, []);
  assert.equal(JSON.parse(p.calls.find(call => call.method === 'session/prompt').params.prompt[0].text).goal, brief.goal);
  await assert.rejects(controls.workers.dispatch({ ...input, id: 'two' }), /capacity|ceiling|approval/);
  p.complete(result.request.workerId); await new Promise(resolve => setImmediate(resolve));
  await controls.workers.reconcile();
  const reused = await controls.workers.dispatch({ ...input, id: 'reuse', reuseWorker: result.request.workerId });
  assert.equal(reused.executionObserved, true); assert.equal(p.calls.filter(call => call.method === 'session/new').length, 1);
  assert.equal((await controls.workers.cancel('reuse')).uncertain, true);
  p.complete(result.request.workerId, 'cancelled'); await new Promise(resolve => setImmediate(resolve));
  assert.equal((await controls.workers.reconcile()).requests.find(request => request.id === 'reuse').status, 'cancelled');
  await controls.release(result.request.workerId);
  assert.ok((await controls.workers.reconcile()).requests.every(request => request.occupied === false));
});

test('ACP review and compaction retain native results and require actual lifecycle plus resume acceptance', async t => {
  const f = repository(t, 'copilot'), p = acp(); p.client.autoComplete = true;
  const controls = f.install(createHostControls('copilot', { ...f.options, client: p.client, protocolVersion: 1, coordinatorState: p.state, agentCapabilities: { loadSession: true }, readOnlyMode: 'ask' }));
  const review = await f.offer('review');
  assert.equal((await controls.actions.execute(review)).executed, false);
  assert.equal((await controls.actions.reconcile(review)).executed, true);
  const op = controls.nativeStatus().operations.find(op => op.kind === 'review');
  assert.equal(controls.output(op.key).chunks[0], 'Actual protocol fixture output');
  const compact = await f.offer('compact');
  await controls.actions.execute(compact);
  assert.equal((await controls.actions.reconcile(compact)).executed, true);
  assert.equal(p.calls.filter(call => call.method === 'session/load').length, 1);
  assert.equal((await controls.actions.execute(compact)).duplicate, true);
});

test('ACP missing commands, model/mode mappings and compaction evidence fail closed', async t => {
  const f = repository(t, 'cursor'), p = acp(); p.client.autoComplete = true; p.client.compactEvent = false;
  const controls = f.install(createHostControls('cursor', { ...f.options, client: p.client, protocolVersion: 1, coordinatorState: p.state, agentCapabilities: { loadSession: true }, readOnlyMode: 'missing-mode' }));
  assert.equal((await controls.workers.dispatch(f.assignment(await f.offer('delegate')))).uncertain, true);
  assert.equal(p.calls.some(call => call.method === 'session/prompt'), false);
  const compact = await f.offer('compact'); await controls.actions.execute(compact);
  assert.equal((await controls.actions.reconcile(compact)).executed, false);
  assert.equal(p.calls.some(call => call.method === 'session/load'), false);
  p.emit('main-native', { sessionUpdate: 'available_commands_update', availableCommands: [] });
  assert.equal(controls.capabilities().compaction, false);
  await assert.rejects(controls.actionHost.execute({ ...f.binding, key: 'e'.repeat(64), action: 'compact', scope: 'scope', source: 'f'.repeat(64) }), /source|unsupported/);
  assert.equal(p.calls.filter(call => call.method === 'session/prompt').length, 1);
});

test('ACP native intent survives restart without reconstructing work from missing local state', async t => {
  const f = repository(t, 'gemini'), p = acp(), config = { ...f.options, client: p.client, protocolVersion: 1, coordinatorState: p.state, agentCapabilities: { loadSession: true }, readOnlyMode: 'ask' };
  let controls = f.install(createHostControls('gemini', config));
  const input = f.assignment(await f.offer('delegate'));
  await controls.workers.dispatch(input); controls.close();
  controls = f.install(createHostControls('gemini', config));
  assert.equal((await controls.workers.dispatch(input)).duplicate, true);
  assert.equal((await controls.workers.reconcile()).requests[0].occupied, true);
  assert.equal(p.calls.filter(call => call.method === 'session/new').length, 1);
});

test('reused worker history counts once against the approved concurrent assignment scope', async t => {
  const f = repository(t, 'cursor'), p = acp(); f.options.capacity = 3;
  const controls = f.install(createHostControls('cursor', { ...f.options, client: p.client, protocolVersion: 1, coordinatorState: p.state,
    agentCapabilities: { loadSession: true }, readOnlyMode: 'ask' }));
  const input = f.assignment(await f.offer('delegate'));
  const first = await controls.workers.dispatch(input); p.complete(first.request.workerId);
  await new Promise(resolve => setImmediate(resolve)); await controls.workers.reconcile();
  await controls.workers.dispatch({ ...input, id: 'reused', reuseWorker: first.request.workerId });
  const second = await controls.workers.dispatch({ ...input, id: 'independent' });
  assert.equal(second.executionObserved, true);
  assert.equal(p.calls.filter(call => call.method === 'session/new').length, 2);
});

test('unresolved native coordinator work blocks a different action after restart', async t => {
  const f = repository(t, 'copilot'), p = acp(), config = { ...f.options, client: p.client, protocolVersion: 1, coordinatorState: p.state,
    agentCapabilities: { loadSession: true }, readOnlyMode: 'ask' };
  let controls = f.install(createHostControls('copilot', config));
  const id = await f.offer('review'); await controls.actions.execute(id);
  const op = controls.nativeStatus().operations.find(op => op.kind === 'review'); controls.close();
  controls = f.install(createHostControls('copilot', config));
  await assert.rejects(controls.actionHost.execute({ ...f.binding, key: 'a'.repeat(64), action: 'review', scope: 'another scope', source: op.source }), /unresolved coordinator/);
  assert.equal(p.calls.filter(call => call.method === 'session/prompt').length, 1);
  assert.equal((await controls.actions.reconcile(id)).executed, false);
});

test('ACP stdio startup negotiates v1, restores native metadata, and sends no model prompt', async t => {
  const f = repository(t, 'cursor');
  const program = `let buffer='';process.stdin.setEncoding('utf8');process.stdin.on('data',chunk=>{buffer+=chunk;let end;
while((end=buffer.indexOf('\\n'))>=0){const m=JSON.parse(buffer.slice(0,end));buffer=buffer.slice(end+1);let result;
if(m.jsonrpc!=='2.0')process.exit(7);
if(m.method==='initialize')result={protocolVersion:1,agentCapabilities:{loadSession:true}};
else if(m.method==='session/load')result={configOptions:[],availableCommands:[{name:'compact'}]};
else {process.stdout.write(JSON.stringify({jsonrpc:'2.0',id:m.id,error:{code:-32601,message:'unexpected'}})+'\\n');continue;}
process.stdout.write(JSON.stringify({jsonrpc:'2.0',id:m.id,result})+'\\n');}});`;
  const controls = f.install(await openHostControls('cursor', { ...f.options, readOnlyMode: 'ask',
    processOptions: { command: process.execPath, args: ['-e', program], cwd: f.root, timeoutMs: 3000 } }));
  assert.equal(controls.capabilities().compaction, true);
  assert.deepEqual(controls.nativeStatus().operations, []);
});

function claude() {
  const calls = [], pending = new Map(); let sequence = 0;
  const query = ({ prompt, options }) => {
    const id = options.resume ?? `claude-worker-${++sequence}`;
    const call = { prompt, options, id, interrupted: false }; calls.push(call);
    const iterable = (async function* () {
      const compact = typeof prompt !== 'string';
      let text = prompt;
      if (compact) { for await (const message of prompt) text = message.message.content; }
      yield { type: 'system', subtype: 'init', session_id: id, model: options.model ?? 'consumer-worker' };
      if (text === '/compact') yield { type: 'system', subtype: 'compact_boundary', session_id: id };
      if (!query.autoComplete) await new Promise(resolve => pending.set(id, resolve));
      yield { type: 'assistant', session_id: id, message: { model: options.model ?? 'consumer-worker', content: [{ type: 'text', text: 'Native SDK fixture findings' }] } };
      yield { type: 'result', subtype: call.interrupted ? 'error_during_execution' : 'success', session_id: id, is_error: call.interrupted,
        usage: { input_tokens: 12, output_tokens: 8 }, duration_ms: 5, total_cost_usd: 0.01 };
    })();
    iterable.interrupt = async () => { call.interrupted = true; };
    iterable.close = () => { pending.get(id)?.(); pending.delete(id); };
    iterable.supportedCommands = async () => query.compactAvailable ? [{ name: 'compact' }] : [];
    return iterable;
  };
  query.autoComplete = false; query.compactAvailable = true;
  return { query, calls, complete(id) { assert.ok(pending.get(id)); pending.get(id)(); pending.delete(id); } };
}
test('Claude SDK worker uses fresh sessions, restricts tools, interrupts without early release and retains output', async t => {
  const f = repository(t, 'claude'), p = claude();
  const controls = f.install(createHostControls('claude', { ...f.options, query: p.query }));
  const input = f.assignment(await f.offer('delegate')), result = await controls.workers.dispatch(input);
  assert.equal(result.executionObserved, true); assert.equal(p.calls[0].options.resume, undefined);
  assert.deepEqual(p.calls[0].options.tools, ['Read', 'Glob', 'Grep']);
  assert.ok(p.calls[0].options.disallowedTools.includes('Agent'));
  assert.equal(p.calls[0].options.model, 'consumer-worker');
  assert.equal((await controls.workers.cancel(input.id)).uncertain, true);
  p.complete(result.request.workerId); await new Promise(resolve => setImmediate(resolve));
  assert.equal((await controls.workers.reconcile()).requests[0].status, 'cancelled');
  const op = controls.nativeStatus().operations.find(op => op.kind === 'worker');
  assert.equal(controls.output(op.key).nativeResult.usage.input_tokens, 12);
  await controls.release(result.request.workerId);
  assert.equal((await controls.workers.reconcile()).requests[0].occupied, false);
});

test('Claude SDK review and compaction use the exact coordinator and verify a real resumed query', async t => {
  const f = repository(t, 'claude'), p = claude(); p.query.autoComplete = true;
  const controls = f.install(createHostControls('claude', { ...f.options, query: p.query }));
  const review = await f.offer('review'); await controls.actions.execute(review); await new Promise(resolve => setImmediate(resolve));
  assert.equal((await controls.actions.reconcile(review)).executed, true);
  const compact = await f.offer('compact'); await controls.actions.execute(compact); await new Promise(resolve => setImmediate(resolve));
  assert.equal((await controls.actions.reconcile(compact)).executed, true);
  assert.ok(p.calls.every(call => call.options.resume === 'main-native'));
  assert.equal(p.calls.length, 3);
});

test('Claude unsupported compaction is rejected before command input is consumed', async t => {
  const f = repository(t, 'claude'), p = claude(); p.query.autoComplete = true; p.query.compactAvailable = false;
  const controls = f.install(createHostControls('claude', { ...f.options, query: p.query }));
  const id = await f.offer('compact'); await controls.actions.execute(id); await new Promise(resolve => setImmediate(resolve));
  assert.equal((await controls.actions.reconcile(id)).executed, false);
  assert.equal(p.calls.length, 1);
  assert.equal(controls.nativeStatus().operations[0].phase, 'intent');
  assert.equal(controls.output(controls.nativeStatus().operations[0].key).nativeResult, null);
  assert.throws(() => createHostControls('claude', { ...f.options, query: p.query, sdkOptions: { permissionMode: 'bypassPermissions' } }), /startup configuration/);
});

test('JetBrains and AGENTS retain manual fallback and bind only an explicit supported underlying agent', async t => {
  for (const adapter of ['jetbrains', 'agents']) {
    const f = repository(t, 'claude');
    const fallback = createHostControls(adapter, f.options);
    assert.equal(fallback.supported, false); assert.equal((await fallback.actions.execute('a'.repeat(64))).supported, false);
    assert.throws(() => createHostControls(adapter, { ...f.options, underlyingHost: 'unknown' }), /underlying/);
    const p = claude(); p.query.autoComplete = true;
    const controls = f.install(createHostControls(adapter, { ...f.options, underlyingHost: 'claude', query: p.query }));
    assert.equal(controls.nativeHost, 'claude'); assert.equal(controls.adapter, adapter);
    assert.equal((await controls.workers.dispatch(f.assignment(await f.offer('delegate')))).executionObserved, true);
  }
});

test('all adapter exports describe every feature and ship the same relocatable control entry', async t => {
  for (const adapter of ['codex', 'claude', 'cursor', 'copilot', 'gemini', 'gemini-extension', 'jetbrains', 'agents']) {
    const support = featureSupport(adapter);
    assert.equal(Object.keys(support.features).length, 18);
    assert.ok(Object.values(support.features).every(feature => feature.support && feature.activation && feature.evidence));
    const folder = adapter === 'claude' ? 'plugins' : 'packages';
    const setup = JSON.parse(fs.readFileSync(`dist/${adapter}/${folder}/craft-delivery/SETUP.json`, 'utf8'));
    assert.deepEqual(setup.featureSupport, support);
    const directory = fs.mkdtempSync(path.join(os.tmpdir(), 'craft-control-export-'));
    t.after(() => fs.rmSync(directory, { recursive: true, force: true }));
    fs.cpSync(`dist/${adapter}/${folder}/craft-delivery/support`, path.join(directory, 'support'), { recursive: true });
    const module = await import(pathToFileURL(path.join(directory, 'support/hosts/control.mjs')));
    assert.equal(typeof module.createHostControls, 'function'); assert.equal(typeof module.openHostControls, 'function');
  }
});

for (const host of ['cursor', 'claude']) test(`${host}: native admission rejects task switches during authorization for every work effect`, async t => {
  for (const kind of ['review', 'compact', 'delegate']) {
    const f = repository(t, host), p = host === 'claude' ? claude() : acp();
    const config = host === 'claude' ? { query: p.query } : { client: p.client, protocolVersion: 1, coordinatorState: p.state, agentCapabilities: { loadSession: true }, readOnlyMode: 'ask' };
    const controls = f.install(createHostControls(host, { ...f.options, ...config, authorize: async input => {
      if (input.key && input.action === (kind === 'delegate' ? 'dispatch' : kind)) startTask(f.root, 'main-native', 'new-task', { goal: 'A new active task' });
      return { allowed: true, evidence: 'user:scoped-work' };
    } }));
    const id = await f.offer(kind);
    if (kind === 'delegate') await controls.workers.dispatch(f.assignment(id));
    else await controls.actions.execute(id);
    assert.equal(p.calls.some(call => host === 'claude' || ['session/new', 'session/prompt', 'session/load'].includes(call.method)), false, kind);
  }
});

for (const host of ['cursor', 'claude']) test(`${host}: compaction reconciliation cannot resume after authority, source or active task changes`, async t => {
  for (const change of ['authority', 'source', 'task']) {
    const f = repository(t, host), p = host === 'claude' ? claude() : acp();
    let allowed = true, checks = 0;
    const config = host === 'claude' ? { query: p.query } : { client: p.client, protocolVersion: 1, coordinatorState: p.state, agentCapabilities: { loadSession: true }, readOnlyMode: 'ask' };
    if (host === 'claude') p.query.autoComplete = true; else p.client.autoComplete = true;
    const controls = f.install(createHostControls(host, { ...f.options, ...config, authorize: async () => { checks++; return { allowed, evidence: 'user:scoped-work' }; } }));
    const id = await f.offer('compact'); await controls.actions.execute(id);
    await new Promise(resolve => setImmediate(resolve));
    if (change === 'authority') allowed = false;
    if (change === 'source') fs.appendFileSync(path.join(f.root, 'value.mjs'), '// changed source');
    if (change === 'task') startTask(f.root, 'main-native', 'new-task', { goal: 'A new active task' });
    const before = checks, calls = p.calls.length;
    await assert.rejects(controls.actions.reconcile(id), /authority|source|active task binding/);
    assert.equal(checks, before + 1);
    assert.equal(p.calls.length, calls, change);
  }
});

for (const host of ['cursor', 'claude']) test(`${host}: premature release creates no intent and can succeed after completion`, async t => {
  const f = repository(t, host), p = host === 'claude' ? claude() : acp();
  const config = host === 'claude' ? { query: p.query } : { client: p.client, protocolVersion: 1, coordinatorState: p.state, agentCapabilities: { loadSession: true, sessionCapabilities: { close: {} } }, readOnlyMode: 'ask' };
  const controls = f.install(createHostControls(host, { ...f.options, ...config }));
  const result = await controls.workers.dispatch(f.assignment(await f.offer('delegate'))), id = result.request.workerId;
  await assert.rejects(controls.release(id), /settle/);
  assert.equal(controls.nativeStatus().operations.some(op => op.kind === 'release'), false);
  p.complete(id); await new Promise(resolve => setImmediate(resolve));
  assert.equal((await controls.release(id)).released, true);
  assert.equal((await controls.reconcileRelease(id)).released, true);
  assert.equal((await controls.release(id)).duplicate, true);
});

test('uncertain ACP release reconciles actual metadata without replaying the effect', async t => {
  const f = repository(t, 'cursor'), p = acp();
  const controls = f.install(createHostControls('cursor', { ...f.options, client: p.client, protocolVersion: 1, coordinatorState: p.state,
    agentCapabilities: { loadSession: true, sessionCapabilities: { close: {} } }, readOnlyMode: 'ask' }));
  const result = await controls.workers.dispatch(f.assignment(await f.offer('delegate'))), id = result.request.workerId;
  p.complete(id); await new Promise(resolve => setImmediate(resolve));
  const request = p.client.request;
  p.client.request = async (method, params) => {
    const result = await request(method, params);
    if (method === 'session/close') throw new Error('lost release acknowledgement');
    return result;
  };
  await assert.rejects(controls.release(id), /lost release/);
  assert.equal((await controls.reconcileRelease(id)).uncertain, true);
  assert.equal((await controls.release(id)).uncertain, true);
  assert.equal(p.calls.filter(call => call.method === 'session/close').length, 1);
});
