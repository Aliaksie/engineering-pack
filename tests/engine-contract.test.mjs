import os from 'node:os';
import path from 'node:path';
import { spawnSync } from 'node:child_process';
import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import { createHash } from 'node:crypto';
import { evaluate } from '../engine/index.mjs';

const catalog = JSON.parse(fs.readFileSync(new URL('../engine/routing/catalog.json', import.meta.url), 'utf8'));
const request = (operation, input = {}) => ({ formatVersion: 1, operation, input });
const result = (operation, input) => {
  const response = evaluate(request(operation,input));
  assert.equal(response.ok,true,JSON.stringify(response));
  return response.result;
};

test('public engine accepts JSON, returns stable errors and leaves caller data unchanged', () => {
  assert.equal(evaluate({ formatVersion:2, operation:'route' }).error.code,'UNSUPPORTED_VERSION');
  assert.equal(evaluate(request('toString')).error.code,'UNKNOWN_OPERATION');
  assert.equal(evaluate(request('route',{ catalog, intent:'invented' })).error.code,'INVALID_INPUT');
  for (const value of [new Map(), new Date(), () => {}, Infinity, undefined]) {
    assert.equal(evaluate(request('digest',{ value })).error.code,'INVALID_INPUT');
  }
  const input = request('activity-update',{ state:{ activity:{} },values:{ routingDecisions:1 } });
  const before = JSON.stringify(input);
  assert.deepEqual(evaluate(input),JSON.parse(JSON.stringify(evaluate(input))));
  assert.equal(JSON.stringify(input),before);
});

test('routing and budgets replay from explicit JSON evidence without filesystem or host objects', () => {
  const project = { formatVersion:1, modules:[{ name:'app', path:'src', checks:[{ command:'check',args:['literal;no-shell'] }] }] };
  const plan = result('route',{ catalog, project, intent:'run-check', files:['src/main.js'] });
  assert.equal(plan.execution.mode,'tool');
  assert.deepEqual(plan.checks[0].args,['literal;no-shell']);
  const input = { sessionId:'session', task:{ stage:'implement', status:'completed', files:['src/main.js'] },
    usage:{ kind:'cumulative', currentTokens:999999, source:'fixture' } };
  const status = result('session-advice',{ catalog, project, input, options:{ now:0 } });
  assert.equal(status.usage.status,'unknown');
  assert.equal(status.recommendation.action,'run-check');
  const blocked = result('attempt-status',{ operation:'implement', records:[
    { operation:'implement',profile:'small',outcome:'permission-denied',evidence:'denied' },
    { operation:'implement',profile:'large',outcome:'quality-failure',evidence:'failed' }
  ] });
  assert.equal(blocked.blocked,true);
});

test('discovery JSON plans defer dependent work and invalidate packets without matching evidence', () => {
  const plan = result('discovery-request',{ input:{ paths:['src/./'],requests:[
    { id:'foo',term:'Foo' }, { id:'bar',term:'Bar' }, { id:'later',term:'Baz',dependsOn:['foo'] }
  ] } });
  assert.deepEqual(plan.terms,['Foo','Bar']);
  assert.deepEqual(plan.paths,['src']);
  assert.equal(plan.deferred[0].id,'later');
  const key = result('digest',{ value:['Foo',['src']] });
  const state = { lookups:{ [key]:{ identity:'revision-1', matches:[] } } };
  for (const identity of [null,'revision-2']) assert.deepEqual(result('lookup-reuse',{ terms:['Foo'],paths:['src'],identity,state }).fresh,['Foo']);
  assert.deepEqual(result('lookup-reuse',{ terms:['Foo'],paths:['src'],identity:'revision-1',state }).reused,['Foo']);
});

test('continuation source binding survives automatic snapshots and incomplete observations stay unknown', () => {
  const continuation = Object.fromEntries(['goal','currentState','completed','approach','evidence','filesChanged','validation','remainingWork','blockers','nextStep'].map(key => [key,key]));
  const repository = { head:'a',status:[],fingerprints:{ complete:true,index:'i',workingTree:'w' } };
  const saved = result('checkpoint-state',{ previous:{},metadata:{ taskId:'task',continuation },repository,updated:'2026-10-18T00:00:00Z' });
  const current = { ...repository,head:'b',fingerprints:{ ...repository.fingerprints,complete:false } };
  const automatic = result('checkpoint-state',{ previous:saved,metadata:{},repository:current,updated:'2026-10-18T01:00:00Z' });
  assert.deepEqual(automatic.continuationSource,saved.continuationSource);
  assert.equal(result('checkpoint-compare',{ saved:automatic,current }).changed,null);
});

test('portable hashes match standard SHA-256 for UTF-8, padding boundaries and long evidence', () => {
  for (const text of ['', 'abc', 'λ🙂', ...[55,56,63,64,65,1000,65536].map(length=>'x'.repeat(length))]) {
    assert.equal(result('hash',{ text }),createHash('sha256').update(text).digest('hex'));
  }
});

test('engine runs after relocation without Node globals, ambient clock or other repository layers', t => {
  const directory = fs.mkdtempSync(path.join(os.tmpdir(),'isolated engine '));
  t.after(() => fs.rmSync(directory,{ recursive:true,force:true }));
  fs.cpSync(path.resolve('engine'),path.join(directory,'engine'),{ recursive:true });
  const script = `
    const output = process.stdout.write.bind(process.stdout);
    globalThis.process = undefined;
    globalThis.Buffer = undefined;
    Date.now = () => { throw new Error('ambient clock'); };
    const { evaluate } = await import('./engine/index.mjs');
    const contract = ${fs.readFileSync(new URL('./fixtures/acceptance/contract.json', import.meta.url), 'utf8')};
    const acceptance = evaluate({ formatVersion:1, operation:'engineering-contract-digest', input:{ contract } });
    if (!acceptance.ok || !/^[a-f0-9]{64}$/.test(acceptance.result)) throw new Error('portable acceptance contract failed');
    const qa = evaluate({ formatVersion:1, operation:'qa-projection', input:{ contract,
      evidence:{ formatVersion:1, records:[] }, current:{ kind:'environment', identity:null }, probes:[] } });
    if (!qa.ok || qa.result.coverage.complete || qa.result.unexploredCriterionIds.length !== 2) throw new Error('portable QA projection failed');
    const result = evaluate({ formatVersion:1, operation:'attempt-status', input:{ operation:'implement', records:[] } });
    output(JSON.stringify(result));
  `;
  const child = spawnSync(process.execPath,['--input-type=module','--eval',script],{ cwd:directory,encoding:'utf8',timeout:10000 });
  assert.equal(child.status,0,child.stderr);
  assert.equal(JSON.parse(child.stdout).result.attempts,0);
});
