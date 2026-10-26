import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import os from 'node:os';
import { validateCatalogSources } from '../tooling/lib/playbook-catalog.mjs';
import { loadCapabilities } from '../tooling/lib/capabilities.mjs';

// The catalog shape is validated by `craft`, which embeds it (`cargo test -p craft-engine`).
const catalog = JSON.parse(fs.readFileSync(new URL('../engine/routing/catalog.json', import.meta.url), 'utf8'));
const copy = () => structuredClone(catalog);

test('published path selector pattern accepts canonical scopes and rejects traversal, globs and ambiguous paths', () => {
  const schema=JSON.parse(fs.readFileSync('schemas/playbook-catalog.schema.json','utf8'));
  const pattern=new RegExp(schema.$defs.entry.properties.when.properties.paths.items.pattern);
  for (const [valid,paths] of [
    [true,['.','a','.github/workflows','automation/release.yml']],
    [false,['','..','./ci','ci/../release','ci/./release','ci//release','ci/','/ci','C:/ci','ci\\release','ci/*.yml','ci/**','ci/[ab]','ci/{a,b}','ci?','ci\0file']]
  ]) for (const scope of paths) assert.equal(pattern.test(scope),valid,scope);
});

test('authoring resolves catalog targets against registered package skills and real references', (t) => {
  const root = path.resolve('.');
  const packages = loadCapabilities(root);
  assert.equal(validateCatalogSources(root, packages).entries.length, catalog.entries.length);
  assert.throws(() => validateCatalogSources(root, packages.filter((p) => p.manifest.name !== 'craft-delivery')), /unresolved catalog target/);
  assert.throws(() => validateCatalogSources(root, packages.filter((p) => p.manifest.name !== 'craft-engineering')), /unresolved catalog target/);
  const repo = fs.mkdtempSync(path.join(os.tmpdir(), 'catalog sources '));
  t.after(() => fs.rmSync(repo, { recursive: true, force: true }));
  fs.mkdirSync(path.join(repo, 'engine/routing'), { recursive:true });
  const changed = copy();
  changed.entries[0].references = ['craft-engineering:testing-engineering/references/absent.md'];
  fs.writeFileSync(path.join(repo, 'engine/routing/catalog.json'), JSON.stringify(changed));
  assert.throws(() => validateCatalogSources(repo, packages), /ENOENT/);
  const fallback = copy();
  fallback.entries.find(entry => entry.id === 'workflow:review').fallbackSkills = { 'craft-delivery:review-code': 'craft-delivery:absent' };
  fs.writeFileSync(path.join(repo, 'engine/routing/catalog.json'), JSON.stringify(fallback));
  assert.throws(() => validateCatalogSources(repo, packages), /unresolved catalog target: craft-delivery:absent/);
});

test('published schema and catalog agree on required contract fields', () => {
  const schema = JSON.parse(fs.readFileSync('schemas/playbook-catalog.schema.json', 'utf8'));
  assert.deepEqual(Object.keys(catalog).sort(), schema.required.toSorted());
  const required = schema.$defs.entry.properties.value.required;
  assert.deepEqual(Object.keys(catalog.entries[0].value).sort(), required.toSorted());
  const unknown = Object.keys(catalog.entries[0]).filter((key) => !Object.hasOwn(schema.$defs.entry.properties, key));
  assert.deepEqual(unknown, []);
});
