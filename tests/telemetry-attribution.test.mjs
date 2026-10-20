import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { spawnSync } from 'node:child_process';
import { createTelemetry, withTelemetry, emitTelemetry, flushTelemetry } from '../application/telemetry/observe.mjs';
import { readTelemetry } from '../platform/index.mjs';
import { telemetryReport } from '../application/telemetry/report.mjs';

function fixture(t) {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'craft-attribution-'));
  t.after(() => fs.rmSync(root, { recursive: true, force: true }));
  return root;
}

test('nested package scopes isolate traces and inherit package identity only within their scope', t => {
  const root = fixture(t);
  withTelemetry(root, { session: 'private', packageName: 'craft-delivery' }, () => {
    emitTelemetry('session.observed');
    withTelemetry(root, { packageName: 'craft-review' }, () => emitTelemetry('review.completed'));
    withTelemetry(root, {}, () => emitTelemetry('session.advice'));
  });
  createTelemetry(root, { packageName: 'PRIVATE_CUSTOM_PACKAGE' }).emit('session.observed');
  const events = readTelemetry(root);
  assert.deepEqual(events.map(event => event.attributes.package), ['craft-delivery', 'craft-review', 'craft-delivery', undefined]);
  assert.equal(events[0].traceId, events[2].traceId);
  assert.notEqual(events[0].traceId, events[1].traceId);
  assert.equal(events[0].sessionId, events[1].sessionId);
  assert.equal(JSON.stringify(events).includes('PRIVATE_'), false);
  assert.deepEqual(telemetryReport(root).attribution.packages, { 'craft-delivery': 2, 'craft-review': 1, unknown: 1 });
});

test('loss counts survive failed diagnostic appends and repeated finish until actually stored', t => {
  const root = fixture(t), stored = [];
  let writes = 0;
  const handle = createTelemetry(root, {}, { appendTelemetry: (_root, event) => {
    writes++;
    if ([1, 3, 4].includes(writes)) return false;
    stored.push(event); return true;
  } });
  handle.emit('session.observed'); // Lost event.
  handle.emit('session.observed'); // Event succeeds; diagnostic fails.
  handle.finish(); // Diagnostic still fails.
  handle.finish(); // Recovery preserves the original count.
  handle.finish(); // No duplicate diagnostic.
  const losses = stored.filter(event => event.reasonCode === 'TELEMETRY_EVENTS_DROPPED');
  assert.equal(losses.length, 1);
  assert.equal(losses[0].attributes.droppedEvents, 1);
  assert.equal(writes, 5);
});

test('a valid JSON prefix followed by oversized whitespace never claims a session from partial input', t => {
  const root = fixture(t);
  const entry = path.resolve('dist/codex/packages/craft-engineering/platform-hooks/observe-before-tool.mjs');
  const result = spawnSync(process.execPath, [entry], { cwd: root, encoding: 'utf8',
    input: JSON.stringify({ session_id: 'PRIVATE_SESSION', cwd: root }) + ' '.repeat(70000) });
  assert.equal(result.status, 0, result.stderr);
  assert.equal(result.stdout, ''); assert.equal(result.stderr, '');
  const events = readTelemetry(root);
  assert.ok(events.some(event => event.reasonCode === 'INPUT_TOO_LARGE'));
  assert.ok(events.every(event => event.attributes.identityScope === 'invocation'));
  assert.ok(events.every(event => event.attributes.package === 'craft-engineering'));
  assert.equal(telemetryReport(root).attribution.hookInputs.oversized, 1);
});

test('queue loss diagnostics retain dropped counts across a failing local sink without recursive export', async t => {
  const root = fixture(t), stored = [];
  fs.mkdirSync(path.join(root, '.capabilities'));
  fs.writeFileSync(path.join(root, '.capabilities/telemetry.json'), JSON.stringify({ remote: { enabled: true, endpoint: 'https://collector.example/v1/traces' } }));
  let recovering = false;
  const handle = createTelemetry(root, {}, { appendTelemetry: (_root, event) => {
    if (event.reasonCode === 'EXPORT_QUEUE_FULL' && !recovering) return false;
    stored.push(event); return true;
  } });
  for (let i = 0; i < 35; i++) handle.emit('session.observed');
  handle.finish();
  recovering = true;
  handle.finish(); handle.finish();
  const losses = stored.filter(event => event.reasonCode === 'EXPORT_QUEUE_FULL');
  assert.equal(losses.length, 1);
  assert.equal(losses[0].attributes.droppedEvents, 3);
  let exported = 0;
  await flushTelemetry(async (_endpoint, payload) => {
    const spans = payload.resourceSpans.flatMap(resource => resource.scopeSpans.flatMap(scope => scope.spans));
    exported += spans.length;
    assert.ok(spans.every(span => span.name === 'session.observed'));
    return { ok: true, statusCode: 200, json: {} };
  });
  assert.equal(exported, 32);
});

test('introducing a child package never attributes later events in an unknown parent', async t => {
  const root = fixture(t);
  await withTelemetry(root, { session: 'private' }, async () => {
    emitTelemetry('session.observed');
    await withTelemetry(root, { packageName: 'craft-review' }, async () => {
      await Promise.resolve(); emitTelemetry('review.completed');
      withTelemetry(root, {}, () => emitTelemetry('session.advice'));
    });
    emitTelemetry('session.observed');
  });
  const events = readTelemetry(root);
  assert.deepEqual(events.map(event => event.attributes.package), [undefined, 'craft-review', 'craft-review', undefined]);
  assert.equal(events[0].traceId, events[3].traceId);
  assert.notEqual(events[0].traceId, events[1].traceId);
  assert.equal(events[1].traceId, events[2].traceId);
});
