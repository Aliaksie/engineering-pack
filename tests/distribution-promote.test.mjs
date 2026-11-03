import fs from 'node:fs';
import path from 'node:path';
import os from 'node:os';
import { execFileSync, spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import test from 'node:test';
import assert from 'node:assert/strict';
import { promoteDistribution } from '../tooling/promote-distribution.mjs';

// Real Git against a local bare remote: no GitHub credentials or network access.
function fixture(t, { annotated = false } = {}) {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'distribution-promote-'));
  t.after(() => fs.rmSync(root, { recursive: true, force: true }));
  const remote = path.join(root, 'remote.git'), source = path.join(root, 'source'), bundle = path.join(root, 'bundle');
  fs.mkdirSync(source); fs.mkdirSync(bundle);
  const git = (args, cwd = source) => execFileSync('git', args, { cwd, encoding: 'utf8', stdio: ['ignore', 'pipe', 'pipe'] }).trim();
  const identity = ['-c', 'user.name=Test', '-c', 'user.email=test@example.invalid'];
  git(['init', '--bare', remote]);
  git(['init', '--initial-branch=main']);
  git(['remote', 'add', 'origin', remote]);
  fs.writeFileSync(path.join(source, 'old.md'), 'Previous stable.\n');
  git(['add', '.']); git([...identity, 'commit', '-m', 'stable']);
  git(['push', 'origin', 'HEAD:refs/heads/distribution']);
  fs.writeFileSync(path.join(source, 'source.md'), 'Release source.\n');
  git(['add', '.']); git([...identity, 'commit', '-m', 'release']);
  git(['push', 'origin', 'HEAD:refs/heads/main']);
  const revision = git(['rev-parse', 'HEAD']);
  git([...identity, 'tag', ...(annotated ? ['-a', '-m', 'v0.0.3'] : []), 'v0.0.3', revision]);
  git(['push', 'origin', 'refs/tags/v0.0.3']);
  fs.writeFileSync(path.join(bundle, '.engineering-distribution.json'), JSON.stringify({ formatVersion: 1, version: '0.0.3' }));
  fs.writeFileSync(path.join(bundle, 'README.md'), 'Generated.\n');
  const stable = git(['--git-dir', remote, 'rev-parse', 'distribution']);
  return { remote, bundle, revision, stable, ref: name => git(['--git-dir', remote, 'rev-parse', name]), tree: name => git(['--git-dir', remote, 'ls-tree', '--name-only', name]) };
}

test('promotion commits the release tree on top of the expected stable revision and is idempotent', t => {
  const f = fixture(t);
  assert.equal(promoteDistribution(f.bundle, f.remote, { version: '0.0.3', revision: f.revision, stable: f.stable }), true);
  assert.equal(f.ref('distribution^'), f.stable);
  assert.equal(f.tree('distribution'), '.engineering-distribution.json\nREADME.md');
  const promoted = f.ref('distribution');
  assert.equal(promoteDistribution(f.bundle, f.remote, { version: '0.0.3', revision: f.revision, stable: promoted }), false);
  assert.equal(f.ref('distribution'), promoted);
});

test('promotion accepts an annotated release tag', t => {
  const f = fixture(t, { annotated: true });
  assert.equal(promoteDistribution(f.bundle, f.remote, { version: '0.0.3', revision: f.revision, stable: f.stable }), true);
});

test('promotion refuses a moved stable branch, a wrong tag revision and a mismatched version', t => {
  const f = fixture(t);
  const options = { version: '0.0.3', revision: f.revision, stable: f.stable };
  assert.throws(() => promoteDistribution(f.bundle, f.remote, { ...options, stable: 'a'.repeat(40) }), /stable distribution moved/);
  assert.throws(() => promoteDistribution(f.bundle, f.remote, { ...options, revision: 'b'.repeat(40) }), /does not resolve/);
  assert.throws(() => promoteDistribution(f.bundle, f.remote, { ...options, version: '0.0.4' }), /version mismatch/);
  assert.equal(f.ref('distribution'), f.stable);
});

test('promotion CLI refuses local or untrusted workflow execution', () => {
  const script = fileURLToPath(new URL('../tooling/promote-distribution.mjs', import.meta.url));
  const result = spawnSync(process.execPath, [script, '.'], { env: { ...process.env, GITHUB_ACTIONS: 'false', GH_TOKEN: '' }, encoding: 'utf8' });
  assert.notEqual(result.status, 0);
  assert.match(result.stderr, /authorized manual main-branch/);
});
