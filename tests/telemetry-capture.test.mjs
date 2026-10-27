import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { spawnSync, spawn } from 'node:child_process';
import { launch, run } from './craft.mjs';
import { digest, recordEvent, stateFile } from './state.mjs';

const CAPTURE_MAX_ENTRIES = 8, CAPTURE_MAX_BYTES = 65536;
function fixture(t) {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'craft-capture-'));
  t.after(() => fs.rmSync(root, { recursive: true, force: true }));
  return root;
}
const file = root => stateFile(root, 'telemetry/captures.json');
const observe = (root, session = 'PRIVATE_SESSION') => recordEvent(root, 'discovery.completed', { session });
// Seeded evidence only: later commands stop recording their own events.
const seal = root => fs.writeFileSync(path.join(root, '.craft/telemetry.json'), '{"local":false}');
const telemetryReport = (root, { session } = {}) => run(['telemetry-report', '--root', root, ...(session === undefined ? [] : ['--session', session])]);
const captureTelemetry = (root, { session } = {}) => run(['telemetry-capture', '--root', root, ...(session === undefined ? [] : ['--session', session])]);
const readTelemetryCapture = (root, id) => run(['telemetry-report', '--root', root, '--capture', id]);
const forgetTelemetryCapture = (root, id) => run(['telemetry-forget', '--root', root, '--capture', id]);

test('captured session report survives deletion of rolling logs and never implies complete history', t => {
  const root = fixture(t);
  observe(root); observe(root, 'other'); seal(root);
  const expected = telemetryReport(root, { session: 'PRIVATE_SESSION' });
  const captured = captureTelemetry(root, { session: 'PRIVATE_SESSION' });
  assert.equal(captured.unchanged, false);
  assert.equal(captureTelemetry(root, { session: 'PRIVATE_SESSION' }).unchanged, true);
  fs.unlinkSync(stateFile(root, 'telemetry/events.jsonl'));
  fs.unlinkSync(stateFile(root, 'telemetry/key.json'));
  const saved = readTelemetryCapture(root, captured.capture);
  assert.deepEqual(saved.report, expected);
  assert.equal(saved.report.retainedEvents, 1);
  assert.equal(saved.report.storage.completeHistory, false);
  assert.equal(saved.report.loss.totalLostEvents, null);
  assert.equal(JSON.stringify(saved).includes('PRIVATE_SESSION'), false);
  assert.equal(fs.readFileSync(file(root), 'utf8').includes('PRIVATE_SESSION'), false);
  assert.ok(fs.statSync(file(root)).size <= CAPTURE_MAX_BYTES);
});

test('capacity never evicts captures; explicit removal frees one slot without changing rolling evidence', t => {
  const root = fixture(t), captures = [];
  for (let i = 0; i < CAPTURE_MAX_ENTRIES; i++) observe(root, `session-${i}`);
  observe(root, 'next'); seal(root);
  for (let i = 0; i < CAPTURE_MAX_ENTRIES; i++) captures.push(captureTelemetry(root, { session: `session-${i}` }).capture);
  const before = fs.readFileSync(file(root), 'utf8');
  assert.throws(() => captureTelemetry(root, { session: 'next' }), /capacity reached/);
  assert.equal(fs.readFileSync(file(root), 'utf8'), before);
  for (const id of captures) assert.equal(readTelemetryCapture(root, id).report.retainedEvents, 1);
  const log = fs.readFileSync(stateFile(root, 'telemetry/events.jsonl'), 'utf8');
  assert.equal(forgetTelemetryCapture(root, captures[0]).remaining, CAPTURE_MAX_ENTRIES - 1);
  assert.equal(fs.readFileSync(stateFile(root, 'telemetry/events.jsonl'), 'utf8'), log);
  assert.throws(() => readTelemetryCapture(root, captures[0]), /not found/);
  assert.equal(captureTelemetry(root, { session: 'next' }).unchanged, false);
});

