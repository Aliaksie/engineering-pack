#!/usr/bin/env node
import fs from 'node:fs';
import path from 'node:path';
import { createHash } from 'node:crypto';
import { execFileSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import { copy } from './export/export.mjs';
import { files } from './lib/capabilities.mjs';
import { nativeTargets, releaseTargets, validateNativeBundles } from './native-release.mjs';

// Build-time assembly only. Each independently cached plugin carries its own runtime.
export function installNativeArtifacts({ destination, ...options }) {
  const bundles = validateNativeBundles(options);
  const roots = files(destination).filter(file => file.endsWith(`${path.sep}platform${path.sep}node${path.sep}project-evidence.mjs`))
    .map(file => path.resolve(path.dirname(file), '../..'));
  if (!roots.length) throw new Error('no host runtime found in distribution');
  const artifacts = bundles.map(({ target, executable, manifest }) => ({ ...nativeTargets.find(entry => entry.target === target),
    executable: `${target}/${executable}`, sha256: manifest.files.find(entry => entry.path === executable).sha256,
    binaryVersion: manifest.binaryVersion, sourceDirty: manifest.sourceDirty }));
  for (const root of roots) {
    const directory = path.join(root, 'native-bin');
    // Host builds are fresh. Refuse repeated assembly instead of retaining stale targets.
    if (fs.existsSync(directory)) throw new Error(`native artifacts already installed: ${root}`);
  }
  for (const root of roots) {
    const directory = path.join(root, 'native-bin');
    for (const { root: source, target } of bundles) copy(source, path.join(directory, target));
    fs.writeFileSync(path.join(directory, 'runtime.json'), `${JSON.stringify({ formatVersion: 1,
      packVersion: options.version, sourceRevision: options.revision, cargoLockSha256: options.lockSha256, artifacts }, null, 2)}\n`);
  }
  return { roots: roots.length, targets: artifacts.map(entry => entry.target) };
}
if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  try {
    const [input, destination, flag] = process.argv.slice(2);
    if (!input || !destination || (flag && flag !== '--current-target') || process.argv.length > 5) throw new Error('usage: native-install.mjs <bundles> <host-export> [--current-target]');
    const current = nativeTargets.find(entry => entry.platform === process.platform && entry.arch === process.arch);
    if (flag && !current) throw new Error('unsupported current native platform');
    const revision = execFileSync('git', ['rev-parse', 'HEAD'], { encoding: 'utf8' }).trim();
    const version = JSON.parse(fs.readFileSync('package.json', 'utf8')).version;
    const lockSha256 = createHash('sha256').update(fs.readFileSync('Cargo.lock')).digest('hex');
    console.log(JSON.stringify(installNativeArtifacts({ input: path.resolve(input), destination: path.resolve(destination), revision, version,
      lockSha256, targets: flag ? [current.target] : releaseTargets })));
  } catch (error) { console.error(error.message); process.exitCode = 1; }
}
