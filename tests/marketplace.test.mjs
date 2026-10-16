import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import test from 'node:test';
import assert from 'node:assert/strict';
import { fileURLToPath } from 'node:url';
import { validateCatalog, validateMarketplace } from '../scripts/validate-marketplace.mjs';
import { readFrontmatter, validatePlugin } from '../scripts/lib/plugin-manifest.mjs';

const built = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '../dist/claude');
function fixture(t) {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'marketplace-validation-'));
  t.after(() => fs.rmSync(root, { recursive: true, force: true }));
  const put = (name, content) => {
    const file = path.join(root, name);
    fs.mkdirSync(path.dirname(file), { recursive: true });
    fs.writeFileSync(file, typeof content === 'string' ? content : JSON.stringify(content));
    return file;
  };
  const manifest = { name: 'sample-plugin', version: '0.0.1', description: 'A sample.', keywords: ['review-snapshot'] };
  const plugin = { name: manifest.name, source: './plugins/sample-plugin', version: manifest.version };
  const catalog = { name: 'sample-marketplace', owner: { name: 'Sample' }, plugins: [plugin] };
  const manifestPath = put('plugins/sample-plugin/.claude-plugin/plugin.json', manifest);
  put('plugins/sample-plugin/README.md', 'A review snapshot guides TypeScript work.\n');
  put('plugins/sample-plugin/skills/sample-skill/SKILL.md', '---\nname: sample-skill\ndescription: A skill.\n---\nBody.\n');
  put('.claude-plugin/marketplace.json', catalog);
  return { root, put, plugin, catalog, manifest, manifestPath };
}

test('generated marketplace validates and lists exactly its plugin directories', () => {
  assert.deepEqual(validateMarketplace(built), []);
  const catalog = JSON.parse(fs.readFileSync(path.join(built, '.claude-plugin/marketplace.json'), 'utf8'));
  const directories = fs.readdirSync(path.join(built, 'plugins'), { withFileTypes: true }).filter((entry) => entry.isDirectory()).map((entry) => entry.name).sort();
  assert.deepEqual(catalog.plugins.map((entry) => entry.name).sort(), directories);
  const events = new Set(['PreToolUse', 'PostToolUse', 'PostToolUseFailure', 'PermissionRequest', 'UserPromptSubmit',
    'Notification', 'Stop', 'SubagentStart', 'SubagentStop', 'PreCompact', 'SessionStart', 'SessionEnd']);
  for (const entry of catalog.plugins) {
    const manifest = JSON.parse(fs.readFileSync(path.join(built, entry.source, '.claude-plugin/plugin.json'), 'utf8'));
    const hookFile = path.join(built, entry.source, manifest.hooks ?? 'hooks/hooks.json');
    if (!fs.existsSync(hookFile)) continue;
    const hooks = JSON.parse(fs.readFileSync(hookFile, 'utf8')).hooks ?? {};
    for (const event of Object.keys(hooks)) assert.ok(events.has(event), `${entry.name}: unsupported hook event ${event}`);
  }
});

test('catalog validation handles malformed entries and JSON values without throwing', (t) => {
  const { root, catalog, plugin } = fixture(t);
  for (const malformed of [null, 1, 'plugin', [], false]) {
    assert.ok(validateCatalog(root, { ...catalog, plugins: [malformed] }).some((error) => error.includes('must be an object')));
    const errors = [];
    assert.doesNotThrow(() => validatePlugin(root, malformed, errors));
    assert.ok(errors.length);
  }
  for (const malformed of [null, 1, [], {}, { plugins: [] }]) assert.ok(validateCatalog(root, malformed).length);
  assert.ok(validateCatalog(root, { ...catalog, plugins: [plugin, plugin] }).some((error) => error.includes('duplicate plugin name')));
  assert.ok(validateCatalog(root, { ...catalog, plugins: [{ ...plugin, tags: {} }] }).some((error) => error.includes('must be an array')));
});

test('plugin identity, version and description must agree with catalog declarations', (t) => {
  const { root, catalog, plugin, manifest, put } = fixture(t);
  const { version, ...unversioned } = plugin;
  assert.ok(validateCatalog(root, { ...catalog, plugins: [unversioned] }).some((error) => error.includes('marketplace entry has no version')));
  put('plugins/sample-plugin/.claude-plugin/plugin.json', { ...manifest, name: 'different-plugin', version: '0.0.2', description: '' });
  const errors = validateCatalog(root, catalog).join('\n');
  for (const expected of ['names differ', 'versions differ', 'description is required']) assert.ok(errors.includes(expected));
  put('plugins/sample-plugin/.claude-plugin/plugin.json', []);
  assert.ok(validateCatalog(root, catalog).some((error) => error.includes('manifest must be an object')));
});

