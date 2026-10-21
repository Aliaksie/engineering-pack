import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import http from 'node:http';
import { spawn } from 'node:child_process';
import { performance } from 'node:perf_hooks';
import { telemetryEvent } from '../application/telemetry/events.mjs';
import { createTelemetry, telemetryOptions, withTelemetry, observe, flushTelemetry } from '../application/telemetry/observe.mjs';
import { appendTelemetry, readTelemetry, telemetryIdentity, postTelemetry, TELEMETRY_MAX_BYTES } from '../platform/node/telemetry.mjs';
import { runtimeVersion } from '../platform/index.mjs';
import { otlpTraces } from '../integrations/otel/traces.mjs';
import { execute } from '../cli/requests.mjs';
import { lockState, stateFile } from '../platform/node/state-files.mjs';

const envelope = { name:'routing.completed', timestamp:'2026-10-19T00:00:00.000Z', sessionId:'a'.repeat(32), operationId:'12345678-1234-4234-a234-123456789abc', packVersion:'0.0.2', host:'codex', outcome:'success', durationMs:12 };
function fixture(t, config) {
  const root = fs.mkdtempSync(path.join(os.tmpdir(),'pack-telemetry-'));
  t.after(() => fs.rmSync(root,{recursive:true,force:true}));
  if (config !== undefined) { fs.mkdirSync(path.join(root,'.craft')); fs.writeFileSync(path.join(root,'.craft/telemetry.json'),JSON.stringify(config)); }
  return root;
}
test('telemetry accepts only bounded safe metadata and a valid envelope', () => {
  const event = telemetryEvent({ ...envelope, prompt:'secret', attributes:{ intent:'implement', guidanceBytes:10, evidenceCount:Infinity, batchSize:-1, sourceCode:'secret', command:'secret', host:'secret', skill:'consumer:secret', reason:'token-value' } });
  assert.deepEqual(event.attributes,{intent:'implement',guidanceBytes:10});
  assert.equal(JSON.stringify(event).includes('secret'),false);
  for (const invalid of [{name:'arbitrary'}, {sessionId:'raw-session'}, {timestamp:'invalid'}, {packVersion:'bad'}]) assert.throws(() => telemetryEvent({...envelope,...invalid}));
  assert.equal(telemetryEvent({...envelope,attributes:{skill:'craft-review:review-change'}},['craft-review:review-change']).attributes.skill,'craft-review:review-change');
});
test('local default records version and project-scoped session correlation without session material', t => {
  const root = fixture(t), other = fixture(t);
  const first = createTelemetry(root,{session:'private-session',host:'codex'});
  first.emit('session.observed',{prompt:'secret'});
  createTelemetry(root,{session:'private-session'}).emit('session.advice',{});
  const events = readTelemetry(root);
  assert.equal(events.length,2); assert.equal(events[0].packVersion,runtimeVersion());
  assert.equal(events[0].sessionId,events[1].sessionId);
  assert.notEqual(telemetryIdentity(other,'private-session'),events[0].sessionId);
  assert.equal(JSON.stringify(events).includes('private-session'),false);
  assert.equal(events[0].attributes.identityScope,'session');
  createTelemetry(root).emit('session.observed'); createTelemetry(root).emit('session.observed');
  const invocations = readTelemetry(root).slice(-2);
  assert.equal(invocations[0].attributes.identityScope,'invocation');
  assert.notEqual(invocations[0].sessionId,invocations[1].sessionId);
});
test('disabled and malformed configuration never write or queue network work', async t => {
  for (const config of [{local:false}, {remote:{enabled:false}}, {local:'secret'}, {remote:{enabled:true,endpoint:'https://user:secret@example.test'}}, {remote:{enabled:true,endpoint:'http://remote.test'}}]) {
    const root = fixture(t,config);
    createTelemetry(root).emit('session.observed');
    if (config.local === false) assert.equal(fs.existsSync(path.join(root,'.craft/state')),false);
    else if (typeof config.local === 'string' || config.remote?.enabled) assert.ok(readTelemetry(root).some(event => event.reasonCode === 'TELEMETRY_CONFIG_INVALID'));
  }
  let calls=0; await flushTelemetry(async () => { calls++; }); assert.equal(calls,0);
  assert.equal(telemetryOptions().remote,undefined);
});
test('storage and projection failures preserve operation results and original exceptions', t => {
  const root = fixture(t); fs.mkdirSync(path.join(root,'.craft')); fs.writeFileSync(path.join(root,'.craft/state'),'blocked');
  const expected = {value:1}, failure = new Error('private failure');
  assert.equal(withTelemetry(root,{},() => observe('routing.completed',() => expected)),expected);
  assert.throws(() => withTelemetry(root,{},() => observe('routing.completed',() => {throw failure;})),error => error === failure);
  const writable=fixture(t);
  assert.equal(withTelemetry(writable,{},() => observe('routing.completed',() => expected,() => {throw failure;})),expected);
  assert.equal(readTelemetry(writable).length,0);
});
test('local files rotate, skip malformed records and reject symlinks and oversize input', t => {
  const root=fixture(t); const event=telemetryEvent(envelope);
  appendTelemetry(root,event);
  const file=path.join(root,'.craft/state/telemetry.jsonl');
  fs.writeFileSync(file,' '.repeat(TELEMETRY_MAX_BYTES)); appendTelemetry(root,event);
  assert.equal(fs.statSync(`${file}.1`).size,TELEMETRY_MAX_BYTES);
  assert.equal(readTelemetry(root).length,1);
  fs.appendFileSync(file,'broken\n{"schemaVersion":2}\n'); assert.equal(readTelemetry(root).length,1);
  assert.throws(() => appendTelemetry(root,{...event,body:'x'.repeat(8192)}));
  fs.writeFileSync(`${file}.lock`,''); assert.equal(appendTelemetry(root,event),false); fs.unlinkSync(`${file}.lock`);
  fs.unlinkSync(file); fs.symlinkSync(path.join(root,'target'),file);
  assert.throws(() => appendTelemetry(root,event)); assert.equal(fs.existsSync(path.join(root,'target')),false);
});
test('concurrent processes append complete JSONL events within the shared bound', async t => {
  const root=fixture(t), module=new URL('../platform/node/telemetry.mjs',import.meta.url).href;
  const script=`import {appendTelemetry} from ${JSON.stringify(module)}; let accepted=0; for(let i=0;i<40;i++) if (appendTelemetry(process.argv[1],${JSON.stringify(telemetryEvent(envelope))})) accepted++; process.stdout.write(JSON.stringify({accepted,dropped:40-accepted}));`;
  const writers=await Promise.all(Array.from({length:4},() => new Promise((resolve,reject) => {
    const child=spawn(process.execPath,['--input-type=module','-e',script,root],{stdio:'pipe'});
    let errors='', output=''; child.stderr.on('data',data => errors+=data); child.stdout.on('data',data => output+=data);
    child.on('error',reject); child.on('close',code => {
      if (code) { reject(new Error(errors)); return; }
      try { resolve(JSON.parse(output)); } catch (error) { reject(error); }
    });
  })));
  const accepted=writers.reduce((sum,writer)=>sum+writer.accepted,0);
  const dropped=writers.reduce((sum,writer)=>sum+writer.dropped,0);
  assert.equal(accepted+dropped,160);
  assert.ok(accepted>0);
  const lines=fs.readFileSync(path.join(root,'.craft/state/telemetry.jsonl'),'utf8').trim().split('\n');
  assert.equal(lines.length,accepted);
  for(const line of lines) assert.deepEqual(JSON.parse(line),telemetryEvent(envelope));
});
test('denied telemetry lock acquisition skips the event without touching another writer or weakening shared locks', t => {
  const root=fixture(t), event=telemetryEvent(envelope);
  const directory=path.join(root,'.craft/state'); fs.mkdirSync(directory,{recursive:true});
  const file=stateFile(root,'telemetry.jsonl'), lock=`${file}.lock`;
  fs.writeFileSync(lock,'another writer');
  const open=fs.openSync;
  for (const code of ['EPERM','EACCES','EBUSY','EIO']) {
    const failure=Object.assign(new Error('lock unavailable'),{code});
    const mock=t.mock.method(fs,'openSync',(name,...args) => {
      if (name===lock) throw failure;
      return open(name,...args);
    });
    try {
      if (code==='EIO') assert.throws(() => appendTelemetry(root,event),error => error===failure);
      else assert.equal(appendTelemetry(root,event),false);
      assert.throws(() => lockState(file),error => error===failure);
      assert.equal(fs.existsSync(file),false);
      assert.equal(fs.readFileSync(lock,'utf8'),'another writer');
    } finally { mock.mock.restore(); }
  }
  fs.unlinkSync(lock);
  assert.equal(appendTelemetry(root,event),true);
  assert.deepEqual(readTelemetry(root),[event]);
});
test('telemetry retries transient contention and preserves events after the writer releases its lock', t => {
  const root=fixture(t), event=telemetryEvent(envelope);
  const open=fs.openSync;
  let attempts=0;
  let now=0;
  const clock=t.mock.method(performance,'now',()=>now);
  const wait=t.mock.method(Atomics,'wait',(_array,_index,_value,milliseconds)=>{ now+=milliseconds; return 'timed-out'; });
  const mock=t.mock.method(fs,'openSync',(name,...args) => {
    if (String(name).endsWith('telemetry.jsonl.lock') && ++attempts <= 3) {
      throw Object.assign(new Error('busy writer'),{code:'EEXIST'});
    }
    return open(name,...args);
  });
  try { assert.equal(appendTelemetry(root,event),true); }
  finally { mock.mock.restore(); wait.mock.restore(); clock.mock.restore(); }
  assert.equal(attempts,4);
  assert.equal(now,6);
  assert.deepEqual(readTelemetry(root),[event]);
});
test('telemetry stops retrying when a wait overshoots the deadline without removing the active lock', t => {
  const root=fixture(t), event=telemetryEvent(envelope);
  const directory=path.join(root,'.craft/state'); fs.mkdirSync(directory,{recursive:true});
  const file=stateFile(root,'telemetry.jsonl'), lock=`${file}.lock`;
  fs.writeFileSync(lock,'active writer');
  let now=0, waits=0;
  const clock=t.mock.method(performance,'now',()=>now);
  const wait=t.mock.method(Atomics,'wait',()=>{ waits++; now+=30; return 'timed-out'; });
  try { assert.equal(appendTelemetry(root,event),false); }
  finally { wait.mock.restore(); clock.mock.restore(); }
  assert.equal(waits,1);
  assert.equal(fs.readFileSync(lock,'utf8'),'active writer');
  assert.equal(fs.existsSync(file),false);
});
test('real routing and session commands emit sanitized observations and keep occupancy unknown', t => {
  const root=fixture(t), options={intent:'implement',file:[],risk:[],host:'codex',session:'session',task:'task'};
  const routed=execute('route',options,root);
  assert.ok(routed.skills.length);
  const session=execute('session',options,root);
  assert.equal(session.usage.status,'unknown'); assert.equal(session.packVersion,runtimeVersion());
  const events=readTelemetry(root);
  assert.ok(events.some(event => event.name==='routing.completed'));
  assert.ok(events.some(event => event.name==='guidance.selected' && event.attributes.skill));
  assert.ok(events.some(event => event.name==='host.observed' && event.attributes.occupancyKnown===false));
});
test('OTLP JSON maps IDs, timestamps, safe attributes and version without an SDK', () => {
  const result=otlpTraces([telemetryEvent(envelope)]).resourceSpans[0];
  assert.ok(result.resource.attributes.some(item => item.key==='service.version' && item.value.stringValue==='0.0.2'));
  const span=result.scopeSpans[0].spans[0];
  assert.match(span.traceId,/^[a-f0-9]{32}$/); assert.match(span.spanId,/^[a-f0-9]{16}$/);
  assert.equal(BigInt(span.endTimeUnixNano)-BigInt(span.startTimeUnixNano),12000000n);
  assert.equal(span.kind,1); assert.equal(span.status.code,0); assert.equal(span.parentSpanId,undefined);
});
test('opt-in export reaches a local collector; rejection and timeouts stay advisory', async t => {
  const bodies=[];
  const server=http.createServer((req,res) => {
    if (req.url==='/timeout') return;
    const chunks=[]; req.on('data',data => chunks.push(data)); req.on('end',() => { bodies.push(JSON.parse(Buffer.concat(chunks))); res.writeHead(req.url==='/reject' ? 500 : 200,{'content-type':'application/json'}); res.end('{}'); });
  });
  await new Promise(resolve => server.listen(0,'127.0.0.1',resolve));
  t.after(() => {server.closeAllConnections(); server.close();});
  const base=`http://127.0.0.1:${server.address().port}`;
  const root=fixture(t,{remote:{enabled:true,endpoint:`${base}/v1/traces`}});
  createTelemetry(root,{session:'private'}).emit('session.observed',{prompt:'secret'});
  await flushTelemetry(); assert.equal(bodies.length,1); assert.equal(JSON.stringify(bodies).includes('private'),false);
  const payload=otlpTraces([telemetryEvent(envelope)]);
  assert.equal((await postTelemetry(`${base}/reject`,payload)).ok,false);
  assert.equal((await postTelemetry(`${base}/timeout`,payload,30)).failure,'timeout');
  await flushTelemetry(() => { throw new Error('secret'); });
});
