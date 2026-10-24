import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';
import test from 'node:test';
import assert from 'node:assert/strict';
import { projectEvidenceProcess } from '../platform/node/project-evidence.mjs';

const repository = fileURLToPath(new URL('../', import.meta.url));

// The executable fixtures exercise the actual synchronous process boundary.
// POSIX shebangs/signals do not establish Windows acceptance.
for (const boundary of ['source', 'relocated']) {
  test(`native process failure contracts: ${boundary}`, { skip: process.platform === 'win32' }, async t => {
    const root = fs.realpathSync(fs.mkdtempSync(path.join(os.tmpdir(), 'craft-native-contract-')));
    t.after(() => fs.rmSync(root, { recursive: true, force: true }));
    let invoke = projectEvidenceProcess;
    if (boundary === 'relocated') {
      const bundle = path.join(root, 'bundle with spaces λ');
      fs.cpSync(path.join(repository, 'dist/agents'), bundle, { recursive: true });
      invoke = (await import(pathToFileURL(path.join(bundle, 'platform/node/project-evidence.mjs')).href)).projectEvidenceProcess;
    }
    const log = path.join(root, 'invocations.jsonl');
    function executable(body) {
      const binary = path.join(root, 'native fixture');
      fs.writeFileSync(log, '');
      fs.writeFileSync(binary, `#!${process.execPath}\nimport fs from 'node:fs';\nconst request=JSON.parse(fs.readFileSync(0,'utf8'));\nfs.appendFileSync(${JSON.stringify(log)},JSON.stringify(request)+'\\n');\nconst send=(result)=>console.log(JSON.stringify({formatVersion:1,ok:true,result}));\n${body}\n`, { mode: 0o700 });
      return binary;
    }
    const calls = () => fs.readFileSync(log, 'utf8').trim().split('\n').filter(Boolean).map(line => JSON.parse(line));
    const run = (binary, options = {}) => invoke({ enabled: true, binary, root, operation: 'status', ...options });
    const version = "if(request.operation==='version'){send({schemaVersion:1,binaryVersion:'0.0.1'});}else";
    await t.test('disabled access bypasses missing binary and invalid native options', () => {
      assert.equal(invoke({ binary: '/missing', operation: 'unknown', timeoutMs: 0 }).enabled, false);
    });
    await t.test('missing executable remains an explicit availability failure', () => {
      assert.throws(() => run(path.join(root, 'missing')), error => error.code === 'NATIVE_UNAVAILABLE');
    });
    await t.test('handshake precedes one operation with explicit root and input', () => {
      const binary = executable(`${version}{send(request.input);}`);
      assert.deepEqual(run(binary, { input: { marker: 'λ' } }), { marker: 'λ', root });
      assert.deepEqual(calls().map(call => call.operation), ['version', 'status']);
    });
    await t.test('incompatible handshake refuses the operation', () => {
      const binary = executable('send({schemaVersion:99,binaryVersion:"other"});');
      assert.throws(() => run(binary), /incompatible native evidence binary/);
      assert.equal(calls().length, 1);
    });
    await t.test('native error code survives without retry or fallback', () => {
      const binary = executable(`${version}{console.log(JSON.stringify({formatVersion:1,ok:false,error:{code:'FIXTURE_REFUSED',message:'refused'}}));}`);
      assert.throws(() => run(binary), error => error.code === 'FIXTURE_REFUSED');
      assert.equal(calls().length, 2);
    });
    await t.test('malformed output and unsuccessful exit remain failures', () => {
      for (const body of ['console.log("invalid");', 'send({});process.exitCode=7;']) {
        const binary = executable(`${version}{${body}}`);
        assert.throws(() => run(binary), /invalid native evidence response|exited unsuccessfully/);
        assert.equal(calls().length, 2);
      }
    });
    await t.test('signal interruption reports partial-effect uncertainty', () => {
      const binary = executable(`${version}{process.kill(process.pid,'SIGTERM');}`);
      assert.throws(() => run(binary), error => error.code === 'NATIVE_INTERRUPTED' && /partial/.test(error.message));
      assert.equal(calls().length, 2);
    });
    await t.test('operation deadline terminates the child without replaying effects', () => {
      const binary = executable(`${version}{fs.writeFileSync(${JSON.stringify(path.join(root, 'child.pid'))},String(process.pid));setInterval(()=>{},1000);}`);
      assert.throws(() => run(binary, { timeoutMs: 1000 }), error => error.code === 'NATIVE_UNAVAILABLE');
      assert.deepEqual(calls().map(call => call.operation), ['version', 'status']);
      const pid = Number(fs.readFileSync(path.join(root, 'child.pid'), 'utf8'));
      assert.throws(() => process.kill(pid, 0), error => error.code === 'ESRCH');
    });
  });
}
