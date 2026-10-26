import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { spawnSync } from 'node:child_process';
import { launch, run } from './craft.mjs';
import { recordEvent, readTelemetry, stateFile } from './state.mjs';

// Reports read the store before recording their own command event.
const telemetryReport = (root, { session } = {}) => run(['telemetry-report', '--root', root, ...(session === undefined ? [] : ['--session', session])]);
// Seeded evidence only: later commands stop recording their own events.
const seal = root => fs.writeFileSync(path.join(root, '.craft/telemetry.json'), '{"local":false}');
function fixture(t) {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'craft-telemetry-report-'));
  t.after(() => fs.rmSync(root, { recursive: true, force: true }));
  return root;
}

test('empty reports do not create a correlation key or invent applicability and session identity', t => {
  const root = fixture(t);
  fs.mkdirSync(path.join(root, '.craft'));
  fs.writeFileSync(path.join(root, '.craft/telemetry.json'), '{"local":false}');
  for (const session of [undefined, 'private-session']) {
    const result = telemetryReport(root, { session });
    assert.equal(result.retainedEvents, 0);
    assert.equal(result.scope, session ? 'retained-session' : 'retained-project');
    assert.equal(result.features.discovery.status, 'unobserved');
    assert.equal(result.loss.totalLostEvents, null);
    assert.equal(result.storage.completeHistory, false);
  }
  assert.deepEqual(fs.readdirSync(path.join(root, '.craft')), ['telemetry.json']);
});

test('report filters real scoped identities and keeps loss counts separate from observed feature outcomes', t => {
  const root = fixture(t);
  const session = 'private-one';
  recordEvent(root, 'discovery.completed', { session, attributes: { batchSize: 2 } });
  recordEvent(root, 'attempt.recorded', { session, attributes: { recordedAttempts: 40 } });
  recordEvent(root, 'attempt.recorded', { session, attributes: { recordedAttempts: 41 } });
  recordEvent(root, 'diagnostic', { session, attributes: { droppedEvents: 3 }, outcome: 'failure', reasonCode: 'TELEMETRY_EVENTS_DROPPED' });
  recordEvent(root, 'diagnostic', { session, attributes: { droppedEvents: 2 }, outcome: 'failure', reasonCode: 'EXPORT_QUEUE_FULL' });
  recordEvent(root, 'telemetry.export', { session, outcome: 'failure' });
  recordEvent(root, 'discovery.completed', { session: 'private-two', outcome: 'failure' });
  recordEvent(root, 'host.hook');
  seal(root);
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
  recordEvent(root, 'session.observed');
  seal(root);
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
  assert.throws(() => telemetryReport(root, { session: '' }), /invalid telemetry session/);
  assert.equal(fs.existsSync(path.join(root, 'target')), false);
});

test('a standalone Codex package launcher reports without local authoring modules', t => {
  const root = fixture(t);
  const exported = path.join(fixture(t), 'craft-delivery');
  fs.cpSync(path.resolve('dist/codex/packages/craft-delivery'), exported, { recursive: true });
  assert.ok(fs.existsSync(path.join(exported, 'bin/craft')), 'build exports before this test');
  const result = launch(exported, ['telemetry-report', '--root', root, '--session', 'private']);
  assert.equal(result.status, 0, result.stderr);
  assert.equal(JSON.parse(result.stdout).retainedEvents, 0);
});

test('an unknown command fails with usage and records one failed invocation', t => {
  const root = fixture(t);
  assert.throws(() => run(['absent-command', '--root', root]), /usage: craft/);
  const events = readTelemetry(root);
  assert.deepEqual(events.map(event => [event.name, event.outcome, event.reasonCode]), [['cli.command', 'failure', 'INVALID_INPUT']]);
});
