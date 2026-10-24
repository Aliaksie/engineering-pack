import { stateFile } from '../platform/index.mjs';
import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import http from 'node:http';
import https from 'node:https';
import childProcess, { spawnSync } from 'node:child_process';
import { withTelemetry, observe, emitTelemetry, diagnostic, createTelemetry, flushTelemetry } from '../application/telemetry/observe.mjs';
import { telemetryEvent, traceContext } from '../application/telemetry/events.mjs';
import { readTelemetry, telemetryIdentity } from '../platform/node/telemetry.mjs';
import { otlpTraces } from '../integrations/otel/traces.mjs';
import { run } from '../cli/main.mjs';
import { execute } from '../cli/requests.mjs';
import { readProject } from '../application/project/configuration.mjs';
import { discover } from '../application/discovery/discover.mjs';
import { readCheckpoint } from '../application/checkpoint.mjs';

const uuid = '12345678-1234-4234-a234-123456789abc';
function fixture(t, telemetry) {
  const root=fs.mkdtempSync(path.join(os.tmpdir(),'pack-observe-'));
  t.after(() => fs.rmSync(root,{recursive:true,force:true}));
  fs.mkdirSync(path.join(root,'.craft'));
  if (telemetry !== undefined) fs.writeFileSync(path.join(root,'.craft/telemetry.json'),JSON.stringify(telemetry));
  return root;
}
function command(root, name, ...args) { return run([name,'--root',root,'--session','private-session',...args]); }
function tree(events, topName) {
  const top=events.find(event => event.name===topName && !event.parentOperationId);
  assert.ok(top,`missing ${topName} root`);
  const trace=events.filter(event => event.traceId===top.traceId), ids=new Set(trace.map(event => event.operationId));
  for (const event of trace) {
    assert.equal(event.schemaVersion,1); assert.equal(event.packVersion,'0.0.2');
    assert.equal(typeof event.timestamp,'string');
    if (event.parentOperationId) assert.ok(ids.has(event.parentOperationId),`${event.name} missing parent`);
    if (event.kind==='operation') assert.ok(event.durationMs>=0);
  }
  return {top,trace};
}
test('portable trace and diagnostic contract is deterministic, bounded and serializable',()=>{
  const input={traceId:'a'.repeat(32),operationId:uuid,parentOperationId:'87654321-1234-4234-a234-123456789abc'};
  assert.deepEqual(JSON.parse(JSON.stringify(traceContext(input))),input);
  const record={...input,name:'diagnostic',timestamp:'2026-10-19T00:00:00.000Z',sessionId:'b'.repeat(32),packVersion:'0.0.2',reasonCode:'CONFIG_INVALID',attributes:{area:'project',constructor:'private',toString:'private',error:'private',path:'/private/secret',command:'rm secret',droppedEvents:2}};
  assert.deepEqual(telemetryEvent(record),telemetryEvent(record));
  assert.deepEqual(telemetryEvent(record).attributes,{area:'project',droppedEvents:2});
  assert.ok(!JSON.stringify(telemetryEvent(record)).includes('private'));
  assert.throws(()=>traceContext({...input,traceId:'0'.repeat(32)}));
  assert.throws(()=>traceContext({...input,parentOperationId:uuid}));
  assert.throws(()=>telemetryEvent({...record,reasonCode:'raw private error'}));
  const span=otlpTraces([telemetryEvent(record)]).resourceSpans[0].scopeSpans[0].spans[0];
  assert.equal(span.traceId,input.traceId); assert.equal(span.parentSpanId,input.parentOperationId.replaceAll('-','').slice(0,16));
});
test('project/session isolation honors nested opt-out and separate async contexts',async t=>{
  const outer=fixture(t,{remote:{enabled:true,endpoint:'https://collector.example/v1/traces'}}), disabled=fixture(t,{local:false});
  withTelemetry(outer,{session:'outer'},()=>execute('route',{session:'inner',intent:'implement',file:[],risk:[]},disabled));
  withTelemetry(outer,{session:'outer'},()=>{
    readProject(disabled);
    assert.throws(()=>discover(disabled,{}),/invalid locator terms/);
    readCheckpoint(disabled,'inner');
  });
  assert.deepEqual(readTelemetry(outer),[]); assert.deepEqual(readTelemetry(disabled),[]);
  let exported=0; await flushTelemetry(()=>{exported++;return true;}); assert.equal(exported,0);
  const a=fixture(t), b=fixture(t);
  await Promise.all([a,b].map((root,index)=>withTelemetry(root,{session:`session-${index}`},()=>observe('cli.command',async()=>{
    await new Promise(resolve=>setImmediate(resolve)); emitTelemetry('session.advice',{decision:'continue'});
  }))));
  for (const [index,root] of [a,b].entries()) {
    const {trace}=tree(readTelemetry(root),'cli.command'); assert.equal(trace.length,2);
    assert.ok(trace.every(event=>event.sessionId===telemetryIdentity(root,`session-${index}`)));
  }
  withTelemetry(a,{session:'outer'},()=>withTelemetry(a,{session:'inner'},()=>emitTelemetry('session.advice',{decision:'continue'})));
  assert.equal(readTelemetry(a).at(-1).sessionId,telemetryIdentity(a,'inner'));
});
test('route, session and integration commands produce correlated roots and meaningful children',t=>{
  const root=fixture(t);
  const result=command(root,'route','--intent','implement'); assert.ok(result.skills.length);
  const {trace}=tree(readTelemetry(root),'cli.command');
  for (const name of ['project.loaded','routing.completed','guidance.selected']) assert.ok(trace.some(event=>event.name===name),name);
  const routing=trace.find(event=>event.name==='routing.completed');
  assert.ok(trace.filter(event=>event.name==='guidance.selected').every(event=>event.parentOperationId===routing.operationId));
  const session=command(root,'session'); assert.equal(session.usage.status,'unknown');
  const sessionEvents=readTelemetry(root), status=sessionEvents.find(event=>event.name==='session.observed');
  assert.ok(sessionEvents.some(event=>event.traceId===status.traceId && event.name==='diagnostic' && event.reasonCode==='HOST_OBSERVATION_UNAVAILABLE'));
  command(root,'prepare-services');
  assert.ok(readTelemetry(root).some(event=>event.name==='integration.prepared' && event.parentOperationId));
});
test('forwarded route preserves one command trace and pseudonym through session activity and failures', t => {
  for (const args of [[], ['--session', 's1', '--task', 'T-1'], ['--session', 'bad\u0001id'], ['--intent', 'invalid']]) {
    const root = fixture(t);
    const argv = ['route', '--root', root, ...(args.includes('--intent') ? [] : ['--intent', 'implement']), ...args];
    if (args.includes('invalid')) assert.throws(() => run(argv), /unknown intent/);
    else assert.ok(run(argv).skills.length);
    const events = readTelemetry(root);
    const { top, trace } = tree(events, 'cli.command');
    assert.equal(events.filter(event => event.name === 'cli.command').length, 1);
    assert.equal(trace.length, events.length);
    assert.ok(trace.every(event => event.sessionId === top.sessionId));
    if (args.includes('T-1')) {
      const updated = trace.find(event => event.name === 'state.updated');
      assert.equal(updated.parentOperationId, top.operationId);
    }
  }
});
test('native route preserves access-denied telemetry when the caller owns the command', { skip: process.platform === 'win32' || process.getuid?.() === 0 }, t => {
  const root = fixture(t), file = path.join(root, '.craft/project.json');
  fs.writeFileSync(file, '{}', { mode: 0 });
  assert.throws(() => command(root, 'route', '--intent', 'implement'), { code: 'EACCES' });
  const events = readTelemetry(root), { top } = tree(events, 'cli.command');
  assert.equal(top.reasonCode, 'ACCESS_DENIED');
  assert.equal(events.find(event => event.name === 'project.loaded').reasonCode, 'ACCESS_DENIED');
});
test('discovery correlates cache/search and records bounded failures without input bodies',t=>{
  const root=fixture(t); fs.writeFileSync(path.join(root,'source.txt'),'PRIVATE_SEARCH_MARKER');
  command(root,'locate','--path','source.txt','--term','PRIVATE_SEARCH_MARKER');
  const {trace}=tree(readTelemetry(root),'cli.command');
  for (const name of ['discovery.completed','cache.lookup','search.completed']) assert.ok(trace.some(event=>event.name===name),name);
  assert.ok(!JSON.stringify(trace).includes('PRIVATE_SEARCH_MARKER'));
  assert.throws(()=>command(root,'locate','--term','PRIVATE_SEARCH_MARKER','--path','missing'));
  assert.ok(readTelemetry(root).some(event=>event.name==='search.completed' && event.outcome==='failure' && event.reasonCode));
});
test('invalid configuration, state and arguments retain original failures and structured reasons',t=>{
  const root=fixture(t);
  fs.writeFileSync(path.join(root,'.craft/project.json'),'PRIVATE_INVALID_JSON');
  assert.throws(()=>command(root,'route','--intent','implement'));
  let events=readTelemetry(root); const {top}=tree(events,'cli.command'); assert.equal(top.outcome,'failure');
  assert.ok(events.some(event=>event.name==='project.loaded' && event.reasonCode==='CONFIG_INVALID'));
  assert.ok(!JSON.stringify(events).includes('PRIVATE_INVALID_JSON'));
  fs.unlinkSync(path.join(root,'.craft/project.json'));
  fs.writeFileSync(path.join(root,'request.json'),JSON.stringify({sessionId:'private-session',task:{id:'task'}}));
  const state=path.dirname(stateFile(root, 'probe.json', false, 'private-session'));
  // A malformed session receipt must remain advisory for the session view.
  command(root,'route','--intent','implement','--task','task');
  const sessionFile=fs.readdirSync(state).find(name=>name.startsWith('session-')); fs.writeFileSync(path.join(state,sessionFile),'PRIVATE_BAD_STATE');
  assert.equal(command(root,'session','--task','task').usage.status,'unknown');
  events=readTelemetry(root);
  assert.ok(events.some(event=>event.reasonCode==='STATE_UNAVAILABLE'));
  assert.ok(!JSON.stringify(events).includes('PRIVATE_BAD_STATE'));
  assert.throws(()=>command(root,'route','--PRIVATE_UNKNOWN','secret'));
  assert.ok(readTelemetry(root).some(event=>event.name==='cli.command' && event.reasonCode==='INVALID_INPUT'));
});
test('review preparation/validation and checkpoint flows have traced success and failure',t=>{
  const root=fixture(t), git=(...args)=>{
    const result=spawnSync('git',args,{cwd:root,encoding:'utf8'}); assert.equal(result.status,0,result.stderr); return result.stdout.trim();
  };
  git('init','--initial-branch=main'); fs.writeFileSync(path.join(root,'source.txt'),'source'); git('add','source.txt');
  git('-c','user.name=Test','-c','user.email=test@example.test','commit','-m','fixture');
  command(root,'checkpoint-save'); command(root,'checkpoint-read');
  for (const name of ['checkpoint.saved','checkpoint.recovered','checkpoint.read']) assert.ok(readTelemetry(root).some(event=>event.name===name && event.parentOperationId));
  const prepare=path.resolve('packages/craft-review/bin/prepare-review.mjs'), check=path.resolve('packages/craft-review/bin/check-review.mjs');
  const artifact=path.join(root,'snapshot.json');
  const result=spawnSync(process.execPath,[prepare,'--root',root,'--base','HEAD','--head','HEAD','--output',artifact],{cwd:root,encoding:'utf8'});
  assert.equal(result.status,0,result.stderr);
  const events=readTelemetry(root), {trace}=tree(events,'review.command');
  assert.ok(trace.some(event=>event.name==='review.completed' && event.attributes.phase==='prepared'));
  const invalid=path.join(root,'invalid-review.json'); fs.writeFileSync(invalid,'{}');
  const failure=spawnSync(process.execPath,[check,artifact,invalid],{cwd:root,encoding:'utf8'}); assert.equal(failure.status,2);
  assert.ok(readTelemetry(root).some(event=>event.name==='review.command' && event.reasonCode==='REVIEW_INVALID'));
});
test('attempt recording uses cumulative counter beyond bounded receipt retention',t=>{
  const root=fixture(t);
  const data=path.join(fs.mkdtempSync(path.join(os.tmpdir(),'attempt-')),'attempt.json');
  t.after(()=>fs.rmSync(path.dirname(data),{recursive:true,force:true}));
  fs.writeFileSync(data,JSON.stringify({operation:'verify',target:'acceptance',approach:'probe',evidence:'check.json',outcome:'passed'}));
  for(let index=0;index<26;index++) run(['session-attempt','--root',root,'--session','attempts','--task','task','--data',data]);
  const events=readTelemetry(root).filter(event=>event.name==='attempt.recorded');
  assert.deepEqual(events.slice(-3).map(event=>event.attributes.recordedAttempts),[24,25,26]);
  assert.ok(events.every(event=>!Object.hasOwn(event.attributes,'attemptNumber')));
});
test('queue overflow and exporter failures are local, bounded and never recursively exported',async t=>{
  const root=fixture(t,{remote:{enabled:true,endpoint:'https://collector.example/v1/traces'}});
  withTelemetry(root,{session:'export'},()=>observe('cli.command',()=>{
    for(let i=0;i<40;i++) emitTelemetry('session.advice',{decision:'continue'});
  }));
  let calls=0, spans;
  await flushTelemetry(async (_endpoint,payload)=>{calls++; spans=payload.resourceSpans; return {ok:false,failure:'timeout'};});
  assert.equal(calls,1); assert.equal(spans.length,32);
  assert.ok(spans.some(item => item.scopeSpans[0].spans[0].name === 'cli.command'));
  const events=readTelemetry(root);
  assert.equal(events.filter(event=>event.reasonCode==='EXPORT_QUEUE_FULL').length,9);
  const exported=events.find(event=>event.name==='telemetry.export');
  assert.equal(exported.reasonCode,'EXPORT_TIMEOUT');
  assert.equal(exported.attributes.exportedEvents,undefined); assert.equal(exported.attributes.batchSize,32);
  await flushTelemetry(()=>{calls++;}); assert.equal(calls,1);
});
test('local sink loss is reported after recovery and partial collector rejection is diagnosed safely',async t=>{
  const root=fixture(t,{remote:{enabled:true,endpoint:'https://collector.example/v1/traces'}});
  let writes=0; const handle=createTelemetry(root,{}, {appendTelemetry:(_root,event)=>{if(++writes===1)return false; return fs.appendFileSync(path.join(root,'captured.jsonl'),JSON.stringify(event)+'\n');}});
  handle.emit('session.advice'); handle.emit('session.advice');
  const captured=fs.readFileSync(path.join(root,'captured.jsonl'),'utf8'); assert.ok(captured.includes('TELEMETRY_EVENTS_DROPPED'));
  await flushTelemetry(async()=>({ok:true,statusCode:200,json:{partialSuccess:{rejectedSpans:'1',errorMessage:'PRIVATE_BACKEND_MESSAGE'}}}));
  const final=fs.readFileSync(path.join(root,'captured.jsonl'),'utf8');
  assert.ok(final.includes('EXPORT_PARTIAL_REJECTED')); assert.ok(!final.includes('PRIVATE_BACKEND_MESSAGE'));
});
test('observability alone never executes tools/models, adds context or opens default network connections',async t=>{
  const spies=[];
  for (const [object,name] of [[childProcess,'spawn'],[childProcess,'spawnSync'],[childProcess,'execFileSync'],[http,'request'],[https,'request'],[globalThis,'fetch']]) {
    spies.push(t.mock.method(object,name,()=>{throw new Error(`observability called ${name}`);}));
  }
  const root=fixture(t), result={guidance:'unchanged'};
  assert.equal(withTelemetry(root,{},()=>observe('cli.command',()=>{diagnostic('host','HOST_CAPABILITY_UNAVAILABLE');return result;})),result);
  const events=readTelemetry(root); assert.ok(events.length>=2);
  let remote=0; await flushTelemetry(()=>{remote++;}); assert.equal(remote,0);
  for (const spy of spies) assert.equal(spy.mock.callCount(),0);
});

