import { existsSync, readdirSync } from 'node:fs';
import { spawn, spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import os from 'node:os';
import path from 'node:path';

const repository = fileURLToPath(new URL('../', import.meta.url));
const suite = fileURLToPath(new URL('../tests/', import.meta.url));
const selected = readdirSync(suite, { withFileTypes: true })
  .filter(entry => entry.isFile() && entry.name.endsWith('.test.mjs'))
  .map(entry => path.join(suite, entry.name)).sort();
if (!selected.length) throw new Error('The test directory contains no runnable suites');
// Migrated commands run in the native runtime. Without an explicit CRAFT_NATIVE_BINARY,
// build the development executable once so every suite exercises the same binary.
const env = { ...process.env };
if (!env.CRAFT_NATIVE_BINARY) {
  const rustup = path.join(os.homedir(), '.cargo', 'bin', `cargo${process.platform === 'win32' ? '.exe' : ''}`);
  const cargo = env.CARGO ?? (spawnSync('cargo', ['--version']).error && existsSync(rustup) ? rustup : 'cargo');
  const build = spawnSync(cargo, ['build', '--locked', '--quiet', '-p', 'craft-application', '--bin', 'craft'], { cwd: repository, stdio: 'inherit', shell: false });
  if (build.status !== 0) {
    process.stderr.write(`Cannot build the native runtime for tests (${build.error?.message ?? `exit ${build.status}`}); install Rust or set CRAFT_NATIVE_BINARY.\n`);
    process.exit(1);
  }
  env.CRAFT_NATIVE_BINARY = path.join(env.CARGO_TARGET_DIR ? path.resolve(repository, env.CARGO_TARGET_DIR) : path.join(repository, 'target'), 'debug', `craft${process.platform === 'win32' ? '.exe' : ''}`);
}
const runner = spawn(process.execPath, ['--test', ...process.argv.slice(2), ...selected], {
  cwd: repository, stdio: 'inherit', shell: false, env
});
runner.once('error', error => { process.stderr.write(`Cannot start test runner: ${error.message}\n`); process.exitCode = 1; });
runner.once('exit', (code, signal) => { process.exitCode = code ?? (signal ? 1 : 0); });
