import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { spawn } from 'node:child_process';
import { pathToFileURL } from 'node:url';
import { appendTelemetryBatch, readTelemetry, TELEMETRY_MAX_BYTES, TELEMETRY_BATCH_EVENTS } from '../platform/node/telemetry.mjs';
import { stateFile } from '../platform/index.mjs';
import { createTelemetry, withTelemetry, emitTelemetry } from '../application/telemetry/observe.mjs';
import { telemetryEvent } from '../application/telemetry/events.mjs';

const event = telemetryEvent({ name: 'session.observed', timestamp: '2026-10-20T00:00:00.000Z', sessionId: 'a'.repeat(32),
  operationId: '12345678-1234-4234-a234-123456789abc', packVersion: '0.0.2' });
function fixture(t) {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'craft-batch-'));
  t.after(() => fs.rmSync(root, { recursive: true, force: true }));
  return root;
}

test('a bounded batch acquires one lock and retains every event through rotation', t => {
  const root = fixture(t), file = stateFile(root, 'telemetry.jsonl', true);
  fs.writeFileSync(file, ' '.repeat(TELEMETRY_MAX_BYTES));
  const open = fs.openSync; let locks = 0;
  const mock = t.mock.method(fs, 'openSync', (name, ...args) => {
    if (name === `${file}.lock`) locks++;
    return open(name, ...args);
  });
  try { assert.equal(appendTelemetryBatch(root, Array(TELEMETRY_BATCH_EVENTS).fill(event)), true); }
  finally { mock.mock.restore(); }
  assert.equal(locks, 1);
  assert.equal(readTelemetry(root).length, TELEMETRY_BATCH_EVENTS);
  assert.equal(fs.statSync(`${file}.1`).size, TELEMETRY_MAX_BYTES);
  assert.ok(fs.statSync(file).size <= TELEMETRY_BATCH_EVENTS * 8192);
});

test('invalid batch size or an oversized member writes no partial group', t => {
  const root = fixture(t);
  for (const batch of [[], Array(TELEMETRY_BATCH_EVENTS + 1).fill(event), [event, { body: 'x'.repeat(8192) }]]) {
    assert.throws(() => appendTelemetryBatch(root, batch));
    assert.equal(fs.existsSync(stateFile(root, 'telemetry.jsonl')), false);
  }
});

test('invocation batches flush at the bound and finish while direct handles stay immediate', t => {
  const root = fixture(t), batches = [], singles = [];
  const ports = { appendTelemetryBatch: (_root, events) => { batches.push(events); return true; },
    appendTelemetry: (_root, event) => { singles.push(event); return true; } };
  const handle = createTelemetry(root, {}, ports, { batchLocal: true });
  for (let i = 0; i < 70; i++) handle.emit('session.observed');
  assert.deepEqual(batches.map(batch => batch.length), [32, 32]);
  handle.finish(); handle.finish();
  assert.deepEqual(batches.map(batch => batch.length), [32, 32, 6]);
  assert.equal(singles.length, 0);
  createTelemetry(root, {}, ports).emit('session.advice');
  assert.equal(singles.length, 1);
});

test('a failed batch counts failed events, preserves later events and reports loss once', t => {
  const root = fixture(t), stored = [];
  let calls = 0;
  const handle = createTelemetry(root, {}, {
    appendTelemetryBatch: (_root, events) => { if (++calls === 1) return false; stored.push(...events); return true; },
    appendTelemetry: (_root, event) => { stored.push(event); return true; }
  }, { batchLocal: true });
  for (let i = 0; i < 35; i++) handle.emit('session.observed');
  handle.finish(); handle.finish();
  assert.equal(stored.filter(event => event.name === 'session.observed').length, 3);
  const losses = stored.filter(event => event.reasonCode === 'TELEMETRY_EVENTS_DROPPED');
  assert.equal(losses.length, 1);
  assert.equal(losses[0].attributes.droppedEvents, 32);
});

test('synchronous and asynchronous operation failures flush pending local evidence without replacing the error', async t => {
  const root = fixture(t), error = new Error('original operation failure');
  assert.throws(() => withTelemetry(root, {}, () => {
    emitTelemetry('session.advice'); throw error;
  }), value => value === error);
  await assert.rejects(withTelemetry(root, {}, async () => {
    emitTelemetry('session.advice'); await Promise.resolve(); throw error;
  }), value => value === error);
  assert.equal(readTelemetry(root).length, 2);
});

test('concurrent batch writers retain complete JSON records and account for contended groups', async t => {
  const root = fixture(t), module = pathToFileURL(path.resolve('platform/node/telemetry.mjs')).href;
  const script = `import {appendTelemetryBatch} from ${JSON.stringify(module)}; let accepted=0; for(let i=0;i<8;i++) if(appendTelemetryBatch(process.argv[1],Array(5).fill(${JSON.stringify(event)}))) accepted+=5; console.log(JSON.stringify({accepted,dropped:40-accepted}));`;
  const results = await Promise.all(Array.from({ length: 4 }, () => new Promise((resolve, reject) => {
    const child = spawn(process.execPath, ['--input-type=module', '-e', script, root], { stdio: 'pipe' });
    let output = '', error = '';
    child.stdout.on('data', data => output += data); child.stderr.on('data', data => error += data);
    child.on('error', reject); child.on('close', status => status ? reject(new Error(error)) : resolve(JSON.parse(output)));
  })));
  const accepted = results.reduce((sum, result) => sum + result.accepted, 0);
  assert.ok(accepted > 0);
  assert.equal(accepted + results.reduce((sum, result) => sum + result.dropped, 0), 160);
  assert.equal(readTelemetry(root).length, accepted);
  for (const stored of readTelemetry(root)) assert.deepEqual(stored, event);
});