test('advertised terms require ordered evidence outside plugin metadata', (t) => {
  const { root, catalog, plugin, manifest, put } = fixture(t);
  const withTags = (tags) => validateCatalog(root, { ...catalog, plugins: [{ ...plugin, tags }] });
  assert.deepEqual(withTags(['review-snapshot', 'typescript']), []);
  assert.ok(withTags(['snapshot-review']).some((error) => error.includes('appears nowhere')));
  put('plugins/sample-plugin/.claude-plugin/plugin.json', { ...manifest, keywords: ['absent-evidence'] });
  assert.ok(withTags(['absent-evidence']).filter((error) => error.includes('appears nowhere')).length === 2);
  put('plugins/sample-plugin/.claude-plugin/plugin.json', { ...manifest, keywords: {} });
  assert.ok(withTags([]).some((error) => error.includes('keyword declarations must be an array')));
});

test('local plugin sources reject traversal, noncanonical paths and symlink directories', (t) => {
  const { root, catalog, plugin } = fixture(t);
  for (const source of ['../outside', './plugins/../sample-plugin', './plugins//sample-plugin', './plugins\\sample-plugin', './plugins/./sample-plugin']) {
    assert.ok(validateCatalog(root, { ...catalog, plugins: [{ ...plugin, source }] }).length);
  }
  fs.symlinkSync(path.join(root, 'plugins/sample-plugin'), path.join(root, 'shortcut'), 'junction');
  assert.ok(validateCatalog(root, { ...catalog, plugins: [{ ...plugin, source: './shortcut' }] }).some((error) => error.includes('symbolic link')));
});

test('plugin scans report linked content without following external files or directories', (t) => {
  const { root, catalog, plugin, put } = fixture(t);
  put('external/README.md', 'Unique external phrase.\n`sample-plugin:absent-component`\n');
  fs.symlinkSync(path.join(root, 'external'), path.join(root, 'plugins/sample-plugin/linked'), 'junction');
  const errors = validateCatalog(root, { ...catalog, plugins: [{ ...plugin, tags: ['unique-external-phrase'] }] });
  assert.ok(errors.some((error) => error.includes('symbolic links')));
  assert.ok(errors.some((error) => error.includes('appears nowhere')));
  assert.ok(!errors.some((error) => error.includes('does not ship')));
});

test('missing skill documents and unsafe agent permissions produce diagnostics', (t) => {
  const { root, catalog, put } = fixture(t);
  fs.mkdirSync(path.join(root, 'plugins/sample-plugin/skills/empty-skill'));
  put('plugins/sample-plugin/agents/worker.md', '---\nname: worker\npermissionMode: bypassPermissions\n---\nBody.\n');
  const errors = validateCatalog(root, catalog).join('\n');
  assert.match(errors, /empty-skill is missing SKILL.md/);
  assert.match(errors, /description is required/);
  assert.match(errors, /bypassPermissions is prohibited/);
});

test('frontmatter supports CRLF sequences and keeps nested text out of top-level fields', (t) => {
  const { put } = fixture(t);
  const file = put('header.md', '---\r\nname: worker\r\ndescription: >-\r\n  Detail: text\r\nskills:\r\n  - sample-skill\r\n---\r\n');
  const errors = [];
  const fields = readFrontmatter(file, errors);
  assert.deepEqual(errors, []);
  assert.equal(fields.description, '>-');
  assert.deepEqual(fields.skills, ['sample-skill']);
  assert.equal(fields.Detail, undefined);
});

test('catalog loading reports missing, malformed and symlinked metadata', (t) => {
  const { root, put } = fixture(t);
  put('.claude-plugin/marketplace.json', '{');
  assert.ok(validateMarketplace(root).some((error) => error.includes('invalid JSON')));
  fs.unlinkSync(path.join(root, '.claude-plugin/marketplace.json'));
  assert.ok(validateMarketplace(root).some((error) => error.includes('missing')));
  put('external-catalog/marketplace.json', {});
  fs.rmSync(path.join(root, '.claude-plugin'), { recursive: true });
  fs.symlinkSync(path.join(root, 'external-catalog'), path.join(root, '.claude-plugin'), 'junction');
  assert.ok(validateMarketplace(root).some((error) => error.includes('symbolic links')));
});
