import { spawnSync } from 'node:child_process';

function git(root, args) {
  const result = spawnSync('git', ['-c', 'core.fsmonitor=false', '-C', root, ...args], { encoding: 'utf8', timeout: 15000, maxBuffer: 16 * 1024 * 1024 });
  if (result.status !== 0) throw new Error(`Git snapshot failed: ${result.stderr?.trim() || result.error?.message || result.status}`);
  return result.stdout;
}
export function reviewSnapshot({ root, base, head, provider = 'local', repository = 'local', changeId = null }) {
  if (!['local', 'github', 'gitlab'].includes(provider)) throw new Error('unsupported review provider');
  if (provider !== 'local' && !/^[1-9][0-9]*$/.test(String(changeId))) throw new Error('remote change id required');
  if (typeof repository !== 'string' || !repository.trim()) throw new Error('repository identity required');
  for (const ref of [base, head]) if (typeof ref !== 'string' || !ref || ref.startsWith('-')) throw new Error('explicit base and head references required');
  const baseSha = git(root, ['rev-parse', '--verify', '--end-of-options', `${base}^{commit}`]).trim();
  const headSha = git(root, ['rev-parse', '--verify', '--end-of-options', `${head}^{commit}`]).trim();
  const mergeBase = git(root, ['merge-base', baseSha, headSha]).trim();
  const scope = ['--', '.', ':(exclude,glob)**/.craft/state/**', ':(exclude,glob)**/.craft/state', ':(exclude,glob)**/.capabilities/state/**', ':(exclude,glob)**/.capabilities/state'];
  const files = git(root, ['diff', '--name-only', '-z', '--no-renames', mergeBase, headSha, ...scope]).split('\0').filter(Boolean);
  const diff = git(root, ['diff', '--no-ext-diff', '--no-textconv', '--no-color', '--no-renames', '--unified=3', mergeBase, headSha, ...scope]);
  return { formatVersion: 1, provider, repository, changeId, baseSha, headSha, mergeBase, files, diff,
    verification: 'source snapshot only; no project code, tests or model executed' };
}
