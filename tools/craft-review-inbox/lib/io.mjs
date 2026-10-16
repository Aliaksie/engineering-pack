import fs from 'node:fs';
import path from 'node:path';
import { randomUUID } from 'node:crypto';
import { spawnSync } from 'node:child_process';

export function noSymlinks(file) {
  const absolute = path.resolve(file);
  const { root } = path.parse(absolute);
  let current = root;
  for (const component of absolute.slice(root.length).split(path.sep).filter(Boolean)) {
    current = path.join(current, component);
    if (fs.lstatSync(current, { throwIfNoEntry: false })?.isSymbolicLink()) throw new Error(`refusing symlinked inbox path: ${current}`);
  }
}
export function atomicWrite(file, text) {
  noSymlinks(file);
  fs.mkdirSync(path.dirname(file), { recursive: true });
  const temp = path.join(path.dirname(file), `.${randomUUID()}.tmp`);
  try {
    fs.writeFileSync(temp, text, { flag: 'wx', mode: 0o600 });
    fs.renameSync(temp, file);
  } finally { fs.rmSync(temp, { force: true }); }
}
export function jsonFile(file, fallback) {
  noSymlinks(file);
  if (!fs.existsSync(file) && arguments.length > 1) return fallback;
  return JSON.parse(fs.readFileSync(file, 'utf8'));
}
export const saveJson = (file, value) => atomicWrite(file, `${JSON.stringify(value, null, 2)}\n`);
export function withLock(config, action) {
  const lock = path.join(config.home, '.writer.lock');
  noSymlinks(lock);
  fs.mkdirSync(config.home, { recursive: true });
  let fd;
  try { fd = fs.openSync(lock, 'wx', 0o600); }
  catch (error) { if (error.code === 'EEXIST') throw new Error('review inbox writer is busy'); throw error; }
  try { return action(); }
  finally { fs.closeSync(fd); fs.rmSync(lock, { force: true }); }
}
export function run(command, args, { env = process.env, cwd } = {}) {
  const result = spawnSync(command, args, { env, cwd, shell: false, encoding: 'utf8', timeout: 60000, maxBuffer: 32 * 1024 * 1024 });
  // Provider stderr may contain private values. Keep errors bounded and do not echo credentials.
  if (result.status !== 0) throw new Error(`${command} failed (${result.error?.code ?? result.status ?? 'unknown'}); check tool availability, access and configuration`);
  return result.stdout;
}
