import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { run } from './craft.mjs';
import { runHook } from './hook-command.mjs';
import { readTelemetry } from './state.mjs';

// Package attribution as installed hooks record it; export queue loss: `cargo test -p
// craft-application a_full_export_queue`.
const report = root => run(['telemetry-report', '--root', root]);
function fixture(t) {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'craft-attribution-'));
  t.after(() => fs.rmSync(root, { recursive: true, force: true }));
  return root;
}
const hook = (root, pkg, input) => runHook(path.resolve(`dist/codex/packages/${pkg}`), 'codex', 'observe-before-tool', { cwd: root, input });

test('each package hook attributes its own events and keeps one session pseudonym', t => {
  const root = fixture(t), input = JSON.stringify({ session_id: 'PRIVATE_SESSION', cwd: root, tool_name: 'Read' });
  for (const pkg of ['craft-engineering', 'craft-review']) {
    const result = hook(root, pkg, input);
    assert.equal(result.status, 0, result.stderr);
  }
  const events = readTelemetry(root);
  assert.deepEqual([...new Set(events.map(event => event.attributes.package))].sort(), ['craft-engineering', 'craft-review']);
  assert.equal(new Set(events.map(event => event.sessionId)).size, 1);
  assert.equal(JSON.stringify(events).includes('PRIVATE_'), false);
  const counts = report(root).attribution.packages;
  assert.ok(counts['craft-engineering'] > 0 && counts['craft-review'] > 0, JSON.stringify(counts));
});

test('a valid JSON prefix followed by oversized whitespace never claims a session from partial input', t => {
  const root = fixture(t);
  const result = hook(root, 'craft-engineering', JSON.stringify({ session_id: 'PRIVATE_SESSION', cwd: root }) + ' '.repeat(70000));
  assert.equal(result.status, 0, result.stderr);
  assert.equal(result.stdout, ''); assert.equal(result.stderr, '');
  const events = readTelemetry(root);
  assert.ok(events.some(event => event.reasonCode === 'INPUT_TOO_LARGE'));
  assert.ok(events.every(event => event.attributes.identityScope === 'invocation'));
  assert.ok(events.every(event => event.attributes.package === 'craft-engineering'));
  assert.equal(report(root).attribution.hookInputs.oversized, 1);
});
