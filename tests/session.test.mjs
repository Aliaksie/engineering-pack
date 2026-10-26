import fs from 'node:fs';
import path from 'node:path';
import os from 'node:os';
import { spawnSync } from 'node:child_process';
import test from 'node:test';
import assert from 'node:assert/strict';
import { decide } from '../application/engine-client.mjs';
import { catalog } from '../application/routing/catalog.mjs';
import { readTelemetry } from '../platform/index.mjs';

// The CLI reaches session advice through sessionStatus; these cases pin the decision itself.
const sessionAdvice = (input, project, options = {}) => decide('session-advice', { input, project, catalog, options: { now: Date.now(), ...options } });
const now = Date.parse('2026-10-18T12:00:00Z');
const observed = (currentTokens = 85) => ({ sessionId: 's1', host: { reliableCompaction: true }, task: { safeBoundary: true, handoffSaved: true, checkpointFresh: true }, usage: { kind: 'current-context', currentTokens,
  capacityTokens: 100, source: 'fixture-host', observedAt: new Date(now).toISOString(), sessionId: 's1' } });
const advice = (input = {}, project) => sessionAdvice(input, project, { now });
const project = { formatVersion: 1, modules: [{ name: 'app', path: 'app', checks: [{ command: 'consumer-check', args: ['--scope', 'literal; $(no-execution)'] }] }] };

test('fresh session occupancy drives compact/save/new advice; invalid or cumulative observations remain unknown', () => {
  assert.equal(advice(observed()).suggestion.action, 'compact');
  assert.equal(advice({ ...observed(96), task: { safeBoundary: true, phaseChanged: true } }).suggestion.action, 'save-handoff');
  assert.equal(advice({ ...observed(96), task: { safeBoundary: true, phaseChanged: true, handoffSaved: true, checkpointFresh: true } }).suggestion.action, 'start-new-session');
  assert.equal(advice(observed(20)).suggestion, null);
  for (const change of [
    { kind: 'cumulative' }, { observedAt: '2026-10-17T12:00:00Z' },
    { observedAt: '2026-10-19T12:00:00Z' }, { source: '' }, { sessionId: 'other' },
    { currentTokens: -1 }, { currentTokens: 101 }, { currentTokens: null }, { capacityTokens: 0 }
  ]) {
    const input = observed(); Object.assign(input.usage, change);
    assert.equal(advice(input).usage.status, 'unknown', JSON.stringify(change));
    assert.equal(advice(input).suggestion, null);
  }
  assert.equal(advice().usage.currentTokens, null);
});

test('next commands require explicit milestones and preserve exact declared argv without execution', () => {
  for (const status of ['working', 'unknown', undefined]) assert.equal(advice({ task: { stage: 'implement', status } }, project).suggestion, null);
  const task = { id: 'T1', stage: 'implement', status: 'completed', files: ['app/main.py'], revision: 'r2' };
  const next = advice({ task }, project);
  assert.equal(next.suggestion.action, 'run-check');
  assert.deepEqual(next.suggestion.checks, [{ module: 'app', cwd: 'app', command: 'consumer-check', args: ['--scope', 'literal; $(no-execution)'], execution: 'not-run' }]);
  assert.equal(advice({ task }).suggestion.action, 'design-verification');
  assert.equal(advice({ task: { ...task, verification: 'passed', verificationRevision: 'r1', reviewRequested: true } }, project).suggestion.action, 'run-check');
  assert.equal(advice({ task: { ...task, verification: 'passed', verificationRevision: 'r2', reviewRequested: true } }, project).suggestion.action, 'review');
  assert.equal(advice({ task: { ...task, verification: 'passed', verificationRevision: 'r2' } }, project).suggestion.action, 'summarize-evidence');
  assert.equal(advice({ task: { ...task, goalComplete: true } }, project).suggestion.action, 'summarize-evidence');
  assert.equal(advice({ task: { status: 'failed' } }).suggestion.action, 'inspect-failure');
  assert.equal(advice({ task: { status: 'blocked' } }).suggestion.action, 'resolve-blocker');
  for (const stage of ['explore', 'story', 'architecture']) assert.equal(advice({ task: { stage, status: 'completed' } }).suggestion.action, 'summarize-evidence');
  assert.equal(advice({ task: { stage: 'plan', status: 'completed' } }).suggestion, null, 'do not infer authorization for implementation');
});

