import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { spawnSync } from 'node:child_process';
import { telemetryReport } from '../application/telemetry/report.mjs';
import { createTelemetry } from '../application/telemetry/observe.mjs';
import { stateFile } from '../platform/index.mjs';

function fixture(t) {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'craft-telemetry-report-'));
  t.after(() => fs.rmSync(root, { recursive: true, force: true }));
  return root;
}

test('empty reports do not create state or invent applicability and session identity', t => {
  const root = fixture(t);
  for (const session of [undefined, 'private-session']) {
    const result = telemetryReport(root, { session });
    assert.equal(result.retainedEvents, 0);
    assert.equal(result.scope, session ? 'retained-session' : 'retained-project');
    assert.equal(result.features.discovery.status, 'unobserved');
    assert.equal(result.loss.totalLostEvents, null);
    assert.equal(result.storage.completeHistory, false);
  }
  assert.deepEqual(fs.readdirSync(root), []);
});

test('report filters real scoped identities and keeps loss counts separate from observed feature outcomes', t => {
  const root = fixture(t);
  const first = createTelemetry(root, { session: 'private-one' });
  first.emit('discovery.completed', { batchSize: 2 });
  first.emit('attempt.recorded', { recordedAttempts: 40 });
  first.emit('attempt.recorded', { recordedAttempts: 41 });
  first.emit('diagnostic', { droppedEvents: 3 }, 'failure', undefined, {}, 'TELEMETRY_EVENTS_DROPPED');
  first.emit('diagnostic', { droppedEvents: 2 }, 'failure', undefined, {}, 'EXPORT_QUEUE_FULL');
  first.emit('telemetry.export', {}, 'failure');
  createTelemetry(root, { session: 'private-two' }).emit('discovery.completed', {}, 'failure');
  createTelemetry(root).emit('host.hook');
  const result = telemetryReport(root, { session: 'private-one' });
  assert.equal(result.retainedEvents, 6);
  assert.deepEqual(result.features.discovery, { status: 'observed', events: 1, failures: 0, deferred: 0 });
  assert.equal(result.features.attempts.events, 2);
  assert.deepEqual(result.loss, { reportedLocalDrops: 3, reportedQueueDrops: 2, exportFailures: 1, totalLostEvents: null });
  assert.equal(result.identity.invocationOrUnknownEvents, 0);
  const all = telemetryReport(root);
  assert.equal(all.features.discovery.failures, 1);
  assert.equal(all.identity.invocationOrUnknownEvents, 1);
  assert.deepEqual(all.attribution, { packages: { unknown: 8 }, hookInputs: { unknown: 1 } });
  assert.equal(telemetryReport(root, { session: 'absent' }).retainedEvents, 0);
  assert.equal(JSON.stringify(result).includes('private-one'), false);
});

test('bounded storage report exposes rotation and skipped data without leaking raw records', t => {
  const root = fixture(t);
  createTelemetry(root).emit('session.observed');
  const file = stateFile(root, 'telemetry.jsonl');
  fs.copyFileSync(file, `${file}.1`);
  fs.appendFileSync(file, 'secret-malformed\nnull\n{"schemaVersion":2}\n{"schemaVersion":1,"secret":"private"}\n' + 'x'.repeat(8193) + '\n');
  const result = telemetryReport(root);
  assert.equal(result.storage.files, 2);
  assert.equal(result.storage.backupPresent, true);
  assert.equal(result.storage.malformedLines, 1);
  assert.equal(result.storage.unsupportedRecords, 2);
  assert.equal(result.storage.oversizedLines, 1);
  assert.equal(result.storage.invalidRecords, 1);
  assert.equal(result.retainedEvents, 2);
  assert.equal(JSON.stringify(result).includes('secret'), false);
  fs.unlinkSync(file);
  fs.symlinkSync(path.join(root, 'target'), file);
  assert.throws(() => telemetryReport(root), /invalid telemetry file/);
  assert.equal(fs.existsSync(path.join(root, 'target')), false);
});

test('standalone Codex CLI ships the report without needing local authoring modules', t => {
  const root = fixture(t);
  const exported = path.join(fixture(t), 'craft-delivery');
  fs.cpSync(path.resolve('dist/codex/packages/craft-delivery'), exported, { recursive: true });
  const cli = path.join(exported, 'support/cli/main.mjs');
  assert.ok(fs.existsSync(cli), 'build exports before this test');
  const result = spawnSync(process.execPath, [cli, 'telemetry-report', '--root', root, '--session', 'private'], { encoding: 'utf8' });
  assert.equal(result.status, 0, result.stderr);
  assert.equal(JSON.parse(result.stdout).retainedEvents, 0);
});
