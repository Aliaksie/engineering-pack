import { execFileSync } from 'node:child_process';
import { readFileSync } from 'node:fs';

// Authoring-repository policy; consumer branch names remain consumer-owned.
const protectedBranches = new Set(['main', 'master']);
const git = (...args) => execFileSync('git', args, { encoding: 'utf8', stdio: ['ignore', 'pipe', 'pipe'] }).trim();

try {
  const mode = process.argv[2];
  if (!['check', 'pre-commit', 'pre-push'].includes(mode)) throw new Error('Expected check, pre-commit or pre-push');
  const branch = git('symbolic-ref', '--quiet', '--short', 'HEAD');
  if (!branch || protectedBranches.has(branch)) {
    throw new Error('Use a task branch before implementation or commits; main/master are protected.');
  }
  if (mode === 'pre-commit') {
    const paths = git('diff', '--cached', '--name-only', '--diff-filter=ACMR', '-z').split('\0');
    if (paths.some(file => /(^|\/)\.plan\//.test(file) || file === 'PLAN.md')) {
      throw new Error('Internal plans must remain untracked; remove them from the index without deleting local notes.');
    }
  }
  if (mode === 'pre-push') {
    for (const line of readFileSync(0, 'utf8').split('\n').filter(Boolean)) {
      const fields = line.trim().split(/\s+/);
      if (fields.length !== 4) throw new Error('Cannot evaluate malformed pre-push input.');
      if (protectedBranches.has(fields[2].replace(/^refs\/heads\//, ''))) {
        throw new Error('Direct pushes or deletions of main/master are blocked; use the repository review/merge process.');
      }
    }
  }
} catch (error) {
  process.stderr.write(`Git guard: ${error.message}\n`);
  process.exitCode = 1;
}
