import fs from 'node:fs';
import path from 'node:path';
import os from 'node:os';
import { fileURLToPath } from 'node:url';
import test from 'node:test';
import assert from 'node:assert/strict';
import { copy } from '../tooling/export/export.mjs';
import { assembleDistribution } from '../tooling/distribution.mjs';
import { validateMarketplace } from '../tooling/validate-marketplace.mjs';
import { build } from '../tooling/build.mjs';
import { files, loadCapabilities } from '../tooling/lib/capabilities.mjs';

const repo = fileURLToPath(new URL('../', import.meta.url));
const read = file => JSON.parse(fs.readFileSync(file, 'utf8'));

test('Codex marketplace resolves every plugin within the generated bundle', () => {
  const root = path.join(repo, 'dist/codex');
  const catalog = read(path.join(root, '.agents/plugins/marketplace.json'));
  assert.equal(catalog.name, 'engineering-pack');
  assert.deepEqual(catalog.plugins.map(p => p.name).sort(), loadCapabilities(repo).map(p => p.manifest.name).sort());
  for (const plugin of catalog.plugins) {
    assert.equal(plugin.source.source, 'local');
    assert.ok(plugin.source.path.startsWith('./'));
    const manifest = read(path.join(root, plugin.source.path, '.codex-plugin/plugin.json'));
    assert.equal(manifest.name, plugin.name);
    assert.equal(plugin.policy.installation, 'AVAILABLE');
    assert.ok(fs.existsSync(path.join(root, plugin.source.path, 'skills')));
  }
});

test('new distributions are reproducible and do not activate hooks or MCP', t => {
  const fixture = fs.mkdtempSync(path.join(os.tmpdir(), 'engineering-distributions-'));
  t.after(() => fs.rmSync(fixture, { recursive: true, force: true }));
  for (const directory of ['packages', 'hosts', 'tooling/export', 'engine', 'application', 'platform', 'integrations', 'cli', 'docs', 'schemas']) copy(path.join(repo, directory), path.join(fixture, directory));
  fs.copyFileSync(path.join(repo,'package.json'), path.join(fixture,'package.json'));
  for (const host of ['gemini-extension', 'jetbrains']) {
    build(fixture, host);
    const root = path.join(fixture, 'dist', host);
    const snapshot = () => files(root).map(file => [path.relative(root, file), fs.readFileSync(file, 'utf8')]);
    const before = snapshot();
    build(fixture, host);
    assert.deepEqual(snapshot(), before);
    const report = read(path.join(root, 'adapter-report.json'));
    assert.ok(report.packages.every(p => p.adapter === host));
    assert.ok(report.packages.every(p => ['none', 'unsupported-not-installed'].includes(p.hooks)));
    assert.ok(!files(root).some(file => /(?:hooks\.json|settings\.json|mcp\.json)$/.test(file)));
  }
  const gemini = path.join(repo, 'dist/gemini-extension');
  const manifest = read(path.join(gemini, 'gemini-extension.json'));
  assert.equal(manifest.name, 'engineering-pack');
  assert.equal(manifest.version, read(path.join(repo,'package.json')).version);
  assert.ok(fs.existsSync(path.join(gemini, manifest.contextFileName)));
  const skills = fs.readdirSync(path.join(gemini, 'skills'));
  assert.equal(skills.length, loadCapabilities(repo).reduce((sum, p) => sum + p.manifest.skills.length + p.manifest.workflows.length, 0));
  const rule = fs.readFileSync(path.join(repo, 'dist/jetbrains/.aiassistant/rules/engineering-pack.md'), 'utf8');
  assert.match(rule, /Generated from craft-engineering:foundations/);
  assert.match(rule, /design-principles/);
});

test('GitHub branch exposes all native entry points and reproducible plugin sources', t => {
  const fixture = fs.mkdtempSync(path.join(os.tmpdir(), 'github-distribution-'));
  t.after(() => fs.rmSync(fixture, { recursive: true, force: true }));
  for (const host of ['gemini-extension', 'claude', 'codex', 'jetbrains']) copy(path.join(repo, 'dist', host), path.join(fixture, 'dist', host));
  fs.copyFileSync(path.join(repo,'package.json'),path.join(fixture,'package.json'));
  const output = assembleDistribution(fixture);
  const snapshot = () => files(output).map(file => [path.relative(output, file), fs.readFileSync(file, 'utf8')]);
  const before = snapshot();
  assembleDistribution(fixture);
  assert.deepEqual(snapshot(), before);
  assert.deepEqual(validateMarketplace(output), []);
  assert.ok(fs.existsSync(path.join(output, 'gemini-extension.json')));
  assert.ok(fs.existsSync(path.join(output, 'jetbrains/.aiassistant/rules/engineering-pack.md')));
  const codex = read(path.join(output, '.agents/plugins/marketplace.json'));
  for (const plugin of codex.plugins) {
    assert.equal(read(path.join(output, plugin.source.path, '.codex-plugin/plugin.json')).name, plugin.name);
    assert.ok(fs.existsSync(path.join(output, plugin.source.path, 'bin/craft')));
  }
  const remoteLinks = files(output).filter(file => file.endsWith('.md')).flatMap(file => fs.readFileSync(file, 'utf8').match(/https:\/\/github\.com\/Aliaksie\/engineering-pack\/blob\/distribution[^)\s]*/g) ?? []);
  assert.ok(remoteLinks.some(link => link.startsWith('https://github.com/Aliaksie/engineering-pack/blob/distribution-next/')));
  assert.ok(remoteLinks.every(link => !link.startsWith('https://github.com/Aliaksie/engineering-pack/blob/distribution/')));
  assert.ok(!files(output).some(file => /(?:^|[/\\])(?:\.git|\.plan|node_modules|\.github)(?:[/\\]|$)/.test(path.relative(output, file))));
  fs.unlinkSync(path.join(output, '.engineering-distribution.json'));
  assert.throws(() => assembleDistribution(fixture), /unmanaged/);
});
