import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';

const repository = fileURLToPath(new URL('../', import.meta.url));

test('routine CLI output is compact while JSON and verbose retain full acceptance and guidance evidence', t => {
  const root = fs.realpathSync(fs.mkdtempSync(path.join(os.tmpdir(), 'craft-cli-output-')));
  t.after(() => fs.rmSync(root, { recursive: true, force: true }));
  const git = (...args) => {
    const result = spawnSync('git', ['-C', root, '-c', 'core.hooksPath=/dev/null', '-c', 'commit.gpgsign=false', '-c', 'user.name=Fixture', '-c', 'user.email=fixture@example.test', ...args], { encoding: 'utf8' });
    assert.equal(result.status, 0, result.stderr);
  };
  git('init', '-q'); fs.writeFileSync(path.join(root, 'value.mjs'), 'export const value = 1;\n');
  git('add', '.'); git('commit', '-qm', 'fixture');
  const invoke = (action, ...args) => {
    const result = spawnSync(process.env.CRAFT_NATIVE_BINARY, [action, '--root', root, '--session', 'cli-session', '--preferences', path.join(root, 'missing-preferences.json'), ...args],
      { encoding: 'utf8', env: { ...process.env, CRAFT_PACKAGE_ROOT: repository } });
    assert.equal(result.status, 0, result.stderr);
    return result.stdout;
  };
  const started = invoke('task-start', '--task', 'T1', '--goal', 'Correct the value', '--file', 'value.mjs');
  assert.match(started, /Task bound: T1/);
  assert.match(started, /Skills: .*craft-delivery:apply-change/);
  const step = ['--stage', 'verify', '--target', 'acceptance:value', '--approach', 'manual-check', '--evidence', 'checks:value', '--outcome', 'passed', '--result', 'Value verified', '--next', 'Return the result'];
  const compact = invoke('task-step', ...step);
  assert.match(compact, /acceptance:value — passed/);
  assert.match(compact, /Checkpoint: saved/);
  assert.match(compact, /Next: /);
  assert.doesNotMatch(compact, /packReport|fingerprints|featureOutcomes|sha256|allClassified/);
  const json = invoke('task-step', ...step, '--json');
  const verbose = invoke('task-step', ...step, '--verbose');
  for (const text of [json, verbose]) {
    const full = JSON.parse(text);
    assert.equal(full.recorded.evidence, 'checks:value');
    assert.equal(full.checkpoint.state.continuation.currentState, 'Value verified');
    assert.ok(full.status.packReport.files.length > 0);
    assert.equal(full.status.featureOutcomes.features.review.status, 'unknown');
    assert.equal(full.status.featureOutcomes.features.delegation.status, 'unknown');
  }
  assert.ok(compact.length < json.length / 4, `compact ${compact.length} bytes vs JSON ${json.length}`);
  t.diagnostic(`task-step fixture: ${compact.length} characters compact vs ${json.length} in full JSON`);
  const failure = spawnSync(process.env.CRAFT_NATIVE_BINARY, ['task-step', '--root', root, '--session', 'missing-session'], { encoding: 'utf8' });
  assert.equal(failure.status, 2);
  assert.equal(failure.stdout, '');
  assert.match(failure.stderr, /task-start binding/);
});

test('compact receipt commands keep structured opt-ins and inspection commands keep their data', t => {
  const root = fs.realpathSync(fs.mkdtempSync(path.join(os.tmpdir(), 'craft-cli-detail-')));
  t.after(() => fs.rmSync(root, { recursive: true, force: true }));
  const craft = (...args) => {
    const result = spawnSync(process.env.CRAFT_NATIVE_BINARY, [...args, '--root', root, '--session', 's1'], { encoding: 'utf8' });
    assert.equal(result.status, 0, result.stderr);
    return result.stdout;
  };
  assert.match(craft('checkpoint-save'), /^Checkpoint: saved/);
  for (const detail of ['--json', '--verbose']) assert.equal(JSON.parse(craft('checkpoint-save', detail)).state.formatVersion, 1);
  // Inspection prints its data whatever the detail requested.
  assert.equal(JSON.parse(craft('checkpoint-read')).saved.formatVersion, 1);
  assert.deepEqual(Object.keys(JSON.parse(craft('task-step', '--help'))), ['usage', 'mode', 'output']);
  // Search results stay on one line whatever the detail requested.
  fs.writeFileSync(path.join(root, 'needle.txt'), 'needle\n');
  for (const detail of [[], ['--json'], ['--verbose']]) {
    const text = craft('locate', '--term', 'needle', '--path', '.', ...detail);
    assert.equal(text.indexOf('\n'), text.length - 1);
    assert.equal(JSON.parse(text).matches[0].path, 'needle.txt');
  }
});
