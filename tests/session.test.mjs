import fs from 'node:fs';
import path from 'node:path';
import os from 'node:os';
import { spawnSync } from 'node:child_process';
import test from 'node:test';
import assert from 'node:assert/strict';
import { readTelemetry } from './state.mjs';

// Session advice decisions are pinned by `cargo test -p craft-engine --test session_advice`.
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
    const { contextLimits, ...reported } = sessionHosts[host];
    assert.deepEqual(output.host, reported);
    assert.deepEqual(report.sessionAdvice.contextLimits, contextLimits);
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

test('unified CLI display preserves unknown host, model and attempt fields', (t) => {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'session-status-'));
  t.after(() => fs.rmSync(root, { recursive: true, force: true }));
  const result = spawnSync(process.env.CRAFT_NATIVE_BINARY, ['session', '--root', root, '--bundle', path.resolve('.')], { encoding: 'utf8' });
  assert.equal(result.status, 0, result.stderr);
  assert.match(result.stdout, /Host: unknown/);
  assert.match(result.stdout, /Model: unknown/);
  assert.match(result.stdout, /Attempts: unknown\/unknown/);
});
