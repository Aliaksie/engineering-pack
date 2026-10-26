import { existsSync } from 'node:fs';
import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import os from 'node:os';
import path from 'node:path';

const repository = fileURLToPath(new URL('../../', import.meta.url));
// Commands and hooks run in the native runtime. Without an explicit
// CRAFT_NATIVE_BINARY, build the development executable once and return its path.
export function nativeBinary(env = process.env) {
  if (env.CRAFT_NATIVE_BINARY) return env.CRAFT_NATIVE_BINARY;
  const rustup = path.join(os.homedir(), '.cargo', 'bin', `cargo${process.platform === 'win32' ? '.exe' : ''}`);
  const cargo = env.CARGO ?? (spawnSync('cargo', ['--version']).error && existsSync(rustup) ? rustup : 'cargo');
  const build = spawnSync(cargo, ['build', '--locked', '--quiet', '-p', 'craft-application', '--bin', 'craft'], { cwd: repository, stdio: 'inherit', shell: false });
  if (build.status !== 0) throw new Error(`Cannot build the native runtime (${build.error?.message ?? `exit ${build.status}`}); install Rust or set CRAFT_NATIVE_BINARY.`);
  return path.join(env.CARGO_TARGET_DIR ? path.resolve(repository, env.CARGO_TARGET_DIR) : path.join(repository, 'target'), 'debug', `craft${process.platform === 'win32' ? '.exe' : ''}`);
}
