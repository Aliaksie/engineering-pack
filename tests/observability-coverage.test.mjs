import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import http from 'node:http';
import childProcess, { spawnSync } from 'node:child_process';
import { run } from './craft.mjs';
import { readTelemetry, stateFile } from './state.mjs';
import { readProject } from './project-settings.mjs';
import { readCheckpoint } from './checkpoint-cli.mjs';
import { runHook } from './hook-command.mjs';
import { craft } from './review-tools.mjs';

// Traces and diagnostics as `craft` records them. Envelope sanitizing and trace context:
// `cargo test -p craft-engine telemetry`; OTLP mapping and collector outcomes: `otlp_contracts`;
// export queue bounds: `cargo test -p craft-application a_full_export_queue`.
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
test('commands under a project that disables telemetry record nothing', t => {
  const disabled = fixture(t, { local: false });
  run(['worker-status', '--root', disabled, '--session', 'inner', '--task', 'task']);
  readProject(disabled);
  readCheckpoint(disabled, 'inner');
  assert.deepEqual(readTelemetry(disabled), []);
  assert.equal(fs.existsSync(path.join(disabled, '.craft/state')), false);
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
test('native route preserves access-denied telemetry and its I/O code', { skip: process.platform === 'win32' || process.getuid?.() === 0 }, t => {
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
  const artifact=path.join(root,'snapshot.json');
  const result=craft(['review-prepare','--root',root,'--base','HEAD','--head','HEAD','--output',artifact],{cwd:root});
  assert.equal(result.status,0,result.stderr);
  const events=readTelemetry(root), {trace}=tree(events,'review.command');
  assert.ok(trace.some(event=>event.name==='review.completed' && event.attributes.phase==='prepared'));
  const invalid=path.join(root,'invalid-review.json'); fs.writeFileSync(invalid,'{}');
  const failure=craft(['review-check','--snapshot',artifact,'--review',invalid],{cwd:root}); assert.equal(failure.status,2);
  assert.ok(readTelemetry(root).some(event=>event.name==='review.command' && event.reasonCode==='REVIEW_INVALID'));
});
test('a command run directly by the native runtime posts its remote export', async t => {
  const spans=[];
  const server=http.createServer((request,response)=>{
    let body=''; request.on('data',chunk=>{ body+=chunk; });
    request.on('end',()=>{ spans.push(...JSON.parse(body).resourceSpans.flatMap(entry=>entry.scopeSpans[0].spans)); response.end('{}'); });
  });
  await new Promise(resolve=>server.listen(0,'127.0.0.1',resolve));
  t.after(()=>server.close());
  const root=fixture(t,{remote:{enabled:true,endpoint:`http://127.0.0.1:${server.address().port}/v1/traces`}});
  const status=await new Promise((resolve,reject)=>{
    const child=childProcess.spawn(process.env.CRAFT_NATIVE_BINARY,['review-check','--snapshot','absent.json','--review','absent.json'],{cwd:root,stdio:'ignore'});
    child.on('error',reject); child.on('close',resolve);
  });
  assert.equal(status,2);
  assert.deepEqual(spans.map(span=>span.name),['review.command']);
  const exported=readTelemetry(root).find(event=>event.name==='telemetry.export');
  assert.deepEqual([exported?.outcome,exported?.attributes.exportedEvents],['success',1]);
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
test('generated native hooks trace success, malformed input and configuration failure without changing wire output',t=>{
  const enabled=fixture(t), disabled=fixture(t,{local:false});
  const invoke=(root,input)=>runHook(path.resolve('dist/claude/plugins/craft-delivery'),'claude','prompt-routing',{cwd:root,input:typeof input === 'string' ? input : JSON.stringify(input)});
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

test('an unavailable correlation key yields invocation identities and a safe local diagnostic', t => {
  const root = fixture(t); fs.mkdirSync(path.dirname(stateFile(root, 'probe.json')), { recursive: true });
  fs.writeFileSync(stateFile(root, 'telemetry-key.json'), 'PRIVATE_CORRUPT_KEY');
  run(['route', '--root', root, '--intent', 'implement', '--session', 'PRIVATE_RAW_SESSION']);
  const events = readTelemetry(root);
  assert.ok(events.some(event => event.reasonCode === 'TELEMETRY_IDENTITY_UNAVAILABLE'));
  assert.ok(events.every(event => event.attributes.identityScope === 'invocation'));
  assert.ok(!JSON.stringify(events).includes('PRIVATE_'));
});
