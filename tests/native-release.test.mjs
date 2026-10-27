import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { createHash } from 'node:crypto';
import { execFileSync } from 'node:child_process';
import { assembleNativeRelease, releaseTargets } from '../tooling/native-release.mjs';
const digest = value => createHash('sha256').update(value).digest('hex');
function fixture(t) {
  const base = fs.mkdtempSync(path.join(os.tmpdir(), 'craft-native-release-'));
  t.after(() => fs.rmSync(base, { recursive: true, force: true }));
  const options = { input: path.join(base, 'input'), output: path.join(base, 'output'), revision: 'a'.repeat(40), version: '0.0.2', lockSha256: 'b'.repeat(64) };
  for (const target of releaseTargets) {
    const root = path.join(options.input, target);
    fs.mkdirSync(path.join(root, 'native-bin'), { recursive: true });
    const primary = `native-bin/craft${target.includes('windows') ? '.exe' : ''}`;
    fs.writeFileSync(path.join(root, primary), 'binary', { mode: 0o755 });
    fs.writeFileSync(path.join(root, 'native-bundle.json'), JSON.stringify({ formatVersion: 1, packVersion: options.version, protocolVersion: 1, profile: 'release', target,
      sourceRevision: options.revision, sourceDirty: false, cargoLockSha256: options.lockSha256, binaryVersion: '0.0.1',
      files: [primary].map(file => ({ path: file, bytes: 6, sha256: digest('binary') })) }));
  }
  return options;
}
test('release collects checked targets with checksummed archives and binary provenance', t => {
  const options = fixture(t);
  const index = assembleNativeRelease(options);
  assert.equal(index.sourceRevision, options.revision);
  assert.equal(index.assets.length, 3);
  for (const asset of index.assets) {
    const archive = path.join(options.output, asset.archive);
    assert.equal(digest(fs.readFileSync(archive)), asset.sha256);
    assert.equal(asset.binarySha256, digest('binary'));
    const relocated = path.join(options.output, asset.target);
    fs.mkdirSync(relocated);
    const descriptor = fs.openSync(archive, 'r');
    try {
      execFileSync('tar', ['-xzf', '-'], { cwd: relocated, stdio: [descriptor, 'pipe', 'pipe'] });
    } finally {
      fs.closeSync(descriptor);
    }
    assert.equal(JSON.parse(fs.readFileSync(path.join(relocated, 'native-bundle.json'))).target, asset.target);
    const executable = `native-bin/craft${asset.target.includes('windows') ? '.exe' : ''}`;
    assert.equal(digest(fs.readFileSync(path.join(relocated, executable))), asset.binarySha256);
  }
});
test('release resolves relative bundle and output directories from the caller', t => {
  const options = fixture(t);
  const module = new URL('../tooling/native-release.mjs', import.meta.url).href;
  const relative = { ...options, input: 'input', output: 'output with spaces' };
  execFileSync(process.execPath, ['--input-type=module', '--eval',
    `import { assembleNativeRelease } from ${JSON.stringify(module)}; assembleNativeRelease(${JSON.stringify(relative)});`],
  { cwd: path.dirname(options.input) });
  const output = path.join(path.dirname(options.input), relative.output);
  const index = JSON.parse(fs.readFileSync(path.join(output, 'native-release.json')));
  assert.deepEqual(index.assets.map(asset => asset.target), releaseTargets);
  for (const asset of index.assets) {
    assert.equal(digest(fs.readFileSync(path.join(output, asset.archive))), asset.sha256);
  }
});
for (const defect of ['version', 'revision', 'dirty', 'lock', 'target', 'checksum', 'extra', 'missing', 'primary', 'traversal', 'duplicate', 'symlink']) {
  test(`release refuses ${defect} before creating assets`, t => {
    const options = fixture(t), root = path.join(options.input, releaseTargets[2]);
    const file = path.join(root, 'native-bundle.json'), manifest = JSON.parse(fs.readFileSync(file));
    if (defect === 'version') manifest.packVersion = '9.9.9';
    if (defect === 'primary') {
      fs.unlinkSync(path.join(root, manifest.files[0].path));
      manifest.files.pop();
    }
    if (defect === 'revision') manifest.sourceRevision = 'c'.repeat(40);
    if (defect === 'dirty') manifest.sourceDirty = true;
    if (defect === 'lock') manifest.cargoLockSha256 = 'c'.repeat(64);
    if (defect === 'target') manifest.target = releaseTargets[0];
    if (defect === 'checksum') fs.writeFileSync(path.join(root, manifest.files[0].path), 'broken');
    if (defect === 'extra') fs.writeFileSync(path.join(root, 'unexpected'), 'extra');
    if (defect === 'missing') fs.unlinkSync(path.join(root, manifest.files[0].path));
    if (defect === 'traversal') manifest.files[0].path = '../outside';
    if (defect === 'duplicate') manifest.files.push(manifest.files[0]);
    if (defect === 'symlink') fs.symlinkSync(file, path.join(root, 'link'));
    fs.writeFileSync(file, JSON.stringify(manifest));
    assert.throws(() => assembleNativeRelease(options));
    assert.equal(fs.existsSync(options.output), false);
  });
}
