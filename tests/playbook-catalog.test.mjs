import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import os from 'node:os';
import { catalog } from '../application/routing/catalog.mjs';
import { validatePlaybookCatalog } from '../engine/contracts/catalog.mjs';
import { validateCatalogSources } from '../tooling/lib/playbook-catalog.mjs';
import { loadCapabilities } from '../tooling/lib/capabilities.mjs';

const copy = () => structuredClone(catalog);

test('catalog contract rejects ambiguous selectors, missing value and invalid identities', () => {
  const mutations = [
    (c) => { c.formatVersion = 2; },
    (c) => { c.entries.push(c.entries[0]); },
    (c) => { c.entries[0].id = 'vendor:implement'; },
    (c) => { c.entries[0].when = {}; },
    (c) => { c.entries[0].when.framework = ['spring']; },
    (c) => { c.entries[0].when.intents = ['invent']; },
    (c) => { c.entries[0].when.intents = ['implement', 'implement']; },
    (c) => { c.entries[0].when.languages = ['java']; },
    (c) => { c.entries[0].value.evidence = ' '; },
    (c) => { delete c.entries[0].value.inputs; },
    (c) => { c.entries[0].value.permission = 'publish'; },
    (c) => { c.entries.find((e) => e.id === 'runtime:jvm').when.intents = ['story', 'implement']; },
    (c) => { c.runtimeImplications.java = 'missing'; },
    (c) => { c.extensions['java'] = 'java'; },
    (c) => { c.layerExtensions['.tsx'] = 'react'; }
  ];
  for (const mutation of mutations) {
    const changed = copy(); mutation(changed);
    assert.throws(() => validatePlaybookCatalog(changed), undefined, mutation.toString());
  }
});

test('catalog distinguishes context identity, unavailable guidance and actual load targets', () => {
  assert.equal(validatePlaybookCatalog(copy()).formatVersion, 1);
  const changed = copy();
  delete changed.entries.find((e) => e.id === 'runtime:node').contextOnly;
  assert.throws(() => validatePlaybookCatalog(changed), /requires load targets/);
  for (const target of ['other:missing', 'craft-engineering:foundations/../../secret.md']) {
    const c = copy(); c.entries[0].references = [target];
    assert.throws(() => validatePlaybookCatalog(c), /references/);
  }
  assert.throws(() => validatePlaybookCatalog(copy(), { availableTargets: new Set() }), /unresolved catalog target/);
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
});

test('published schema and runtime agree on required contract fields', () => {
  const schema = JSON.parse(fs.readFileSync('schemas/playbook-catalog.schema.json', 'utf8'));
  assert.deepEqual(Object.keys(catalog).sort(), schema.required.toSorted());
  const required = schema.$defs.entry.properties.value.required;
  assert.deepEqual(Object.keys(catalog.entries[0].value).sort(), required.toSorted());
  const unknown = Object.keys(catalog.entries[0]).filter((key) => !Object.hasOwn(schema.$defs.entry.properties, key));
  assert.deepEqual(unknown, []);
});
