import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { createHash } from 'node:crypto';
import { execFileSync } from 'node:child_process';
import { fileURLToPath, pathToFileURL } from 'node:url';
import { installNativeArtifacts } from '../tooling/native-install.mjs';
import { nativeTargets } from '../tooling/native-release.mjs';
import { nativeArtifact } from '../platform/node/native-artifact.mjs';
const repo = fileURLToPath(new URL('../', import.meta.url));
const hash = value => createHash('sha256').update(value).digest('hex');
function fixture(t) {
  const temporary = fs.realpathSync(fs.mkdtempSync(path.join(os.tmpdir(), 'craft-native-install-')));
  t.after(() => fs.rmSync(temporary, { recursive: true, force: true }));
  const options = { input: path.join(temporary, 'input'), destination: path.join(temporary, 'host'), revision: 'a'.repeat(40), version: '0.0.2', lockSha256: 'b'.repeat(64) };
  for (const { target, platform } of nativeTargets) {
    const root = path.join(options.input, target);
    fs.mkdirSync(path.join(root, 'native-bin'), { recursive: true });
    const names = ['craft', 'craft-project-evidence'].map(name => `native-bin/${name}${platform === 'win32' ? '.exe' : ''}`);
    for (const name of names) fs.writeFileSync(path.join(root, name), target);
    fs.writeFileSync(path.join(root, 'native-bundle.json'), JSON.stringify({ formatVersion: 1, packVersion: options.version, protocolVersion: 1, profile: 'release',
      target, sourceRevision: options.revision, sourceDirty: false, cargoLockSha256: options.lockSha256,
      files: names.map(name => ({ path: name, bytes: Buffer.byteLength(target), sha256: hash(target) })) }));
  }
  for (const root of [options.destination, path.join(options.destination, 'packages/example/support')]) {
    fs.mkdirSync(path.join(root, 'platform/node'), { recursive: true });
    for (const name of ['native-artifact.mjs', 'native-command.mjs', 'project-evidence.mjs']) fs.copyFileSync(path.join(repo, 'platform/node', name), path.join(root, 'platform/node', name));
  }
  return options;
}
test('self-contained host and plugin select each prebuilt target and refuse unsupported platforms', async t => {
  const options = fixture(t);
  assert.equal(installNativeArtifacts(options).roots, 2);
  for (const root of [options.destination, path.join(options.destination, 'packages/example/support')]) {
    const directory = path.join(root, 'native-bin');
    for (const entry of nativeTargets) {
      const binary = nativeArtifact({ directory, platform: entry.platform, arch: entry.arch });
      assert.equal(fs.readFileSync(binary, 'utf8'), entry.target);
      assert.match(path.basename(binary), /^craft(?:\.exe)?$/);
    }
    for (const [platform, arch] of [['linux', 'arm64'], ['darwin', 'x64'], ['freebsd', 'x64']]) {
      assert.throws(() => nativeArtifact({ directory, platform, arch }), /Unsupported native platform/);
    }
    const invoke = (await import(pathToFileURL(path.join(root, 'platform/node/project-evidence.mjs')).href)).projectEvidenceProcess;
    fs.rmSync(directory, { recursive: true });
    assert.equal(invoke({ enabled: false, operation: 'invalid' }).enabled, false);
  }
});
test('damaged binaries and unsafe selection paths fail before activation', t => {
  const options = fixture(t);
  installNativeArtifacts(options);
  const directory = path.join(options.destination, 'native-bin');
  const entry = nativeTargets[0], select = () => nativeArtifact({ directory, platform: entry.platform, arch: entry.arch });
  const binary = select();
  fs.writeFileSync(binary, 'damaged');
  assert.throws(select, /checksum/);
  const file = path.join(directory, 'runtime.json'), index = JSON.parse(fs.readFileSync(file));
  index.artifacts[0].executable = '../outside';
  fs.writeFileSync(file, JSON.stringify(index));
  assert.throws(select, /Invalid native artifact/);
});
test('incomplete target collection leaves host exports untouched', t => {
  const options = fixture(t);
  fs.rmSync(path.join(options.input, nativeTargets[2].target), { recursive: true });
  assert.throws(() => installNativeArtifacts(options));
  assert.equal(fs.existsSync(path.join(options.destination, 'native-bin')), false);
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
  const run = (root, operation, args = []) => JSON.parse(execFileSync(process.execPath,
    [path.join(root, 'support/cli/project-evidence.mjs'), operation, '--enable', '--root', consumer, '--json', ...args], { cwd: path.dirname(consumer), encoding: 'utf8' }));
  assert.equal(run(moved, 'version').binaryVersion, metadata.binaryVersion);
  assert.equal(run(moved, 'refresh').changed, 1);
  assert.equal(run(moved, 'query', ['--value', 'retained', '--verify']).results[0].verification, 'verified-selected-source');
  const replacement = path.join(path.dirname(moved), 'replacement plugin');
  fs.cpSync(moved, replacement, { recursive: true });
  assert.equal(run(replacement, 'refresh').reused, 1);
  fs.rmSync(replacement, { recursive: true });
  assert.equal(run(moved, 'query', ['--value', 'retained', '--verify']).results.length, 1);
  const directory = path.join(moved, 'support/native-bin');
  const executable = nativeArtifact({ directory });
  fs.appendFileSync(executable, 'tampered');
  assert.throws(() => run(moved, 'version'), /checksum mismatch/);
  const disabled = JSON.parse(execFileSync(process.execPath, [path.join(moved, 'support/cli/project-evidence.mjs'), 'status', '--json'], { encoding: 'utf8' }));
  assert.equal(disabled.enabled, false);
});
