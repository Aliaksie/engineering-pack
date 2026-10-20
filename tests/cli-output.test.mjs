import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { spawnSync } from 'node:child_process';
import { renderCommand } from '../cli/render.mjs';

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
    const result = spawnSync(process.execPath, ['cli/main.mjs', action, '--root', root, '--session', 'cli-session', '--preferences', path.join(root, 'missing-preferences.json'), ...args], { encoding: 'utf8' });
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
  const failure = spawnSync(process.execPath, ['cli/main.mjs', 'task-step', '--root', root, '--session', 'missing-session'], { encoding: 'utf8' });
  assert.equal(failure.status, 2);
  assert.equal(failure.stdout, '');
  assert.match(failure.stderr, /task-start binding/);
});

test('compact receipts preserve failed outcomes, storage diagnostics and action boundaries', () => {
  for (const outcome of ['quality-failure', 'permission-denied', 'tool-unavailable', 'blocked', 'prerequisite-resolved']) {
    const text = renderCommand('task-step', { recorded: { operation: 'verify', target: 'acceptance:scope', outcome },
      result: 'Actual prerequisite detail', checkpoint: { status: 'unavailable', reason: 'EPERM: state write denied' },
      complete: false, nextStep: null, status: { diagnostics: ['Guidance unavailable'], taskEvidenceStale: true } });
    assert.ok(text.includes(outcome));
    assert.match(text, /Actual prerequisite detail/);
    assert.match(text, /Checkpoint: unavailable.*EPERM/);
    assert.match(text, /Warning: Guidance unavailable/);
    assert.match(text, /evidence is stale/);
    assert.doesNotMatch(text, /Task: complete|Next:/);
  }
  const offer = { id: 'offer-1', action: 'review', scope: 'review:current-source', decision: 'pending', execution: 'not-observed' };
  const blocked = renderCommand('task-action-check', { offer, ready: false, reason: 'Approval is pending' });
  assert.match(blocked, /Ready: false/);
  assert.match(blocked, /Approval is pending/);
  assert.match(blocked, /not execution or a permission grant/);
  const ready = renderCommand('task-action-check', { offer: { ...offer, decision: 'approved' }, ready: true, reason: 'Current prerequisites present' });
  assert.match(ready, /Ready: true/);
  assert.match(ready, /Execution: not-observed/);
  assert.match(renderCommand('task-offer', { offer: null, applicability: { reason: 'No observed capacity' } }), /No observed capacity/);
  const suppressed = renderCommand('task-offer', { offer, suppressed: true });
  assert.match(suppressed, /unchanged; suppressed/);
  assert.match(suppressed, /No action dispatched/);
});

test('compact receipt commands preserve structured opt-ins and inspection commands keep their data', () => {
  const cases = {
    'checkpoint-save': { file: '/private/state/checkpoint.json', unchanged: true, state: { continuation: { nextStep: 'Resume here' } } },
    'session-attempt': { recorded: { operation: 'verify', target: 'acceptance:one', outcome: 'blocked' } },
    'task-decision': { offer: { id: 'offer-1', action: 'compact', scope: 'session:one', decision: 'declined', execution: 'not-observed' } }
  };
  for (const [action, result] of Object.entries(cases)) {
    assert.doesNotThrow(() => renderCommand(action, result));
    assert.deepEqual(JSON.parse(renderCommand(action, result, { json: true })), result);
    assert.deepEqual(JSON.parse(renderCommand(action, result, { verbose: true })), result);
  }
  const recovery = cases['checkpoint-save'];
  assert.deepEqual(JSON.parse(renderCommand('checkpoint-read', recovery)), recovery);
  assert.deepEqual(JSON.parse(renderCommand('task-step', { usage: 'task-step ...' }, { help: true })), { usage: 'task-step ...' });
});
