import fs from 'node:fs';
import path from 'node:path';
import os from 'node:os';
import { spawnSync } from 'node:child_process';
import test from 'node:test';
import assert from 'node:assert/strict';
import { sessionAdvice } from '../runtime/session.mjs';
import { sessionCapabilities, sessionObservation, renderSession } from '../adapters/shared/session.mjs';

const now = Date.parse('2026-10-18T12:00:00Z');
const observed = (currentTokens = 85) => ({ sessionId: 's1', usage: { kind: 'current-context', currentTokens,
  capacityTokens: 100, source: 'fixture-host', observedAt: new Date(now).toISOString(), sessionId: 's1' } });
const advice = (input = {}, project) => sessionAdvice(input, project, { now });
const project = { formatVersion: 1, modules: [{ name: 'app', path: 'app', checks: [{ command: 'consumer-check', args: ['--scope', 'literal; $(no-execution)'] }] }] };

test('fresh session occupancy drives compact/save/new advice; invalid or cumulative observations remain unknown', () => {
  assert.equal(advice(observed()).suggestion.action, 'compact');
  assert.equal(advice(observed(96)).suggestion.action, 'save-handoff');
  assert.equal(advice({ ...observed(96), task: { handoffSaved: true } }).suggestion.action, 'start-new-session');
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
  assert.equal(advice({ task: { ...task, goalComplete: true } }, project).suggestion, null);
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
  assert.equal(advice({ ...input, task: { revision: 'new' } }).suppressed, false);
  assert.equal(advice({ ...input, usage: { ...input.usage, currentTokens: 96 } }).suggestion.action, 'save-handoff');
  for (const policy of [{ compactAt: 1 }, { compactAt: '0.8' }, { maxAgeMs: -1 }, { cooldownMs: Infinity }]) assert.throws(() => advice({ policy }), /thresholds/);
  assert.throws(() => advice({ task: { files: ['../outside'] } }), /relative/);
  assert.throws(() => advice({ command: 'invented' }), /fields/);
});

test('Claude normalization includes current input/cache tokens, never cumulative or output totals', () => {
  const payload = { session_id: 's1', context_window: { context_window_size: 100,
    total_input_tokens: 999999, current_usage: { input_tokens: 30, cache_creation_input_tokens: 10, cache_read_input_tokens: 5, output_tokens: 1000 } } };
  const usage = sessionObservation(payload, 'claude', now);
  assert.equal(usage.currentTokens, 45);
  assert.equal(advice({ sessionId: 's1', usage }).usage.usedRatio, 0.45);
  assert.equal(sessionObservation(payload, 'codex', now), undefined);
  assert.equal(sessionObservation({ ...payload, context_window: { current_usage: null } }, 'claude', now), undefined);
  assert.equal(sessionObservation({ ...payload, context_window: { current_usage: { input_tokens: 40 } } }, 'claude', now), undefined);
  assert.match(renderSession(advice(observed()), 'codex'), /Next: \/compact/);
  assert.match(renderSession(advice(), 'agents'), /unknown/);
  assert.equal(sessionCapabilities('agents').commands.compact, undefined);
});

test('each export exposes the same portable advisor and honest adapter capability report after relocation', (t) => {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'session export '));
  t.after(() => fs.rmSync(root, { recursive: true, force: true }));
  const input = path.join(root, 'task.json');
  fs.writeFileSync(input, JSON.stringify({ task: { stage: 'explore', status: 'completed' } }));
  for (const host of ['claude', 'codex', 'cursor', 'copilot', 'gemini', 'agents', 'gemini-extension', 'jetbrains']) {
    const bundle = path.resolve('dist', host);
    const destination = path.join(root, host);
    fs.cpSync(path.join(bundle, host === 'claude' ? 'plugins' : 'packages', 'craft-delivery'), destination, { recursive: true });
    const cli = path.join(destination, 'support/session.mjs');
    const result = spawnSync(process.execPath, [cli, '--root', root, '--data', input, '--json'], { encoding: 'utf8' });
    assert.equal(result.status, 0, result.stderr);
    const output = JSON.parse(result.stdout);
    assert.equal(output.suggestion.action, 'summarize-evidence');
    assert.equal(output.usage.status, 'unknown');
    assert.deepEqual(output.host, sessionCapabilities(host));
    const report = JSON.parse(fs.readFileSync(path.join(bundle, 'adapter-report.json'), 'utf8')).packages.find((pkg) => pkg.package === 'craft-delivery');
    assert.equal(report.sessionAdvice.observation, output.host.observation);
    if (host === 'claude') {
      const payload = JSON.stringify({ session_id: 's1', context_window: { context_window_size: 100, current_usage: { input_tokens: 90, cache_creation_input_tokens: 0, cache_read_input_tokens: 0 } } });
      const native = spawnSync(process.execPath, [cli, '--root', root], { encoding: 'utf8', input: payload });
      assert.equal(native.status, 0, native.stderr);
      assert.match(native.stdout, /90\/100 tokens.*Next: \/compact/);
      const mismatch = spawnSync(process.execPath, [cli, '--root', root, '--data', input, '--native-stdin'], { encoding: 'utf8', input: payload });
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