test('generated native hooks trace success, malformed input and configuration failure without changing wire output',t=>{
  const enabled=fixture(t), disabled=fixture(t,{local:false});
  const hook=path.resolve('dist/claude/plugins/craft-delivery/platform-hooks/prompt-routing.mjs');
  const invoke=(root,input)=>spawnSync(process.execPath,[hook],{cwd:root,encoding:'utf8',input:typeof input === 'string' ? input : JSON.stringify(input)});
  const payload=root=>({cwd:root,session_id:'private-native-session',prompt:'review the change PRIVATE_PROMPT_MARKER'});
  const normal=invoke(enabled,payload(enabled)), off=invoke(disabled,payload(disabled));
  assert.equal(normal.status,0,normal.stderr); assert.equal(normal.stdout,off.stdout); assert.equal(normal.stderr,off.stderr);
  const {trace}=tree(readTelemetry(enabled),'host.hook');
  assert.ok(trace.some(event=>event.name==='routing.completed'));
  assert.ok(trace.some(event=>event.name==='project.loaded'));
  assert.ok(trace.every(event=>event.host==='claude'));
  assert.ok(!JSON.stringify(trace).includes('PRIVATE_PROMPT_MARKER')); assert.deepEqual(readTelemetry(disabled),[]);
  assert.equal(invoke(enabled,'PRIVATE_MALFORMED_PAYLOAD').status,0);
  assert.ok(readTelemetry(enabled).some(event=>event.reasonCode==='INVALID_INPUT'));
  fs.writeFileSync(path.join(enabled,'.craft/project.json'),'PRIVATE_CONFIG_BODY');
  assert.equal(invoke(enabled,payload(enabled)).status,0);
  const failures=readTelemetry(enabled);
  assert.ok(failures.some(event=>event.reasonCode==='CONFIG_INVALID'));
  assert.ok(failures.some(event=>event.name==='host.hook' && event.outcome==='failure'));
  assert.ok(!JSON.stringify(failures).includes('PRIVATE_CONFIG_BODY'));
});

