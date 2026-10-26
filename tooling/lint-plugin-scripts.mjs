import { existsSync, readdirSync } from 'node:fs';
import path from 'node:path';
import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';

const sourceFolders = ['hosts', 'tooling', 'packages', 'tests', '.github'];
export function lintScripts(repo) {
  const errors = [];
  const queue = sourceFolders.map(folder => path.join(repo, folder)).filter(existsSync);
  const modules = [];
  while (queue.length) {
    const directory = queue.pop();
    for (const entry of readdirSync(directory, { withFileTypes: true })) {
      const absolute = path.join(directory, entry.name);
      const label = path.relative(repo, absolute);
      if (entry.isSymbolicLink()) { errors.push(`symlinked tool source: ${label}`); continue; }
      if (entry.isDirectory()) { queue.push(absolute); continue; }
      if (!entry.isFile()) continue;
      const extension = path.extname(entry.name).toLowerCase();
      if (['.sh', '.bash', '.bat', '.cmd', '.ps1'].includes(extension)) errors.push(`nonportable executable: ${label}; use Node ESM`);
      if (extension === '.mjs') modules.push(absolute);
    }
  }
  for (const module of modules.sort()) {
    const check = spawnSync(process.execPath, ['--check', module], { encoding: 'utf8', timeout: 10000, maxBuffer: 65536, shell: false });
    if (check.status !== 0) errors.push(`${path.relative(repo, module)}: ${check.error?.message || check.stderr.trim() || 'Node syntax check failed'}`);
  }
  return { checked: modules.length, errors };
}
if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const report = lintScripts(fileURLToPath(new URL('../', import.meta.url)));
  process.stdout.write(`Checked ${report.checked} Node ESM sources.\n`);
  if (report.errors.length) { process.stderr.write(`${report.errors.join('\n')}\n`); process.exitCode = 1; }
}