test('missing identity or invocation-only evidence never creates a capture store', t => {
  const root = fixture(t);
  fs.mkdirSync(path.join(root, '.craft')); seal(root);
  assert.throws(() => captureTelemetry(root), /requires/);
  assert.throws(() => captureTelemetry(root, { session: 'absent' }), /key is unavailable/);
  assert.deepEqual(fs.readdirSync(path.join(root, '.craft')), ['telemetry.json']);
  fs.rmSync(path.join(root, '.craft/telemetry.json'));
  recordEvent(root, 'host.hook'); seal(root);
  assert.throws(() => captureTelemetry(root, { session: 'absent' }), /no retained session/);
  assert.equal(fs.existsSync(file(root)), false);
  assert.throws(() => readTelemetryCapture(root, '../invalid'), /invalid/);
});

test('busy locks, symlinks and corrupt reports preserve the existing retained store', t => {
  const root = fixture(t); observe(root); seal(root);
  const saved = captureTelemetry(root, { session: 'PRIVATE_SESSION' });
  const before = fs.readFileSync(file(root), 'utf8');
  fs.writeFileSync(`${file(root)}.lock`, 'active');
  assert.throws(() => captureTelemetry(root, { session: 'PRIVATE_SESSION' }), /busy/);
  assert.throws(() => forgetTelemetryCapture(root, saved.capture), /busy/);
  assert.equal(fs.readFileSync(`${file(root)}.lock`, 'utf8'), 'active');
  fs.unlinkSync(`${file(root)}.lock`);
  // An extra report field is rejected even when the identifier matches the changed report.
  const corrupt = JSON.parse(before), entry = corrupt.captures[0];
  entry.report.secret = 'PRIVATE_RAW_BODY';
  entry.id = digest([entry.sessionId, entry.report]).slice(0, 24);
  fs.writeFileSync(file(root), JSON.stringify(corrupt));
  assert.throws(() => readTelemetryCapture(root, entry.id), /invalid retained/);
  assert.throws(() => captureTelemetry(root, { session: 'PRIVATE_SESSION' }), /invalid retained/);
  fs.writeFileSync(file(root), before);
  fs.renameSync(file(root), `${file(root)}.saved`);
  fs.symlinkSync(`${file(root)}.saved`, file(root));
  assert.throws(() => readTelemetryCapture(root, saved.capture), /invalid bounded state/);
  assert.equal(fs.readFileSync(`${file(root)}.saved`, 'utf8'), before);
  fs.unlinkSync(file(root));
  fs.writeFileSync(file(root), 'x'.repeat(CAPTURE_MAX_BYTES + 1));
  assert.throws(() => readTelemetryCapture(root, saved.capture), /invalid bounded state/);
});

test('concurrent processes preserve captures or explicitly report contention, without lost updates', async t => {
  const root = fixture(t);
  for (let i = 0; i < 4; i++) observe(root, `session-${i}`);
  seal(root);
  const results = await Promise.all(Array.from({ length: 4 }, (_, index) => new Promise((resolve, reject) => {
    const child = spawn(process.env.CRAFT_NATIVE_BINARY, ['telemetry-capture', '--root', root, '--session', `session-${index}`], { stdio: 'pipe' });
    let output = '', error = '';
    child.stdout.on('data', data => output += data); child.stderr.on('data', data => error += data);
    child.on('error', reject);
    child.on('close', status => !status ? resolve(JSON.parse(output)) : error.includes('busy') ? resolve({ busy: true }) : reject(new Error(error)));
  })));
  const stored = JSON.parse(fs.readFileSync(file(root), 'utf8'));
  assert.equal(stored.captures.length, results.filter(result => !result.busy).length);
  for (const result of results.filter(result => !result.busy)) assert.equal(readTelemetryCapture(root, result.capture).report.retainedEvents, 1);
});

test('a copied package launcher captures, recovers and explicitly forgets session evidence', t => {
  const root = fixture(t), exported = path.join(fixture(t), 'craft-delivery');
  fs.cpSync(path.resolve('dist/codex/packages/craft-delivery'), exported, { recursive: true });
  observe(root);
  const command = args => {
    const result = launch(exported, [...args, '--root', root]);
    assert.equal(result.status, 0, result.stderr);
    return JSON.parse(result.stdout);
  };
  const captured = command(['telemetry-capture', '--session', 'PRIVATE_SESSION']);
  fs.unlinkSync(stateFile(root, 'telemetry/events.jsonl'));
  assert.equal(command(['telemetry-report', '--capture', captured.capture]).report.retainedEvents, 1);
  assert.equal(command(['telemetry-forget', '--capture', captured.capture]).remaining, 0);
});
