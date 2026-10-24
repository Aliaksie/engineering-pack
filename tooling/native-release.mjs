#!/usr/bin/env node
import fs from 'node:fs';
import path from 'node:path';
import { createHash } from 'node:crypto';
import { execFileSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';

export const releaseTargets = ['x86_64-unknown-linux-gnu', 'aarch64-apple-darwin', 'x86_64-pc-windows-msvc'];
const hash = file => createHash('sha256').update(fs.readFileSync(file)).digest('hex');
function files(root, relative = '') {
  return fs.readdirSync(path.join(root, relative), { withFileTypes: true }).flatMap(entry => {
    const name = relative ? `${relative}/${entry.name}` : entry.name;
    if (entry.isSymbolicLink()) throw new Error(`symlink in native bundle: ${name}`);
    if (entry.isDirectory()) return files(root, name);
    if (!entry.isFile()) throw new Error(`unsupported native file: ${name}`);
    return [name];
  });
}
export function assembleNativeRelease({ input, output, revision, version, lockSha256 }) {
  if (!/^[a-f0-9]{40}$/.test(revision) || !/^\d+\.\d+\.\d+$/.test(version)) throw new Error('invalid release identity');
  const bundles = releaseTargets.map(target => {
    const root = path.join(input, target);
    if (fs.lstatSync(root).isSymbolicLink()) throw new Error(`symlink native target: ${target}`);
    const actual = files(root);
    const manifest = JSON.parse(fs.readFileSync(path.join(root, 'native-bundle.json'), 'utf8'));
    if (manifest.formatVersion !== 1 || manifest.protocolVersion !== 1 || manifest.profile !== 'release'
      || manifest.target !== target || manifest.sourceRevision !== revision || manifest.sourceDirty !== false
      || manifest.cargoLockSha256 !== lockSha256) throw new Error(`invalid native provenance: ${target}`);
    if (!Array.isArray(manifest.files)) throw new Error(`missing native inventory: ${target}`);
    const listed = new Set();
    for (const file of manifest.files) {
      if (typeof file.path !== 'string' || file.path.includes('\\') || file.path.split('/').some(part => !part || part === '.' || part === '..')
        || file.path === 'native-bundle.json' || listed.has(file.path) || !actual.includes(file.path)) throw new Error(`invalid native inventory: ${target}`);
      listed.add(file.path);
      const location = path.join(root, file.path);
      if (fs.statSync(location).size !== file.bytes || hash(location) !== file.sha256) throw new Error(`native checksum mismatch: ${target}/${file.path}`);
    }
    const executable = `native-bin/craft-project-evidence${target.includes('windows') ? '.exe' : ''}`;
    if (!listed.has(executable) || actual.length !== listed.size + 1) throw new Error(`incomplete native inventory: ${target}`);
    return { root, target, executable, manifest };
  });
  // Validate all inputs before producing any release assets.
  fs.mkdirSync(output, { recursive: true });
  const assets = bundles.map(({ root, target, executable, manifest }) => {
    const name = `engineering-pack-${version}-native-${target}.tar.gz`;
    const destination = path.join(output, name);
    execFileSync('tar', ['-czf', destination, '-C', root, '.'], { timeout: 60000 });
    return { target, archive: name, sha256: hash(destination), binaryVersion: manifest.binaryVersion,
      binarySha256: manifest.files.find(file => file.path === executable).sha256 };
  });
  const index = { formatVersion: 1, version, sourceRevision: revision, cargoLockSha256: lockSha256, assets };
  fs.writeFileSync(path.join(output, 'native-release.json'), `${JSON.stringify(index, null, 2)}\n`);
  return index;
}
if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  try {
    if (process.argv.length !== 4) throw new Error('usage: native-release.mjs <downloaded-bundles> <release-output>');
    assembleNativeRelease({ input: path.resolve(process.argv[2]), output: path.resolve(process.argv[3]),
      revision: process.env.RELEASE_REVISION, version: process.env.RELEASE_VERSION, lockSha256: hash('Cargo.lock') });
  } catch (error) { console.error(error.message); process.exitCode = 1; }
}
