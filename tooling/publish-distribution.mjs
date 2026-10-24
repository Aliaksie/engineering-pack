#!/usr/bin/env node
// CI-only candidate Git transport. Stable promotion is a separate release action.
// Build/checks happen in the workflow before this script runs.
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { execFileSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import { copy } from './export/export.mjs';

export function publishDistribution(directory, remote, revision, environment = process.env) {
  const temporary = fs.mkdtempSync(path.join(os.tmpdir(), 'engineering-publish-'));
  try {
    copy(directory, temporary);
    const git = args => execFileSync('git', args, { cwd: temporary, env: environment, encoding: 'utf8', stdio: ['ignore', 'pipe', 'pipe'] });
    git(['init', '--initial-branch=distribution-next']);
    git(['remote', 'add', 'origin', remote]);
    const latest = git(['ls-remote', 'origin', 'refs/heads/main']).split(/\s/)[0];
    if (latest !== revision) throw new Error('main advanced; refusing to publish a stale source revision');
    const previous = git(['ls-remote', 'origin', 'refs/heads/distribution-next']).split(/\s/)[0];
    if (previous) {
      git(['fetch', '--depth=1', 'origin', 'distribution-next']);
      // Preserve branch history without checking old generated files into the new tree.
      git(['update-ref', 'refs/heads/distribution-next', git(['rev-parse', 'FETCH_HEAD']).trim()]);
    }
    git(['add', '--all']);
    if (previous && !git(['diff', '--cached', '--name-only']).trim()) {
      return false;
    } else {
      git(['-c', 'user.name=github-actions[bot]', '-c', 'user.email=41898282+github-actions[bot]@users.noreply.github.com',
        'commit', '-m', `Generate engineering pack from ${revision}`]);
      git(['push', 'origin', 'HEAD:refs/heads/distribution-next']);
      return true;
    }
  } finally {
    fs.rmSync(temporary, { recursive: true, force: true });
  }
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const repo = fileURLToPath(new URL('../', import.meta.url));
  const repository = process.env.GITHUB_REPOSITORY;
  const revision = process.env.PUBLISH_REVISION ?? process.env.GITHUB_SHA;
  const token = process.env.GH_TOKEN;
  if (process.env.GITHUB_ACTIONS !== 'true' || process.env.GITHUB_REF !== 'refs/heads/main'
    || !['push', 'workflow_dispatch'].includes(process.env.GITHUB_EVENT_NAME)
    || repository !== 'Aliaksie/engineering-pack' || !/^[a-f0-9]{40}$/.test(revision ?? '') || !token) {
    throw new Error('publishing requires an authorized main-branch GitHub Actions run');
  }
  const environment = { ...process.env,
    GIT_CONFIG_COUNT: '1',
    GIT_CONFIG_KEY_0: 'http.https://github.com/.extraheader',
    GIT_CONFIG_VALUE_0: `AUTHORIZATION: basic ${Buffer.from(`x-access-token:${token}`).toString('base64')}`
  };
  delete environment.GH_TOKEN;
  try {
    const changed = publishDistribution(path.join(repo, 'dist/distribution'), `https://github.com/${repository}.git`, revision, environment);
    process.stdout.write(changed ? `Published distribution-next from ${revision}.\n` : 'Distribution-next is unchanged.\n');
  } catch (error) {
    // Never serialize subprocess options/environment containing transport credentials.
    process.stderr.write(`Distribution publish failed: ${error.stderr?.toString().trim() || error.message}\n`);
    process.exitCode = 1;
  }
}
