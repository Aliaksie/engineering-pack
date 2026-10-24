import fs from 'node:fs';
import path from 'node:path';
import { createHash } from 'node:crypto';
import { fileURLToPath } from 'node:url';

// Host packaging boundary: select a prebuilt executable; never download or compile.
export function nativeArtifact({ directory = fileURLToPath(new URL('../../native-bin/', import.meta.url)), platform = process.platform, arch = process.arch } = {}) {
  const unavailable = message => Object.assign(new Error(message), { code: 'NATIVE_UNAVAILABLE' });
  const metadata = path.join(directory, 'runtime.json');
  if (!fs.existsSync(metadata)) {
    // Existing explicit single-target bundles remain usable during the transition.
    return path.join(directory, `craft-project-evidence${platform === 'win32' ? '.exe' : ''}`);
  }
  if (fs.lstatSync(directory).isSymbolicLink() || fs.lstatSync(metadata).isSymbolicLink()) throw unavailable('Unsafe native runtime metadata');
  const index = JSON.parse(fs.readFileSync(metadata, 'utf8'));
  if (index.formatVersion !== 1 || !Array.isArray(index.artifacts)) throw unavailable('Incompatible native runtime metadata');
  const matches = index.artifacts.filter(entry => entry.platform === platform && entry.arch === arch);
  if (matches.length !== 1) throw unavailable(`Unsupported native platform: ${platform}/${arch}. No verified prebuilt runtime is included.`);
  const entry = matches[0];
  if (typeof entry.executable !== 'string' || entry.executable.includes('\\') || entry.executable.split('/').some(part => !part || part === '.' || part === '..')
    || !/^[a-f0-9]{64}$/.test(entry.sha256)) throw unavailable('Invalid native artifact metadata');
  const executable = path.join(directory, entry.executable);
  let current = directory;
  for (const part of entry.executable.split('/')) {
    current = path.join(current, part);
    if (fs.lstatSync(current).isSymbolicLink()) throw unavailable('Unsafe native executable path');
  }
  if (!fs.statSync(executable).isFile() || createHash('sha256').update(fs.readFileSync(executable)).digest('hex') !== entry.sha256) throw unavailable('Native executable checksum mismatch; reinstall the complete bundle');
  // Artifact transports may not retain Unix executable permission bits.
  if (platform !== 'win32' && !(fs.statSync(executable).mode & 0o100)) fs.chmodSync(executable, fs.statSync(executable).mode | 0o100);
  return executable;
}
