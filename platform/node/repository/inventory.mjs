import fs from 'node:fs';
import path from 'node:path';
import { spawnSync } from 'node:child_process';
import { createHash } from 'node:crypto';
const digest = value => createHash('sha256').update(JSON.stringify(value)).digest('hex');
const locatorVersion = digest(['../search.mjs', './inventory.mjs', '../../../engine/discovery/match.mjs', '../../../engine/discovery/plan.mjs'].map(file => fs.readFileSync(new URL(file, import.meta.url), 'utf8')));

// Locator-only reuse: one stat inventory validates the narrow packet. No arbitrary executor.
export function inventoryIdentity(root, paths, maxFiles) {
  const git = args => spawnSync('git', ['-c', 'core.fsmonitor=false', '-C', root, ...args], {
    encoding: 'utf8', timeout: 5000, maxBuffer: 2_097_152, env: { ...process.env, GIT_OPTIONAL_LOCKS: '0' }
  });
  const inventory = git(['ls-files', '--cached', '--others', '--exclude-standard', '-z']);
  const head = git(['rev-parse', '--verify', 'HEAD']);
  if (inventory.status !== 0 || head.status !== 0) return null;
  const names = [...new Set(inventory.stdout.split('\0').filter(name => name
    && !name.startsWith('.capabilities/state/')
    && !name.split('/').some(part => ['.git', 'node_modules', 'dist'].includes(part))
    && (!paths.length || paths.some(scope => scope === '.' || name === scope || name.startsWith(`${scope}/`)))))].sort();
  if (names.length > maxFiles) return null;
  // Config/manifest changes outside a narrow symbol scope still reconsider routing/runtime facts.
  names.push('.capabilities/project.json');
  const prefixes = new Set(['']);
  for (const scope of paths) {
    const parts = scope.split('/');
    for (let length = 1; length <= parts.length; length++) prefixes.add(`${parts.slice(0, length).join('/')}/`);
  }
  for (const prefix of prefixes) for (const manifest of ['package.json', 'pyproject.toml', 'go.mod', 'pom.xml', 'build.gradle', 'build.gradle.kts']) names.push(`${prefix}${manifest}`);
  try {
    return digest([locatorVersion, head.stdout.trim(), names.map(name => {
      let file = root;
      for (const part of name.split('/')) {
        if (part === '..' || path.isAbsolute(name)) throw new Error('unsafe inventory path');
        file = path.join(file, part);
        if (fs.lstatSync(file, { throwIfNoEntry: false })?.isSymbolicLink()) throw new Error('symlink inventory');
      }
      const stat = fs.statSync(file, { bigint: true, throwIfNoEntry: false });
      return [name, stat ? [stat.dev, stat.ino, stat.size, stat.mtimeNs, stat.ctimeNs].map(String) : null];
    })]);
  } catch { return null; }
}

