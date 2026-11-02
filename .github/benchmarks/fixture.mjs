// Shared by the benchmark scripts: process helpers, the consumer fixture and one Hyperfine cell.
import fs from 'node:fs';
import path from 'node:path';
import { spawnSync } from 'node:child_process';

export const windows = process.platform === 'win32';
// Hyperfine splits -N commands with shell-words, where backslashes escape: hand it forward slashes.
export const slash = file => windows ? file.replaceAll('\\', '/') : file;
export const quote = file => `"${slash(file)}"`;
export const run = (command, args, settings = {}) => {
  const result = spawnSync(command, args, { encoding: 'utf8', maxBuffer: 64 * 1024 * 1024, ...settings });
  if (result.status !== 0) throw new Error(`${command} ${args.join(' ')} failed: ${result.stderr || result.error?.message || result.stdout}`);
  return result.stdout;
};
export const version = (command, args) => { try { return run(command, args).trim().split('\n')[0]; } catch { return null; } };
export const git = (cwd, ...args) => run('git', ['-c', 'core.autocrlf=false', '-c', 'commit.gpgsign=false', '-c', 'user.name=bench', '-c', 'user.email=bench@example.invalid', ...args], { cwd });
export const directoryBytes = directory => fs.readdirSync(directory, { withFileTypes: true, recursive: true })
  .filter(entry => entry.isFile()).reduce((total, entry) => total + fs.statSync(path.join(entry.parentPath, entry.name)).size, 0);
// Hosts start POSIX hooks through bash; on Windows that is Git Bash, never System32's WSL bash.
export const gitBash = () => windows ? path.resolve(run('git', ['--exec-path']).trim(), '../../../bin/bash.exe') : 'bash';

// Consumer fixture: 200 small sources in a committed Git repository, with Craft enabled.
export function consumer(root) {
  fs.mkdirSync(path.join(root, 'src'), { recursive: true }); fs.mkdirSync(path.join(root, '.craft'));
  for (let n = 0; n < 200; n++) fs.writeFileSync(path.join(root, `src/item-${n}.mjs`), `export function item${n}(value) {\n  return value + ${n};\n}\n`);
  fs.writeFileSync(path.join(root, '.craft/project.json'), '{"formatVersion":1}\n');
  git(root, 'init', '-q'); git(root, 'add', '.'); git(root, 'commit', '-qm', 'fixture');
  return root;
}

// One Hyperfine cell into `results`. `--metrics all` records every metric this platform and
// execution mode supports and skips the rest (peak memory on Windows; hardware counters under a
// shell or without counter permissions).
export function measurer({ hyperfine, output, results, save }) {
  return (name, cwd, args, env = process.env) => {
    const file = `raw/${name.replaceAll('/', '--')}.json`;
    const result = spawnSync(hyperfine, ['--output', 'pipe', '--style', 'basic', '--metrics', 'all', '-n', name,
      '--export-json', slash(path.join(output, file)), ...args], { cwd, env, encoding: 'utf8', maxBuffer: 16 * 1024 * 1024 });
    process.stdout.write(result.stdout);
    if (result.status !== 0) results.failures.push(`${name}: hyperfine exited ${result.status}: ${(result.stderr || result.error?.message || '').trim()}`);
    else results.cells.push({ name, file });
    save();
  };
}
