import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import http from 'node:http';
import os from 'node:os';
import path from 'node:path';
import { spawn } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import { telemetryEvent } from '../application/telemetry/events.mjs';
import { readTelemetry, telemetryIdentity } from '../platform/node/telemetry.mjs';
import { runtimeVersion, stateLocation } from '../platform/index.mjs';

// `settings` runs in the native runtime behind the JS command line; these tests use
// the public entry point exactly as an installed skill does.
const cli = fileURLToPath(new URL('../cli/main.mjs', import.meta.url));
function fixture(t) {
  const root = fs.realpathSync.native(fs.mkdtempSync(path.join(os.tmpdir(), 'craft-native-settings-')));
  t.after(() => fs.rmSync(root, { recursive: true, force: true }));
  const preferences = path.join(root, 'preferences.json');
  return { root, preferences, write: (file, value, mode = 0o600) => {
    fs.mkdirSync(path.dirname(path.join(root, file)), { recursive: true });
    fs.writeFileSync(path.join(root, file), typeof value === 'string' ? value : JSON.stringify(value), { mode });
  } };
}
function settings(root, args, env = {}) {
  return new Promise(resolve => {
    const child = spawn(process.execPath, [cli, 'settings', ...args], { cwd: root, env: { ...process.env, ...env } });
    let stdout = '', stderr = '';
    child.stdout.on('data', chunk => { stdout += chunk; });
    child.stderr.on('data', chunk => { stderr += chunk; });
    child.on('close', status => resolve({ status, stdout, stderr }));
  });
}

test('settings reports precedence, origins, private state placement and worker capacity', async t => {
  const { root, preferences, write } = fixture(t);
  write('.craft/project.json', { formatVersion: 1, routing: { editHints: false }, efficiency: { delegation: { maxWorkers: 2.0 } } });
  write('preferences.json', { routing: { dedupeHints: false } });
  write('task.json', { efficiency: { delegation: { maxWorkers: 1 } } });
  write('facts.json', { capacity: 4, occupiedSlots: 1, taskWorkers: 0, allowed: true, sessionId: 's1', observedAt: new Date(Date.now() - 1000).toISOString(), source: 'host:inventory' });
  const result = await settings(root, ['--root', '.', '--preferences', preferences, '--data', 'task.json', '--session', 's1', '--facts', 'facts.json']);
  assert.equal(result.status, 0, result.stderr);
  assert.equal(result.stderr, '');
  const report = JSON.parse(result.stdout);
  assert.equal(result.stdout, `${JSON.stringify(report, null, 2)}\n`);
  assert.deepEqual(report.settings.routing, { editHints: false, dedupeHints: false });
  assert.equal(report.settings.efficiency.delegation.maxWorkers, 1);
  assert.deepEqual([report.origins['routing.editHints'], report.origins['routing.dedupeHints'], report.origins['efficiency.delegation.maxWorkers']], ['project', 'user', 'task']);
  assert.deepEqual(report.state, stateLocation(root));
  assert.deepEqual([report.workers.effectiveLimit, report.workers.available, report.workers.reason], [1, 1, 'observed']);
  assert.equal(fs.existsSync(path.join(root, '.craft', 'state', report.state.worktreeId, 'shared', 'telemetry.jsonl')), true);
});

test('settings rejects unsafe preferences and invalid input with the CLI error contract', async t => {
  const { root, preferences, write } = fixture(t);
  write('preferences.json', ' '.repeat(65537));
  for (const [args, pattern] of [
    [['--preferences', preferences], /bounded/],
    [['--data', 'missing.json'], /./],
    [['--unknown'], /unknown argument: --unknown/],
    [['--repository', '.'], /--repository is supported by task-start/],
    [['--root', '.', '--root', '.'], /duplicate option --root/]
  ]) {
    const result = await settings(root, args);
    assert.equal(result.status, 2, args.join(' '));
    assert.equal(result.stdout, '');
    assert.match(result.stderr, pattern);
  }
  if (process.platform !== 'win32') {
    write('preferences.json', '{}', 0o666);
    fs.chmodSync(preferences, 0o666);
    assert.match((await settings(root, ['--preferences', preferences])).stderr, /writable/);
    fs.rmSync(preferences); fs.symlinkSync(path.join(root, 'task.json'), preferences);
    assert.match((await settings(root, ['--preferences', preferences])).stderr, /symlink/);
  }
  const help = await settings(root, ['--unknown', '--help']);
  assert.equal(help.status, 0);
  assert.match(JSON.parse(help.stdout).usage, /^settings /);
});

test('native telemetry uses the shared store, schema and session pseudonym', async t => {
  const { root, preferences } = fixture(t);
  assert.equal((await settings(root, ['--root', '.', '--preferences', preferences, '--session', 'private-session', '--host', 'codex'])).status, 0);
  assert.equal((await settings(root, ['--root', '.', '--absent'])).status, 2);
  const [success, failure] = readTelemetry(root);
  for (const event of [success, failure]) assert.deepEqual(telemetryEvent(event), event);
  assert.deepEqual([success.name, success.outcome, success.attributes, success.host, success.packVersion],
    ['cli.command', 'success', { command: 'settings', identityScope: 'session' }, 'codex', runtimeVersion()]);
  assert.equal(success.sessionId, telemetryIdentity(root, 'private-session'));
  assert.deepEqual([failure.outcome, failure.reasonCode, failure.attributes], ['failure', 'INVALID_INPUT', { identityScope: 'invocation' }]);
  assert.equal(JSON.stringify(readTelemetry(root)).includes('private-session'), false);
});

test('remote export of native events runs through the forwarder', async t => {
  const { root, preferences, write } = fixture(t);
  const spans = [];
  const server = http.createServer((request, response) => {
    let body = '';
    request.on('data', chunk => { body += chunk; });
    request.on('end', () => { spans.push(...JSON.parse(body).resourceSpans.map(entry => entry.scopeSpans[0].spans[0])); response.end('{}'); });
  });
  await new Promise(resolve => server.listen(0, '127.0.0.1', resolve));
  t.after(() => server.close());
  write('.craft/telemetry.json', { remote: { enabled: true, endpoint: `http://127.0.0.1:${server.address().port}/v1/traces` } });
  write('task.json', {});
  assert.equal((await settings(root, ['--root', '.', '--preferences', preferences, '--session', 's1', '--data', 'task.json'])).status, 0);
  assert.deepEqual(spans.map(span => span.name), ['input.loaded', 'cli.command']);
  const events = readTelemetry(root);
  assert.deepEqual(events.map(event => event.name), ['input.loaded', 'cli.command', 'telemetry.export']);
  const exported = events[2];
  assert.deepEqual([exported.outcome, exported.attributes.exportedEvents, exported.sessionId, exported.traceId, exported.parentOperationId],
    ['success', 2, events[1].sessionId, events[1].traceId, events[1].operationId]);
  assert.equal(spans[1].traceId, events[1].traceId);
});

test('an unavailable native runtime fails the command explicitly', async t => {
  const { root } = fixture(t);
  const result = await settings(root, [], { CRAFT_NATIVE_BINARY: path.join(root, 'missing', 'craft') });
  assert.equal(result.status, 2);
  assert.equal(result.stdout, '');
  assert.match(result.stderr, /Native runtime unavailable/);
});
