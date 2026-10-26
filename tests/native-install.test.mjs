import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { createHash } from 'node:crypto';
import { execFileSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import { installNativeArtifacts } from '../tooling/native-install.mjs';
import { nativeTargets } from '../tooling/native-release.mjs';
import { launch } from './craft.mjs';
const repo = fileURLToPath(new URL('../', import.meta.url));
const hash = value => createHash('sha256').update(value).digest('hex');
function fixture(t) {
  const temporary = fs.realpathSync(fs.mkdtempSync(path.join(os.tmpdir(), 'craft-native-install-')));
  t.after(() => fs.rmSync(temporary, { recursive: true, force: true }));
  const options = { input: path.join(temporary, 'input'), destination: path.join(temporary, 'host'), revision: 'a'.repeat(40), version: '0.0.2', lockSha256: 'b'.repeat(64) };
  for (const { target, platform } of nativeTargets) {
    const root = path.join(options.input, target);
    fs.mkdirSync(path.join(root, 'native-bin'), { recursive: true });
    const names = ['craft'].map(name => `native-bin/${name}${platform === 'win32' ? '.exe' : ''}`);
    for (const name of names) fs.writeFileSync(path.join(root, name), target);
    fs.writeFileSync(path.join(root, 'native-bundle.json'), JSON.stringify({ formatVersion: 1, packVersion: options.version, protocolVersion: 1, profile: 'release',
      target, sourceRevision: options.revision, sourceDirty: false, cargoLockSha256: options.lockSha256,
      files: names.map(name => ({ path: name, bytes: Buffer.byteLength(target), sha256: hash(target) })) }));
  }
  // A package is found by its launcher; the export root has none.
  fs.mkdirSync(path.join(options.destination, 'packages/example/bin'), { recursive: true });
  fs.writeFileSync(path.join(options.destination, 'packages/example/bin/craft'), '#!/bin/sh\n');
  return options;
}
test('each package launcher gets every prebuilt target where it selects them', t => {
  const options = fixture(t);
  assert.equal(installNativeArtifacts(options).roots, 1);
  const directory = path.join(options.destination, 'packages/example/support/native-bin');
  for (const { target, platform } of nativeTargets) {
    const executable = path.join(directory, target, `native-bin/craft${platform === 'win32' ? '.exe' : ''}`);
    assert.equal(fs.readFileSync(executable, 'utf8'), target);
  }
  const index = JSON.parse(fs.readFileSync(path.join(directory, 'runtime.json'), 'utf8'));
  assert.deepEqual(index.artifacts.map(entry => entry.target), nativeTargets.map(entry => entry.target));
  assert.equal(fs.existsSync(path.join(options.destination, 'native-bin')), false);
  assert.throws(() => installNativeArtifacts(options), /already installed/);
});
test('incomplete target collection leaves host exports untouched', t => {
  const options = fixture(t);
  fs.rmSync(path.join(options.input, nativeTargets[2].target), { recursive: true });
  assert.throws(() => installNativeArtifacts(options));
  assert.equal(fs.existsSync(path.join(options.destination, 'packages/example/support/native-bin')), false);
});
const bundle = process.env.CRAFT_PROJECT_EVIDENCE_BUNDLE;
test('cached plugin runs native refresh/query after relocation and bundle rollback', { skip: !bundle }, async t => {
  const options = fixture(t), metadata = JSON.parse(fs.readFileSync(path.join(bundle, 'native-bundle.json')));
  fs.rmSync(options.destination, { recursive: true });
  // Copy only one independently installed plugin, outside its marketplace and checkout.
  fs.cpSync(path.join(repo, 'dist/codex/packages/craft-engineering'), options.destination, { recursive: true });
  const input = path.dirname(path.resolve(bundle));
  installNativeArtifacts({ ...options, input, revision: metadata.sourceRevision, lockSha256: metadata.cargoLockSha256,
    targets: [metadata.target], allowDirty: process.env.CRAFT_NATIVE_DEVELOPMENT === '1' });
  const consumer = path.join(path.dirname(options.destination), 'consumer λ');
  fs.mkdirSync(consumer);
  execFileSync('git', ['init', '--quiet', consumer]);
  fs.writeFileSync(path.join(consumer, 'source.js'), 'export function retained() { return 1; }');
  const moved = path.join(path.dirname(options.destination), 'cached plugin λ');
  fs.renameSync(options.destination, moved);
  // The installed launcher selects the bundled runtime; no development override.
  const env = { ...process.env };
  delete env.CRAFT_NATIVE_BINARY;
  const craft = (root, args) => {
    const result = launch(root, ['project-evidence', ...args], { cwd: path.dirname(consumer), env });
    assert.equal(result.status, 0, result.stderr);
    return JSON.parse(result.stdout);
  };
  const run = (root, operation, args = []) => craft(root, [operation, '--enable', '--root', consumer, '--json', ...args]);
  assert.equal(run(moved, 'version').binaryVersion, metadata.binaryVersion);
  assert.equal(run(moved, 'refresh').changed, 1);
  assert.equal(run(moved, 'query', ['--value', 'retained', '--verify']).results[0].verification, 'verified-selected-source');
  const replacement = path.join(path.dirname(moved), 'replacement plugin');
  fs.cpSync(moved, replacement, { recursive: true });
  assert.equal(run(replacement, 'refresh').reused, 1);
  fs.rmSync(replacement, { recursive: true });
  assert.equal(run(moved, 'query', ['--value', 'retained', '--verify']).results.length, 1);
  const disabled = craft(moved, ['status', '--json']);
  assert.equal(disabled.enabled, false);
});
