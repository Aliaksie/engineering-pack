import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { run } from './craft.mjs';

// Effective settings as `craft settings` reports them: project file, private preferences and
// task overrides (`--data`). Unsafe preference files: native-settings.test.mjs; contradictory
// worker counts and binding identity types: `cargo test -p craft-engine contract_corrections`.
function fixture(t, { project, preferences, overrides } = {}) {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'craft-settings-'));
  t.after(() => fs.rmSync(root, { recursive: true, force: true }));
  const preferencesFile = path.join(root, 'preferences.json'), args = ['settings', '--root', root, '--preferences', preferencesFile];
  if (project) { fs.mkdirSync(path.join(root, '.craft')); fs.writeFileSync(path.join(root, '.craft/project.json'), JSON.stringify(project)); }
  if (preferences) fs.writeFileSync(preferencesFile, JSON.stringify(preferences), { mode: 0o600 });
  if (overrides) { fs.writeFileSync(path.join(root, 'task.json'), JSON.stringify(overrides)); args.push('--data', path.join(root, 'task.json')); }
  return { root, preferencesFile, resolve: () => run(args) };
}

test('nested precedence retains false, zero and sibling defaults', t => {
  const { resolve } = fixture(t, { project: { formatVersion: 1, routing: { editHints: false } },
    preferences: { efficiency: { delegation: { maxWorkers: 1 } } }, overrides: { efficiency: { delegation: { maxWorkers: 0 } } } });
  const result = resolve();
  assert.equal(result.settings.routing.editHints, false);
  assert.equal(result.settings.routing.dedupeHints, true);
  assert.equal(result.settings.efficiency.delegation.maxWorkers, 0);
  assert.equal(result.settings.efficiency.delegation.mode, 'recommend');
  assert.equal(result.origins['efficiency.delegation.maxWorkers'], 'task');
  assert.equal(result.origins['routing.editHints'], 'project');
});

test('invalid limits, unknown fields, permission preferences and thresholds fail', t => {
  for (const maxWorkers of [-1, 1.5, '3', null, Number.MAX_SAFE_INTEGER + 1]) {
    assert.throws(() => fixture(t, { overrides: { efficiency: { delegation: { maxWorkers } } } }).resolve(), /maxWorkers/, String(maxWorkers));
  }
  assert.throws(() => fixture(t, { preferences: { permissions: { writeOperations: ['git.push'] } } }).resolve(), /unknown/);
  assert.throws(() => fixture(t, { overrides: { efficiency: { context: { compactAt: 0.9 } } } }).resolve(), /thresholds/);
  assert.throws(() => fixture(t, { project: { formatVersion: 1, efficiency: { unknown: true } } }).resolve(), /unknown/);
  assert.throws(() => fixture(t, { overrides: { bogus: 1 } }).resolve(), /unknown overrides field: bogus/);
});

test('settings read private preferences and create no repository config', t => {
  const { root, preferencesFile, resolve } = fixture(t);
  assert.equal(resolve().settings.efficiency.delegation.maxWorkers, 3);
  fs.writeFileSync(preferencesFile, JSON.stringify({ routing: { editHints: false } }), { mode: 0o600 });
  assert.equal(resolve().settings.routing.editHints, false);
  assert.equal(resolve().origins['routing.editHints'], 'user');
  assert.equal(fs.existsSync(path.join(root, '.craft', 'project.json')), false);
});
