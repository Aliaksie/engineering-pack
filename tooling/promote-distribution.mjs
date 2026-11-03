#!/usr/bin/env node
// Stable promotion: commits one verified release's distribution tree onto `distribution`.
// The workflow downloads and verifies the release asset before this script runs.
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { execFileSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import { copy } from './export/export.mjs';
import { parseVersion } from './version.mjs';

const sha = value => /^[a-f0-9]{40}$/.test(value ?? '');

export function promoteDistribution(directory, remote, { version, revision, stable }, environment = process.env) {
  parseVersion(version);
  if (!sha(revision)) throw new Error('expected the release source revision as a full SHA');
  if (!sha(stable)) throw new Error('expected the current stable revision as a full SHA');
  const marker = JSON.parse(fs.readFileSync(path.join(directory, '.engineering-distribution.json'), 'utf8'));
  if (marker.formatVersion !== 1 || marker.version !== version) throw new Error('distribution version mismatch');
  const temporary = fs.mkdtempSync(path.join(os.tmpdir(), 'engineering-promote-'));
  try {
    copy(directory, temporary);
    const git = args => execFileSync('git', args, { cwd: temporary, env: environment, encoding: 'utf8', stdio: ['ignore', 'pipe', 'pipe'] }).trim();
    git(['init', '--initial-branch=distribution']);
    git(['remote', 'add', 'origin', remote]);
    const refs = Object.fromEntries(git(['ls-remote', 'origin', `refs/tags/v${version}`, `refs/tags/v${version}^{}`])
      .split('\n').filter(Boolean).map(line => line.split(/\s+/).reverse()));
    const tag = refs[`refs/tags/v${version}^{}`] ?? refs[`refs/tags/v${version}`];
    if (tag !== revision) throw new Error(`tag v${version} does not resolve to the release revision`);
    const current = git(['ls-remote', 'origin', 'refs/heads/distribution']).split(/\s/)[0];
    if (current !== stable) throw new Error('stable distribution moved; refusing to promote over an unexpected revision');
    git(['fetch', '--depth=1', 'origin', 'distribution']);
    // Keep stable history: the new commit's parent is the expected stable revision.
    git(['update-ref', 'refs/heads/distribution', git(['rev-parse', 'FETCH_HEAD'])]);
    git(['add', '--all']);
    if (!git(['diff', '--cached', '--name-only'])) return false;
    git(['-c', 'user.name=github-actions[bot]', '-c', 'user.email=41898282+github-actions[bot]@users.noreply.github.com',
      'commit', '-m', `Promote Craft Capabilities ${version} from ${revision}`]);
    // A plain push is fast-forward only; a concurrent update is rejected, never overwritten.
    git(['push', 'origin', 'HEAD:refs/heads/distribution']);
    return true;
  } finally {
    fs.rmSync(temporary, { recursive: true, force: true });
  }
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const repository = process.env.GITHUB_REPOSITORY;
  const token = process.env.GH_TOKEN;
  const directory = process.argv[2];
  if (process.env.GITHUB_ACTIONS !== 'true' || process.env.GITHUB_REF !== 'refs/heads/main'
    || process.env.GITHUB_EVENT_NAME !== 'workflow_dispatch' || repository !== 'Aliaksie/engineering-pack' || !token || !directory) {
    throw new Error('promotion requires an authorized manual main-branch GitHub Actions run');
  }
  const environment = { ...process.env,
    GIT_CONFIG_COUNT: '1',
    GIT_CONFIG_KEY_0: 'http.https://github.com/.extraheader',
    GIT_CONFIG_VALUE_0: `AUTHORIZATION: basic ${Buffer.from(`x-access-token:${token}`).toString('base64')}`
  };
  delete environment.GH_TOKEN;
  const release = { version: process.env.PROMOTE_VERSION, revision: process.env.PROMOTE_REVISION, stable: process.env.PROMOTE_STABLE };
  try {
    const changed = promoteDistribution(directory, `https://github.com/${repository}.git`, release, environment);
    process.stdout.write(changed ? `Promoted ${release.version} to distribution.\n` : 'Distribution already matches this release.\n');
  } catch (error) {
    // Never serialize subprocess options/environment containing transport credentials.
    process.stderr.write(`Distribution promotion failed: ${error.stderr?.toString().trim() || error.message}\n`);
    process.exitCode = 1;
  }
}
