import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import http from 'node:http';
import { spawn } from 'node:child_process';
import { run } from './craft.mjs';
import { packVersion, readTelemetry, stateFile } from './state.mjs';

// The local recorder as `craft` commands use it. Envelope sanitizing, rotation, bounds, lock
// waits and OTLP mapping: `cargo test -p craft-engine telemetry`, `-p craft-platform telemetry`
// and the `otlp_contracts` replay.
function fixture(t, config) {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'pack-telemetry-'));
  t.after(() => fs.rmSync(root, { recursive: true, force: true }));
  if (config !== undefined) { fs.mkdirSync(path.join(root, '.craft')); fs.writeFileSync(path.join(root, '.craft/telemetry.json'), JSON.stringify(config)); }
  return root;
}
// Asynchronous, so a local collector in this process can answer the export.
const craft = args => new Promise((resolve, reject) => {
  const child = spawn(process.env.CRAFT_NATIVE_BINARY, args, { stdio: 'pipe' });
  let stdout = '', stderr = '';
  child.stdout.on('data', data => { stdout += data; }); child.stderr.on('data', data => { stderr += data; });
  child.on('error', reject); child.on('close', status => resolve({ status, stdout, stderr }));
});

test('local default records version and project-scoped session correlation without session material', t => {
  const root = fixture(t), other = fixture(t);
  run(['settings', '--root', root, '--session', 'private-session', '--host', 'codex']);
  run(['settings', '--root', root, '--session', 'private-session']);
  run(['settings', '--root', other, '--session', 'private-session']);
  const events = readTelemetry(root);
  assert.equal(events.length, 2);
  assert.equal(events[0].packVersion, packVersion());
  assert.equal(events[0].sessionId, events[1].sessionId);
  assert.notEqual(readTelemetry(other)[0].sessionId, events[0].sessionId);
  assert.equal(JSON.stringify(events).includes('private-session'), false);
  assert.equal(events[0].attributes.identityScope, 'session');
  run(['settings', '--root', root]); run(['settings', '--root', root]);
  const invocations = readTelemetry(root).slice(-2);
  assert.equal(invocations[0].attributes.identityScope, 'invocation');
  assert.notEqual(invocations[0].sessionId, invocations[1].sessionId);
});

test('disabled and malformed configuration never write events or contact a collector', t => {
  for (const config of [{ local: false }, { local: 'secret' }, { remote: { enabled: true, endpoint: 'https://user:secret@example.test' } },
    { remote: { enabled: true, endpoint: 'http://remote.test' } }]) {
    const root = fixture(t, config);
    run(['route', '--root', root, '--intent', 'implement']);
    if (config.local === false) assert.equal(fs.existsSync(path.join(root, '.craft/state')), false);
    else assert.ok(readTelemetry(root).some(event => event.reasonCode === 'TELEMETRY_CONFIG_INVALID'), JSON.stringify(config));
    assert.equal(readTelemetry(root).some(event => event.name === 'telemetry.export'), false);
  }
});

test('unavailable storage preserves command results', t => {
  const root = fixture(t);
  fs.mkdirSync(path.join(root, '.craft')); fs.writeFileSync(path.join(root, '.craft/state'), 'blocked');
  assert.equal(run(['route', '--root', root, '--intent', 'implement']).intent, 'implement');
  assert.equal(fs.readFileSync(path.join(root, '.craft/state'), 'utf8'), 'blocked');
});

test('concurrent processes append complete JSONL events within the shared bound', async t => {
  const root = fixture(t);
  const results = await Promise.all(Array.from({ length: 8 }, () => craft(['route', '--root', root, '--intent', 'implement'])));
  for (const result of results) assert.equal(result.status, 0, result.stderr);
  const lines = fs.readFileSync(stateFile(root, 'telemetry/events.jsonl'), 'utf8').trim().split('\n');
  assert.ok(lines.length > 0);
  for (const line of lines) assert.equal(JSON.parse(line).schemaVersion, 1);
});

test('real routing and session commands emit sanitized observations and keep occupancy unknown', t => {
  const root = fixture(t);
  const routed = run(['route', '--root', root, '--intent', 'implement', '--host', 'codex', '--session', 'session', '--task', 'task']);
  assert.ok(routed.skills.length);
  const session = run(['session', '--root', root, '--host', 'codex', '--session', 'session', '--task', 'task']);
  assert.equal(session.usage.status, 'unknown'); assert.equal(session.packVersion, packVersion());
  const events = readTelemetry(root);
  assert.ok(events.some(event => event.name === 'routing.completed'));
  assert.ok(events.some(event => event.name === 'guidance.selected' && event.attributes.skill));
  assert.ok(events.some(event => event.name === 'host.observed' && event.attributes.occupancyKnown === false));
});

test('opt-in export reaches a local collector; rejection and timeouts stay advisory', async t => {
  const bodies = [];
  const server = http.createServer((request, response) => {
    if (request.url === '/timeout') return;
    const chunks = []; request.on('data', data => chunks.push(data));
    request.on('end', () => { bodies.push(JSON.parse(Buffer.concat(chunks))); response.writeHead(request.url === '/reject' ? 500 : 200, { 'content-type': 'application/json' }); response.end('{}'); });
  });
  await new Promise(resolve => server.listen(0, '127.0.0.1', resolve));
  t.after(() => { server.closeAllConnections(); server.close(); });
  const base = `http://127.0.0.1:${server.address().port}`;
  const exported = async endpoint => {
    const root = fixture(t, { remote: { enabled: true, endpoint: `${base}${endpoint}` } });
    const result = await craft(['route', '--root', root, '--intent', 'implement', '--session', 'private']);
    assert.equal(result.status, 0, result.stderr);
    return readTelemetry(root).find(event => event.name === 'telemetry.export');
  };
  assert.equal((await exported('/v1/traces')).outcome, 'success');
  assert.equal(bodies.length, 1);
  assert.equal(JSON.stringify(bodies).includes('private'), false);
  assert.equal((await exported('/reject')).outcome, 'failure');
  const timeout = await exported('/timeout');
  assert.deepEqual([timeout.outcome, timeout.reasonCode], ['failure', 'EXPORT_TIMEOUT']);
});
