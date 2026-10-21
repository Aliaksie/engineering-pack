import fs from 'node:fs';
import path from 'node:path';
import { spawnSync } from 'node:child_process';

// Observe only explicitly selected roots; never search sibling repositories.
export function repositoryRoots(root, scopes) {
  const workspace = fs.realpathSync(root);
  const seen = new Set();
  return scopes.map(scope => {
    const candidate = path.resolve(workspace, scope);
    const relative = path.relative(workspace, candidate);
    if (relative === '..' || relative.startsWith(`..${path.sep}`) || path.isAbsolute(relative)) throw new Error('repository scope escapes workspace');
    const canonical = fs.realpathSync(candidate);
    // Reject aliases so a saved relative selection cannot silently change its target.
    if (canonical !== candidate) throw new Error('repository scope must not traverse symlinks');
    const result = spawnSync('git', ['-C', canonical, 'rev-parse', '--show-toplevel'], {
      encoding: 'utf8', timeout: 5000, maxBuffer: 16384, env: { ...process.env, GIT_OPTIONAL_LOCKS: '0' }
    });
    if (result.status !== 0 || path.relative(canonical, fs.realpathSync(result.stdout.trim())) !== '') throw new Error(`repository scope is not a Git worktree root: ${scope}`);
    if (seen.has(canonical)) throw new Error('duplicate repository root');
    seen.add(canonical);
    return { path: scope, root: canonical };
  });
}
