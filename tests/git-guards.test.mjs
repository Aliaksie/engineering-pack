import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { spawnSync } from 'node:child_process';

const source = path.resolve('.');
function fixture(t) {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'craft-git-guard-'));
  t.after(() => fs.rmSync(root, { recursive: true, force: true }));
  const run = (command, args, input) => spawnSync(command, args, { cwd: root,
    encoding: 'utf8', input, env: { ...process.env, GIT_CONFIG_NOSYSTEM: '1', GIT_CONFIG_GLOBAL: os.devNull } });
  const git = (...args) => run('git', args);
  assert.equal(git('init', '-b', 'main').status, 0);
  assert.equal(git('config', 'user.name', 'Guard Test').status, 0);
  assert.equal(git('config', 'user.email', 'guard@example.invalid').status, 0);
  fs.writeFileSync(path.join(root, 'code.txt'), 'initial\n');
  assert.equal(git('add', 'code.txt').status, 0);
  assert.equal(git('commit', '-m', 'initial').status, 0);
  for (const file of ['tooling/git-guard.mjs', 'tooling/install-git-hooks.mjs', '.githooks/pre-commit', '.githooks/pre-push']) {
    const target = path.join(root, file);
    fs.mkdirSync(path.dirname(target), { recursive: true });
    fs.copyFileSync(path.join(source, file), target);
    if (file.startsWith('.githooks/')) fs.chmodSync(target, 0o755);
  }
  return { root, run, git, install: () => run(process.execPath, ['tooling/install-git-hooks.mjs']) };
}

test('installed commit hook blocks protected/detached work and private plans, preserving completed task commits', t => {
  const { root, git, install } = fixture(t);
  assert.equal(install().status, 0);
  for (const branch of ['main', 'master']) {
    if (branch === 'master') assert.equal(git('switch', '-c', branch).status, 0);
    const blocked = git('commit', '--allow-empty', '-m', 'blocked');
    assert.notEqual(blocked.status, 0);
    assert.match(blocked.stderr, /task branch/);
  }
  assert.equal(git('switch', '-c', 'task/example').status, 0);
  fs.writeFileSync(path.join(root, 'code.txt'), 'completed\n');
  assert.equal(git('add', 'code.txt').status, 0);
  assert.equal(git('commit', '-m', 'complete coherent unit').status, 0);
  const completed = git('rev-parse', 'HEAD').stdout.trim();
  fs.mkdirSync(path.join(root, '.plan'));
  fs.writeFileSync(path.join(root, '.plan/internal.md'), 'private\n');
  fs.writeFileSync(path.join(root, '.gitignore'), '.plan/\n');
  assert.equal(git('add', '-f', '.plan/internal.md').status, 0);
  const blocked = git('commit', '-m', 'leaked plan');
  assert.notEqual(blocked.status, 0);
  assert.match(blocked.stderr, /Internal plans/);
  assert.equal(git('rev-parse', 'HEAD').stdout.trim(), completed);
  assert.equal(fs.readFileSync(path.join(root, '.plan/internal.md'), 'utf8'), 'private\n');
  assert.equal(git('restore', '--staged', '.plan/internal.md').status, 0);
  assert.equal(git('switch', '--detach').status, 0);
  assert.notEqual(git('commit', '--allow-empty', '-m', 'detached').status, 0);
});

test('installed push hook rejects protected destinations even from a task branch', t => {
  const { root, git, install } = fixture(t);
  assert.equal(install().status, 0);
  assert.equal(git('switch', '-c', 'task/example').status, 0);
  const remote = path.join(root, 'remote.git');
  assert.equal(git('init', '--bare', remote).status, 0);
  assert.equal(git('remote', 'add', 'origin', remote).status, 0);
  for (const branch of ['main', 'master']) {
    const blocked = git('push', 'origin', `HEAD:refs/heads/${branch}`);
    assert.notEqual(blocked.status, 0);
    assert.match(blocked.stderr, /Direct pushes/);
  }
  assert.equal(git('push', 'origin', 'HEAD:refs/heads/task/example').status, 0);
});

test('hook installer refuses to replace existing consumer hooks and is idempotent', t => {
  const { root, git, install } = fixture(t);
  const oldHook = path.join(root, '.git/hooks/commit-msg');
  fs.writeFileSync(oldHook, '#!/bin/sh\nexit 0\n');
  assert.notEqual(install().status, 0);
  assert.equal(fs.readFileSync(oldHook, 'utf8'), '#!/bin/sh\nexit 0\n');
  fs.unlinkSync(oldHook);
  assert.equal(git('config', '--local', 'core.hooksPath', 'custom-hooks').status, 0);
  const blocked = install();
  assert.notEqual(blocked.status, 0);
  assert.match(blocked.stderr, /compose its hooks explicitly/);
  assert.equal(git('config', '--get', 'core.hooksPath').stdout.trim(), 'custom-hooks');
  assert.equal(git('config', '--local', '--unset', 'core.hooksPath').status, 0);
  assert.equal(install().status, 0);
  assert.equal(install().status, 0);
});