test('notice suppression refreshes after cooldown, revision or action changes and validates thresholds', () => {
  const first = advice(observed());
  const input = { ...observed(), previous: first.notice };
  assert.equal(advice(input).suppressed, true);
  assert.equal(advice(input).suggestion, null);
  assert.deepEqual(advice(input).notice, first.notice, 'suppression must not extend cooldown indefinitely');
  assert.equal(sessionAdvice({ ...input, usage: { ...input.usage, observedAt: new Date(now + 300001).toISOString() } }, undefined, { now: now + 300001 }).suppressed, false);
  assert.equal(advice({ ...input, task: { ...input.task, revision: 'new' } }).suppressed, false);
  assert.equal(advice({ ...input, task: { safeBoundary: true, phaseChanged: true }, usage: { ...input.usage, currentTokens: 96 } }).suggestion.action, 'save-handoff');
  for (const policy of [{ elevatedAt: '0.6' }, { compactAt: 1 }, { compactAt: '0.8' }, { maxAgeMs: -1 }, { cooldownMs: Infinity }]) assert.throws(() => advice({ policy }), /thresholds/);
  assert.throws(() => advice({ task: { files: ['../outside'] } }), /relative/);
  assert.throws(() => advice({ command: 'invented' }), /fields/);
});

const sessionHosts = JSON.parse(fs.readFileSync(new URL('../hosts/session.json', import.meta.url), 'utf8'));
// The reported status entry as an installed package runs it: the package launcher, no Node.
function statusline(packageRoot, { entry: launcher, args: fixed }, args, input = '') {
  const options = { encoding: 'utf8', input };
  if (process.platform !== 'win32') return spawnSync(path.join(packageRoot, launcher), [...fixed, ...args], options);
  // Node starts .cmd files only through a shell, which takes the command line as written.
  const line = [path.join(packageRoot, `${launcher}.cmd`), ...fixed, ...args].map(arg => `"${arg}"`).join(' ');
  return spawnSync(line, { ...options, shell: true });
}

test('each export runs its reported status entry after relocation, with honest adapter capabilities', (t) => {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'session export '));
  t.after(() => fs.rmSync(root, { recursive: true, force: true }));
  const input = path.join(root, 'task.json');
  fs.writeFileSync(input, JSON.stringify({ task: { stage: 'explore', status: 'completed' } }));
  for (const host of ['claude', 'codex', 'cursor', 'copilot', 'gemini', 'agents', 'gemini-extension', 'jetbrains']) {
    const bundle = path.resolve('dist', host);
    const destination = path.join(root, host);
    fs.cpSync(path.join(bundle, host === 'claude' ? 'plugins' : 'packages', 'craft-delivery'), destination, { recursive: true });
    const report = JSON.parse(fs.readFileSync(path.join(bundle, 'adapter-report.json'), 'utf8')).packages.find((pkg) => pkg.package === 'craft-delivery');
    assert.deepEqual([report.sessionAdvice.entry, ...report.sessionAdvice.args], ['bin/craft', 'statusline', host, '--package', 'craft-delivery']);
    const run = (args, stdin) => statusline(destination, report.sessionAdvice, args, stdin);
    const result = run(['--root', root, '--data', input, '--json']);
    assert.equal(result.status, 0, result.stderr);
    const output = JSON.parse(result.stdout);
    assert.equal(output.suggestion.action, 'summarize-evidence');
    assert.equal(output.usage.status, 'unknown');
    assert.deepEqual(output.host, sessionHosts[host]);
    assert.equal(report.sessionAdvice.observation, output.host.observation);
    // An explicit read without a host payload reports unknown usage on every host.
    const explicit = run(['--root', root]);
    assert.equal(explicit.status, 0, explicit.stderr);
    assert.equal(explicit.stdout, 'Context usage unknown\n');
    if (host === 'claude') {
      const payload = JSON.stringify({ session_id: 's1', model: { id: 'fixture-native-model' }, context_window: { context_window_size: 100, current_usage: { input_tokens: 90, cache_creation_input_tokens: 0, cache_read_input_tokens: 0 } } });
      const native = run(['--root', root], payload);
      assert.equal(native.status, 0, native.stderr);
      assert.match(native.stdout, /90\/100 tokens.*Next: continue-bounded-step/);
      const nativeJson = run(['--root', root, '--json'], payload);
      assert.equal(nativeJson.status, 0, nativeJson.stderr);
      const nativeStatus = JSON.parse(nativeJson.stdout);
      assert.equal(nativeStatus.sessionHost.model, 'fixture-native-model');
      assert.equal(nativeStatus.sessionHost.name, 'claude');
      assert.equal(nativeStatus.usage.confidence, 'observed');
      const events = readTelemetry(root).filter(event => event.name === 'session.observed' && event.host === 'claude');
      assert.ok(events.length > 0);
      assert.ok(events.every(event => event.attributes.package === 'craft-delivery'));
      const mismatch = run(['--root', root, '--data', input, '--native-stdin'], payload);
      assert.equal(mismatch.status, 2);
      assert.match(mismatch.stderr, /does not identify this native session/);
    }
  }
});

