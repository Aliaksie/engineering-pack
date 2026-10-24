import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import os from 'node:os';
import { createHash } from 'node:crypto';
import { execFileSync } from 'node:child_process';

const artifact = process.env.CRAFT_PROJECT_EVIDENCE_BUNDLE;

test('native bundle works after relocation with checked notices and no authoring runtime', { skip: !artifact }, t => {
  const temporary = fs.realpathSync(fs.mkdtempSync(path.join(os.tmpdir(), 'craft-native-bundle-')));
  t.after(() => fs.rmSync(temporary, { recursive: true, force: true }));
  const bundle = path.join(temporary, 'moved bundle');
  fs.cpSync(path.resolve(artifact), bundle, { recursive: true });
  const manifest = JSON.parse(fs.readFileSync(path.join(bundle, 'native-bundle.json'), 'utf8'));
  assert.equal(manifest.formatVersion, 1);
  assert.equal(manifest.protocolVersion, 1);
  assert.equal(manifest.schemaVersion, 1);
  assert.equal(manifest.profile, 'release');
  assert.equal(manifest.providers.length, 2);
  assert.ok(manifest.files.length > 4);
  for (const entry of manifest.files) {
    assert.ok(!path.isAbsolute(entry.path) && !entry.path.split('/').includes('..'));
    const bytes = fs.readFileSync(path.join(bundle, entry.path));
    assert.equal(bytes.length, entry.bytes);
    assert.equal(createHash('sha256').update(bytes).digest('hex'), entry.sha256);
  }
  const dependencies = JSON.parse(fs.readFileSync(path.join(bundle, 'dependencies.json'), 'utf8'));
  for (const name of ['tree-sitter-java', 'tree-sitter-javascript', 'rusqlite', 'libsqlite3-sys', 'unicode-ident']) {
    const dependency = dependencies.packages.find(pkg => pkg.name === name);
    assert.ok(dependency?.notices.length);
    for (const notice of dependency.notices) assert.ok(fs.readFileSync(path.join(bundle, notice), 'utf8').length > 100);
  }
  assert.match(fs.readFileSync(path.join(bundle, 'notices/libsqlite3-sys-0.38.2/SQLite-NOTICE'), 'utf8'), /disclaims copyright/);
  assert.deepEqual(fs.readdirSync(bundle).sort(), ['LICENSE', 'README.md', 'application', 'cli', 'dependencies.json', 'native-bin', 'native-bundle.json', 'notices', 'platform']);
  const root = path.join(temporary, 'selected');
  fs.mkdirSync(root);
  execFileSync('git', ['init', '--quiet', root]);
  fs.writeFileSync(path.join(root, 'source.js'), 'export const saved = 1;');
  fs.writeFileSync(path.join(root, 'Source.java'), 'class Source { void caller() { ping(); } void ping() {} }');
  const run = (operation, args = [], enable = true) => JSON.parse(execFileSync(process.execPath,
    [path.join(bundle, 'cli/project-evidence.mjs'), operation, ...(enable ? ['--enable'] : []), '--root', root, '--json', ...args],
    { cwd: temporary, encoding: 'utf8' }));
  assert.equal(run('version').binaryVersion, manifest.binaryVersion);
  assert.equal(run('refresh').changed, 2);
  const result = run('query', ['--value', 'ping', '--verify']);
  const call = result.results.find(row => row.evidence.kind === 'java:invokes-syntax');
  assert.equal(call.verification, 'verified-selected-source');
  assert.equal(run('query', ['--field', 'id', '--value', call.evidence.container]).results[0].evidence.name, 'caller');
  assert.equal(run('refresh').reused, 2);
  const sourceBefore = fs.readFileSync(path.join(root, 'source.js'), 'utf8');
  fs.renameSync(path.join(bundle, 'native-bin'), path.join(bundle, 'disabled-native-bin'));
  assert.equal(run('status', [], false).enabled, false);
  assert.equal(fs.readFileSync(path.join(root, 'source.js'), 'utf8'), sourceBefore);
});