test('unavailable correlation key and version metadata yield explicit unknowns and safe local diagnostics',t=>{
  const root=fixture(t); fs.mkdirSync(path.dirname(stateFile(root, 'probe.json')), { recursive: true });
  fs.writeFileSync(stateFile(root, 'telemetry-key.json'),'PRIVATE_CORRUPT_KEY');
  const handle=createTelemetry(root,{session:'PRIVATE_RAW_SESSION'},{runtimeVersion:()=>{throw new Error('PRIVATE_METADATA_ERROR');}});
  handle.emit('session.observed');
  const events=readTelemetry(root);
  assert.ok(events.some(event=>event.reasonCode==='TELEMETRY_IDENTITY_UNAVAILABLE'));
  assert.ok(events.some(event=>event.reasonCode==='TELEMETRY_METADATA_UNAVAILABLE'));
  assert.ok(events.every(event=>event.packVersion===null && event.attributes.identityScope==='invocation'));
  assert.ok(!JSON.stringify(events).includes('PRIVATE_'));
  assert.ok(otlpTraces(events).resourceSpans.every(item=>!item.resource.attributes.some(attribute=>attribute.key==='service.version')));
});

test('invalid event emission is diagnosed without storing its content or changing work',t=>{
  const root=fixture(t), result={done:true};
  assert.equal(withTelemetry(root,{},()=>observe('cli.command',()=>{
    emitTelemetry('PRIVATE_INVALID_EVENT',{message:'PRIVATE_ERROR_BODY'});
    return result;
  })),result);
  const events=readTelemetry(root);
  assert.ok(events.some(event=>event.reasonCode==='TELEMETRY_EVENT_INVALID'));
  assert.ok(!JSON.stringify(events).includes('PRIVATE_'));
  tree(events,'cli.command');
});
