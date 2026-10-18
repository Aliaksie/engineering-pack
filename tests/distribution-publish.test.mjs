import fs from 'node:fs';
import path from 'node:path';
import os from 'node:os';
import { execFileSync, spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import test from 'node:test';
import assert from 'node:assert/strict';
import { publishDistribution } from '../tooling/publish-distribution.mjs';

// Real Git against a local bare remote: no GitHub credentials or network access.
test('publishing creates a branch, skips unchanged trees, preserves history and removes old files', t => {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'distribution-transport-'));
  t.after(() => fs.rmSync(root, { recursive: true, force: true }));
  const remote = path.join(root, 'remote.git');
  const source = path.join(root, 'source');
  const bundle = path.join(root, 'bundle');
  fs.mkdirSync(source);
  fs.mkdirSync(bundle);
  const git = (args, cwd = source) => execFileSync('git', args, { cwd, encoding: 'utf8', stdio: ['ignore', 'pipe', 'pipe'] }).trim();
  git(['init', '--bare', remote]);
  git(['init', '--initial-branch=main']);
  fs.writeFileSync(path.join(source, 'source.md'), 'Canonical source.\n');
  git(['add', '.']);
  const commit = message => git(['-c', 'user.name=Test', '-c', 'user.email=test@example.invalid', 'commit', '-m', message]);
  commit('source');
  git(['remote', 'add', 'origin', remote]);
  git(['push', 'origin', 'main']);
  const revision = git(['rev-parse', 'HEAD']);
  fs.writeFileSync(path.join(bundle, 'obsolete.md'), 'Old generated file.\n');
  assert.equal(publishDistribution(bundle, remote, revision), true);
  const first = git(['--git-dir', remote, 'rev-parse', 'distribution']);
  assert.equal(publishDistribution(bundle, remote, revision), false);
  fs.unlinkSync(path.join(bundle, 'obsolete.md'));
  fs.writeFileSync(path.join(bundle, 'current.md'), 'New generated file.\n');
  assert.equal(publishDistribution(bundle, remote, revision), true);
  assert.equal(git(['--git-dir', remote, 'rev-parse', 'distribution^']), first);
  assert.equal(git(['--git-dir', remote, 'ls-tree', '--name-only', 'distribution']), 'current.md');
  fs.writeFileSync(path.join(source, 'source.md'), 'New source revision.\n');
  git(['add', '.']);
  commit('advance main');
  git(['push', 'origin', 'main']);
  assert.throws(() => publishDistribution(bundle, remote, revision), /stale/);
});

test('publisher CLI refuses local or untrusted workflow execution', () => {
  const script = fileURLToPath(new URL('../tooling/publish-distribution.mjs', import.meta.url));
  const result = spawnSync(process.execPath, [script], {
    env: { ...process.env, GITHUB_ACTIONS: 'false', GH_TOKEN: '' }, encoding: 'utf8'
  });
  assert.notEqual(result.status, 0);
  assert.match(result.stderr, /authorized main-branch/);
});