test('manual verification preserves observation scope when automation exists', () => {
  const project = { formatVersion: 1, modules: [{ name: 'app', path: 'src', checks: [{ command: 'node', args: ['--test'] }] }] };
  const result = sessionAdvice({ task: { stage: 'verify', status: 'completed', verificationMode: 'manual', files: ['src/app.mjs'] } }, project);
  assert.equal(result.suggestion.action, 'design-verification');
  assert.equal(result.suggestion.checks, undefined);
});

test('pressure defaults respect boundaries, reliable controls, phase changes and checkpoint freshness', () => {
  for (const [tokens, pressure] of [[59, 'healthy'], [60, 'elevated'], [75, 'high'], [85, 'saturated']]) {
    assert.equal(advice(observed(tokens)).pressure, pressure);
  }
  assert.equal(advice({ ...observed(99), task: { status: 'working' } }).suggestion.action, 'continue-bounded-step');
  assert.equal(advice({ ...observed(99), task: { ...observed().task, almostComplete: true } }).suggestion.action, 'continue-bounded-step');
  assert.equal(advice({ ...observed(80), task: { safeBoundary: true } }).suggestion.afterCheckpoint, 'compact');
  assert.equal(advice({ ...observed(99), host: {}, task: { safeBoundary: true, handoffSaved: true } }).suggestion.action, 'save-handoff');
  assert.equal(advice({ ...observed(99), task: { ...observed().task, compacted: true }, activity: { repeatedLookups: 3 } }).suggestion.action, 'start-new-session');
  assert.equal(advice({ ...observed(80), host: {} }).suggestion.action, 'start-new-session');
  assert.equal(advice({ task: { safeBoundary: true, phaseChanged: true } }).suggestion, null, 'phase or session age alone does not justify reset');
});

test('execution budget stops loops and preserves unknown activity and model fields', () => {
  assert.equal(advice({ activity: { noProgressAttempts: 1 } }).suggestion.action, 'retry-differently');
  assert.equal(advice({ activity: { noProgressAttempts: 3 } }).suggestion.action, 'return-control');
  assert.equal(advice({ ...observed(99), activity: { attempts: 2, maxAttempts: 2 } }).suggestion.action, 'return-control');
  assert.equal(advice().activity.attempts, null);
  assert.equal(advice().host.model, 'unknown');
  assert.equal(advice().packContext.estimatedTokens, null);
  for (const activity of [{ attempts: -1 }, { maxAttempts: 0 }, { retries: 1.5 }, { providerTokens: 100 }]) assert.throws(() => advice({ activity }));
});


test('unified CLI and display preserve unknown fields and recommendations under notice suppression', (t) => {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'session-status-'));
  t.after(() => fs.rmSync(root, { recursive: true, force: true }));
  const result = spawnSync(process.env.CRAFT_NATIVE_BINARY, ['session', '--root', root, '--bundle', path.resolve('.')], { encoding: 'utf8' });
  assert.equal(result.status, 0, result.stderr);
  assert.match(result.stdout, /Host: unknown/);
  assert.match(result.stdout, /Model: unknown/);
  assert.match(result.stdout, /Attempts: unknown\/unknown/);
  const first = advice(observed());
  const suppressed = advice({ ...observed(), previous: first.notice });
  assert.equal(suppressed.suggestion, null);
  assert.equal(suppressed.recommendation.action, 'compact');
  assert.equal(advice({ task: { goalComplete: true, verification: 'failed' } }).suggestion.action, 'inspect-failure');
});
