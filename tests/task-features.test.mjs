import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { spawnSync } from 'node:child_process';
import { launch, run } from './craft.mjs';

// Outcome record decisions are pinned by `cargo test -p craft-engine --test feature_outcomes`.
test('normal task commands retain outcomes and mark changed sources stale without changing the recorded verdict', t => {
  const root = fs.realpathSync(fs.mkdtempSync(path.join(os.tmpdir(), 'craft-feature-outcomes-')));
  t.after(() => fs.rmSync(root, { recursive: true, force: true }));
  const git = (...args) => {
    const result = spawnSync('git', ['-C', root, '-c', 'core.hooksPath=/dev/null', '-c', 'commit.gpgsign=false', '-c', 'user.name=Fixture', '-c', 'user.email=fixture@example.test', ...args], { encoding: 'utf8' });
    assert.equal(result.status, 0, result.stderr);
  };
  git('init', '-q'); fs.writeFileSync(path.join(root, 'file.txt'), 'one'); git('add', '.'); git('commit', '-qm', 'fixture');
  const command = (action, ...args) => run([action, '--root', root, '--session', 's1', ...args]);
  command('task-start', '--task', 'T1', '--goal', 'Inspect the requested scope');
  const result = command('task-feature', '--feature', 'review', '--feature-status', 'used', '--reason', 'Reviewed the actual source changes', '--evidence', 'review:current');
  assert.equal(result.features.review.sourceStatus, 'current');
  fs.appendFileSync(path.join(root, 'file.txt'), 'two');
  const changed = command('task-features');
  assert.equal(changed.features.review.status, 'used');
  assert.equal(changed.features.review.sourceStatus, 'stale');
  const status = command('session');
  assert.equal(status.featureOutcomes.features.review.sourceStatus, 'stale');
  command('task-start', '--task', 'T2', '--goal', 'A separate outcome');
  assert.equal(command('task-features').features.review.status, 'unknown');
});

test('relocated standalone package persists feature outcomes across processes', t => {
  const root = fs.realpathSync(fs.mkdtempSync(path.join(os.tmpdir(), 'craft-controls-export-')));
  t.after(() => fs.rmSync(root, { recursive: true, force: true }));
  const pkg = path.join(root, 'package');
  fs.cpSync(path.resolve('dist/codex/packages/craft-delivery'), pkg, { recursive: true });
  const options = { session: 'standalone-session', task: 'standalone-task' };
  assert.equal(fs.existsSync(path.join(pkg, 'support/hosts')), false);
  const invoke = (...args) => {
    const result = launch(pkg, [...args, '--root', root, '--session', options.session, '--json']);
    assert.equal(result.status, 0, result.stderr); return JSON.parse(result.stdout);
  };
  invoke('task-start', '--task', options.task, '--goal', 'Inspect portable controls');
  invoke('task-feature', '--feature', 'delegation', '--feature-status', 'unsupported', '--reason', 'No native adapter', '--evidence', 'host:capabilities');
  assert.equal(invoke('task-features').features.delegation.provenance, 'agent-declared');
  assert.deepEqual(invoke('worker-status').requests, []);
});
